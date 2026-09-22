// Sealed trajectory memory. CRC32 covers the canonical 34-byte configuration,
// then 36 bytes per row, little endian. No execution or arming occurs here.
module trajectory_store #(
    parameter MAX_FRAMES=256,MIN_PERIOD=2000000,FAST_MIN_PERIOD=500000,MAX_PERIOD=7500000,MAX_DURATION=600000000
)(
    input wire clk,rst,active,invalidate_plan,
    input wire configure,write_row,seal,
    input wire [8:0] input_mask,input_frames,
    input wire [31:0] input_period,
    input wire [15:0] input_kp,input_kd,input_kv,input_limit,
    input wire [143:0] input_homes,
    input wire [8:0] write_index,read_index,
    input wire [287:0] write_data,
    input wire [31:0] expected_crc32,
    output reg [8:0] mask=0,
    output reg [8:0] frames=0,
    output reg [31:0] period=0,
    output reg [15:0] kp=0,
    output reg [15:0] kd=0,
    output reg [15:0] kv=0,
    output reg [15:0] duty_limit=0,
    output reg [143:0] homes=0,
    output reg valid=0,
    output reg fault=0,
    output wire busy,row_ready,
    output wire [143:0] row_targets,row_deltas,
    output reg [31:0] crc32=0,
    output reg [8:0] written=0
);
    localparam IDLE=0,HEADER=1,WAIT_ROW=2,CHECK_ROW=3,CRC_ROW=4;
    reg [2:0] state=IDLE;
    reg configured=0;
    localparam ADDRESS_BITS=$clog2(MAX_FRAMES);
    reg [287:0] memory[0:MAX_FRAMES-1];
    reg [287:0] data_out;
    reg [8:0] read_tag=0,scan=0;
    reg [5:0] byte_index=0;
    reg [3:0] axis=0;
    reg [143:0] previous_targets=0;
    reg [31:0] crc=32'hffffffff,expected=0;
    wire [8:0] address=state==IDLE ? read_index : scan;
    wire collision=(configure && (write_row || seal)) || (write_row && seal);
    // The separately checked period bound is below 2^23; 9x23 bits fit u32.
    wire [31:0] compact_duration=input_frames*input_period[22:0];
    wire [63:0] total_ticks=MAX_PERIOD<8388608 ? {32'd0,compact_duration} : {55'd0,input_frames}*input_period;
    function [3:0] motor_count;
        input [8:0] bits;
        integer n;
        begin motor_count=0;for(n=0;n<9;n=n+1)motor_count=motor_count+bits[n];end
    endfunction
    wire config_ok=input_mask!=0 && input_frames>=2 && input_frames<=MAX_FRAMES
        && (input_period>=MIN_PERIOD || (motor_count(input_mask)<=3 && input_period>=FAST_MIN_PERIOD))
        && input_period<=MAX_PERIOD && total_ticks<=MAX_DURATION
        && input_kp<=4096 && input_kd<=4096 && input_kv<=4096 && input_limit<=1000
        && (input_limit<=100 || input_period<=2000000);
    wire accept_write=!rst && !invalidate_plan && write_row && configured && !valid && !active && state==IDLE
        && !collision && write_index==written && write_index<frames;
    always @(posedge clk) begin
        if(accept_write) memory[write_index[ADDRESS_BITS-1:0]]<=write_data;
        data_out<=memory[address[ADDRESS_BITS-1:0]];
        read_tag<=address;
    end
    assign busy=state!=IDLE;
    assign row_ready=valid && state==IDLE && read_tag==read_index && read_index<frames
        && !configure && !write_row && !seal && !invalidate_plan;
    genvar i;
    generate for(i=0;i<9;i=i+1) begin: rows
        assign row_targets[i*16 +: 16]=data_out[i*32 +: 16];
        assign row_deltas[i*16 +: 16]=data_out[i*32+16 +: 16];
    end endgenerate
    wire [271:0] header={homes,duty_limit,kv,kd,kp,period,7'd0,frames,7'd0,mask};
    function [31:0] crc_byte;
        input [31:0] value;
        input [7:0] b;
        integer k;
        reg [31:0] c;
        begin c=value^b;for(k=0;k<8;k=k+1)c=(c>>1)^(c[0] ? 32'hedb88320 : 0);crc_byte=c;end
    endfunction
    wire [31:0] next_crc=crc_byte(crc,state==HEADER ? header[byte_index*8 +: 8] : data_out[byte_index*8 +: 8]);
    wire [15:0] target=data_out[axis*32 +: 16];
    wire signed [15:0] delta=data_out[axis*32+16 +: 16];
    wire [15:0] home=homes[axis*16 +: 16];
    wire signed [31:0] offset=$signed({1'b0,target})-$signed({1'b0,home});
    wire signed [31:0] change=$signed({1'b0,target})-$signed({1'b0,previous_targets[axis*16 +: 16]});
    wire bad_axis=mask[axis] ? (home<600 || home>3495 || target>4095 || offset>80 || offset< -80
        || delta>32 || delta< -32 || change!=delta || (scan==0 && (target!=home || delta!=0)))
        : (home!=0 || target!=0 || delta!=0);
    task invalidate;
        begin valid<=0;configured<=0;state<=IDLE;fault<=1;end
    endtask
    always @(posedge clk) begin
        fault<=0;
        if(rst) begin valid<=0;configured<=0;state<=IDLE;written<=0;crc32<=0;end
        else if(invalidate_plan || collision || ((configure || write_row || seal) && (active || state!=IDLE))) invalidate;
        else if(configure) begin
            valid<=0;written<=0;crc32<=0;
            if(!config_ok) invalidate;
            else begin
                configured<=1;mask<=input_mask;frames<=input_frames;period<=input_period;
                kp<=input_kp;kd<=input_kd;kv<=input_kv;duty_limit<=input_limit;homes<=input_homes;
            end
        end else if(write_row) begin
            if(!accept_write) invalidate;else written<=written+1;
        end else if(seal) begin
            if(!configured || valid || written!=frames) invalidate;
            else begin crc<=32'hffffffff;expected<=expected_crc32;byte_index<=0;scan<=0;previous_targets<=homes;state<=HEADER;end
        end else case(state)
            HEADER: begin
                crc<=next_crc;
                if(byte_index==33) begin byte_index<=0;state<=WAIT_ROW;end else byte_index<=byte_index+1;
            end
            WAIT_ROW: begin axis<=0;state<=CHECK_ROW;end
            CHECK_ROW: if(bad_axis) invalidate;
                else begin
                    previous_targets[axis*16 +: 16]<=target;
                    if(axis==8) begin byte_index<=0;state<=CRC_ROW;end else axis<=axis+1;
                end
            CRC_ROW: begin
                crc<=next_crc;
                if(byte_index==35) begin
                    if(scan+1==frames) begin
                        crc32<=~next_crc;
                        if(~next_crc!=expected) invalidate;
                        else begin valid<=1;state<=IDLE;end
                    end else begin scan<=scan+1;state<=WAIT_ROW;end
                end else byte_index<=byte_index+1;
            end
            default: ;
        endcase
    end
endmodule
