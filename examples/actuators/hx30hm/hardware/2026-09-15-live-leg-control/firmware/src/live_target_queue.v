// Finite live references for exactly three selected axes. No ARM or lease renewal.
// A4/0: version=1, run u32, frames u16; uses a sealed 10 ms plan's homes/gains.
// A4/1: run u32, first u16, count u8, count*(three signed i16 offsets).
// Future rows are immutable. An entire batch validates before any row is committed.
module live_target_queue #(
    parameter PERIOD=500000, MAX_FRAMES=1200, FRESH_TICKS=12500000, ALLOWED_MASK=448
)(
    input wire clk,rst,check,packet_ok,transport_fault,locked,active,start,
    input wire [511:0] packet,
    input wire [6:0] packet_length,
    input wire base_valid,
    input wire [8:0] mask,
    input wire [31:0] period,
    input wire [143:0] homes,
    input wire [15:0] read_index,
    output wire handled,busy,row_ready,
    output reg enabled=0,
    output reg fault=0,
    output reg [31:0] run_id=0,
    output reg [15:0] frames=0,
    output reg [15:0] written=0,
    output wire [143:0] targets,deltas
);
    wire selected=check && packet[23:16]==254 && packet[39:32]==8'ha4;
    assign handled=selected;
    wire framing=packet_ok && packet[15:0]==16'hffff && {1'b0,packet[31:24]}+9'd4==packet_length;
    wire configure=selected && framing && packet[47:40]==0 && packet_length==14 && packet[55:48]==1;
    wire append=selected && framing && packet[47:40]==1;
    wire [31:0] input_run=packet[79:48];
    wire [15:0] input_first=packet[95:80];
    wire [7:0] input_count=packet[103:96];
    wire [16:0] input_end={1'b0,input_first}+{9'd0,input_count};
    wire [15:0] consumed=active ? read_index : 0;
    function [3:0] motor_count;
        input [8:0] bits;
        integer n;
        begin motor_count=0;for(n=0;n<9;n=n+1)motor_count=motor_count+bits[n];end
    endfunction
    localparam IDLE=0,VALIDATE=1,COMMIT=2;
    reg [1:0] state=IDLE;
    reg [2:0] row=0;
    reg [3:0] count=0;
    reg [1:0] axis=0;
    reg [15:0] first=0;
    reg [383:0] payload=0;
    reg [47:0] previous=0,scratch_previous=0;
    reg [44:0] memory[0:15];
    reg [44:0] data_out=0;
    reg [15:0] read_tag=0;
    reg [23:0] freshness=0;
    wire signed [15:0] offset=payload[(row*3+axis)*16 +: 16];
    wire signed [16:0] change=$signed(offset)-$signed(scratch_previous[axis*16 +: 16]);
    wire [2:0] previous_row=row-1'b1;
    wire [47:0] commit_offsets=payload[row*48 +: 48];
    wire [47:0] prior_offsets=row==0 ? previous : payload[previous_row*48 +: 48];
    wire [44:0] commit_data;
    genvar c;
    generate for(c=0;c<3;c=c+1)begin: packed_axis
        wire signed [16:0] difference=$signed(commit_offsets[c*16 +: 16])-$signed(prior_offsets[c*16 +: 16]);
        assign commit_data[c*15 +: 8]=commit_offsets[c*16 +: 8];
        assign commit_data[c*15+8 +: 7]=difference[6:0];
    end endgenerate
    wire bad_offset=offset>80 || offset< -80 || change>32 || change< -32
        || (first+row==0 && offset!=0);
    assign busy=state!=IDLE;
    assign row_ready=enabled && read_index<frames && read_index<written && read_tag==read_index;
    genvar lane_id;
    generate for(lane_id=0;lane_id<9;lane_id=lane_id+1)begin: axis_output
        if((ALLOWED_MASK & (1<<lane_id))!=0)begin
            localparam LANE=motor_count(ALLOWED_MASK & ((1<<lane_id)-1));
            assign targets[lane_id*16 +: 16]={4'd0,homes[lane_id*16 +: 12]}+{{8{data_out[LANE*15+7]}},data_out[LANE*15 +: 8]};
            assign deltas[lane_id*16 +: 16]={{9{data_out[LANE*15+14]}},data_out[LANE*15+8 +: 7]};
        end else begin
            assign targets[lane_id*16 +: 16]=0;
            assign deltas[lane_id*16 +: 16]=0;
        end
    end endgenerate
    task fail;
        begin enabled<=0;state<=IDLE;fault<=1;end
    endtask
    always @(posedge clk)begin
        data_out<=memory[read_index[3:0]];read_tag<=read_index;
        fault<=0;
        if(rst)begin enabled<=0;state<=IDLE;written<=0;freshness<=0;end
        else if(transport_fault || (enabled && (!base_valid || (active && freshness==FRESH_TICKS-1)))) fail;
        else begin
            if(active && freshness<FRESH_TICKS) freshness<=freshness+1;
            if(start)freshness<=0;
            if(configure)begin
                if(locked || busy || !base_valid || period!=PERIOD || motor_count(mask)!=3 || mask!=ALLOWED_MASK
                    || packet[103:88]<2 || packet[103:88]>MAX_FRAMES)fail;
                else begin
                    enabled<=1;run_id<=packet[87:56];frames<=packet[103:88];
                    written<=0;previous<=0;freshness<=0;
                end
            end else if(selected)begin
                if(!append || busy || !enabled || input_run!=run_id || input_first!=written
                    || input_count==0 || input_count>8 || packet_length!=14+input_count*6
                    || input_end>{1'b0,frames} || input_end>{1'b0,consumed}+17'd16)fail;
                else begin
                    payload<=packet[487:104];first<=input_first;count<=input_count[3:0];row<=0;axis<=0;
                    scratch_previous<=previous;state<=VALIDATE;
                end
            end else case(state)
                VALIDATE: if(bad_offset)fail;
                    else begin
                        scratch_previous[axis*16 +: 16]<=offset;
                        if(axis==2)begin
                            axis<=0;
                            if(row+1==count)begin row<=0;state<=COMMIT;end else row<=row+1;
                        end else axis<=axis+1;
                    end
                COMMIT: begin
                    memory[(first+row)&15]<=commit_data;
                    if(row+1==count)begin
                        written<=first+count;previous<=scratch_previous;freshness<=0;state<=IDLE;
                    end else row<=row+1;
                end
                default:;
            endcase
        end
    end
endmodule
