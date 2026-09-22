// Bounded, lossless single-record serializer for the device-clock stream.
// record_ready reserves the entire record; packet_valid persists until consumed.
// STOP must not reset this module: accepted evidence drains after stopping.
module experiment_event_packet(
    input wire clk,rst,
    input wire record_valid,
    output wire record_ready,
    input wire [2:0] kind,
    input wire [31:0] run_id,
    input wire [8:0] frame,
    input wire [7:0] motor_id,outcome,sequence,device_error,
    input wire [63:0] request_ticks,completion_ticks,
    input wire [5:0] reported_width,
    input wire [4:0] stored_width,
    input wire [215:0] data,
    output wire packet_valid,
    input wire packet_ready,
    output reg [511:0] packet=0,
    output reg [6:0] packet_length=0,
    output reg fault=0
);
    localparam IDLE=0,CHECKSUM=1,READY=2;
    reg [1:0] state=IDLE;
    reg [6:0] index=0;
    reg [7:0] sum=0;
    integer i;
    wire global_record=(kind==0 || kind==2 || kind==4);
    wire [5:0] expected_width=kind==0 ? 16 : kind==1 ? 15 : kind==2 ? 18
        : kind==3 ? (reported_width>15 ? 15 : reported_width) : 27;
    wire valid_terminal=data[63:0]>=request_ticks && data[127:64]>=data[63:0]
        && data[127:64]==completion_ticks && data[215:208]<=1
        && (data[215:208]==0 ? data[207:128]==0 :
            (data[199:192]<=2 && data[191:128]>=request_ticks && data[191:128]<=data[63:0]
             && (data[199:192]==1 ? data[207:200]==254 : data[207:200]>=4 && data[207:200]<=12)))
        && (outcome!=0 || data[215:208]==0);
    wire allowed=kind<=4 && !frame[8] && completion_ticks>=request_ticks
        && (kind==0 || kind==4 || completion_ticks>request_ticks)
        && (global_record ? motor_id==254 : motor_id>=4 && motor_id<=12)
        && (kind==1 || sequence==0)
        && stored_width==expected_width
        && (kind==3 ? (reported_width<=58 && outcome<=1
            && (outcome!=0 || (reported_width==6 && device_error==0)))
            : (reported_width==expected_width && device_error==0))
        && (kind==4 ? outcome<=8 : kind==3 || outcome==0)
        && (kind!=0 || (frame==0 && request_ticks==completion_ticks))
        && (kind!=4 || valid_terminal);
    assign record_ready=state==IDLE;
    assign packet_valid=state==READY;
    always @(posedge clk) begin
        fault<=0;
        if(rst) begin state<=IDLE;index<=0;packet<=0;packet_length<=0;end
        else case(state)
            IDLE: if(record_valid) begin
                if(!allowed) fault<=1;
                else begin
                    packet<={8'd0,data,3'd0,stored_width,2'd0,reported_width,device_error,
                        completion_ticks,request_ticks,sequence,outcome,motor_id,
                        7'd0,frame,run_id,5'd0,kind,8'ha3,8'd1,
                        8'd0,(8'd33+{3'd0,stored_width}),8'hfd,16'hffff};
                    // Keep one packet bank. Scan its bytes for checksum rather
                    // than copying a second payload bank through a wide writer.
                    for(i=0;i<27;i=i+1) if(i>=stored_width) packet[(36+i)*8 +: 8]<=0;
                    packet_length<=7'd37+stored_width;index<=2;sum<=0;state<=CHECKSUM;
                end
            end
            CHECKSUM: begin
                if(index==packet_length-1'b1) begin
                    packet[index*8 +: 8]<=~sum;state<=READY;
                end else begin sum<=sum+packet[index*8 +: 8];index<=index+1'b1;end
            end
            READY: if(packet_ready) state<=IDLE;
            default:state<=IDLE;
        endcase
    end
endmodule
