// Associates raw servo replies with one accepted autonomous read transaction.
// This does NOT refresh watchdogs or accept a telemetry sample for control;
// hx_safety remains the authority for healthy/fresh feedback. Raw bytes must
// also reach the host capture so rejected/partial packets remain evidence.
module experiment_reply #(
    parameter RX_TIMEOUT=100000 // 2 ms inter-byte timeout at 50 MHz
)(
    input wire clk,rst,cancel,
    // Pulse only when the complete request owns the bus. Kinds match scheduler.
    input wire request_accepted,
    input wire [1:0] request_kind,
    input wire [7:0] request_id,
    // Exact on-wire register encoding, including direction bit, for audit ID.
    input wire [15:0] expected_pwm,
    input wire servo_valid,
    input wire [7:0] servo_data,
    output reg pending=0,
    output reg reply_valid=0,
    output reg reply_ok=0,
    output reg [1:0] reply_kind=0,
    output reg [7:0] reply_id=0,
    output reg [7:0] reply_error=0,
    output reg [5:0] reply_width=0,
    // First 15 payload bytes, zero padded; no fabricated current/voltage units.
    output reg [119:0] reply_data=0,
    output reg fault=0
);
    localparam HEADER1=0,HEADER2=1,BODY=2;
    reg [1:0] state=HEADER1,kind=0;
    reg [7:0] id=0,received_id=0,status=0,sum=0;
    reg [15:0] pwm=0;
    reg [6:0] index=0,total=0;
    reg [119:0] payload=0;
    reg [$clog2(RX_TIMEOUT+1)-1:0] age=0;
    wire [7:0] checksum=sum+servo_data;
    wire correct_width=(kind==0 && total==21) || (kind==2 && total==12);
    wire audit_matches=payload[7:0]==1 && payload[47:32]==pwm;
    always @(posedge clk) begin
        reply_valid<=0;fault<=0;
        if(rst) begin
            pending<=0;state<=HEADER1;age<=0;
            reply_ok<=0;reply_data<=0;reply_width<=0;
        end else if(cancel) begin
            // Completed data may still be queued for the logger after STOP.
            pending<=0;state<=HEADER1;age<=0;
        end else if(request_accepted) begin
            // Overlapping requests are an adapter error, never a new identity
            // for an old response. A control request has no servo ACK here.
            if(pending || request_kind==3
                || (request_kind==1 && request_id!=254)
                || (request_kind!=1 && (request_id<4 || request_id>12))) begin
                fault<=1;pending<=0;state<=HEADER1;
            end else begin
                pending<=request_kind!=1;kind<=request_kind;id<=request_id;
                pwm<=expected_pwm;state<=HEADER1;age<=0;
                reply_ok<=0;reply_data<=0;reply_width<=0;
            end
        end else if(pending) begin
            if(servo_valid) age<=0;
            else if(age<RX_TIMEOUT) age<=age+1'b1;
            // Do not consume a byte using stale parser state at timeout.
            // An FF at the boundary can be the start of a new frame.
            if(age>=RX_TIMEOUT) begin
                state<=servo_valid && servo_data==255 ? HEADER2 : HEADER1;
            end else if(servo_valid) case(state)
                HEADER1: if(servo_data==255) state<=HEADER2;
                HEADER2: if(servo_data==255) begin
                    state<=BODY;index<=2;sum<=0;payload<=0;
                end else state<=HEADER1;
                BODY: begin
                    sum<=checksum;
                    if(index==2) received_id<=servo_data;
                    if(index==3) begin
                        if(servo_data<2 || servo_data>60) state<=HEADER1;
                        else begin total<={1'b0,servo_data[5:0]}+7'd4;index<=4;end
                    end else if(index>3 && index+1'b1==total) begin
                        state<=HEADER1;
                        if(checksum==255 && received_id==id) begin
                            pending<=0;reply_valid<=1;reply_kind<=kind;reply_id<=id;
                            reply_error<=status;reply_width<=total-7'd6;reply_data<=payload;
                            reply_ok<=status==0 && correct_width && (kind==0 || audit_matches);
                            // Failed matching replies retain their payload and
                            // stop the adapter. Bad CRC/foreign replies cannot
                            // complete a request; the scheduler deadline applies.
                            if(status!=0 || !correct_width || (kind==2 && !audit_matches)) fault<=1;
                        end
                    end else begin
                        if(index==4) status<=servo_data;
                        else if(index>=5 && index<20) payload[(index-5)*8 +: 8]<=servo_data;
                        index<=index+1'b1;
                    end
                end
                default:state<=HEADER1;
            endcase
        end
    end
endmodule
