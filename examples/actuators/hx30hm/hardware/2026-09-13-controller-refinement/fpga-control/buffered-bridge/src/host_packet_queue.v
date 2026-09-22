// Collect complete checksum-valid host packets while the servo bus is occupied.
// STOP/DISARM have a separate priority slot and an immediate supervisor pulse.
// A priority stop or malformed/overflowed packet discards older queued work.
module host_packet_queue #(
    parameter DEPTH=2,
    parameter INTERBYTE_TIMEOUT=500000
)(
    input wire clk,rst,
    input wire [7:0] data,
    input wire data_valid,
    output wire [511:0] packet,
    output wire [6:0] packet_length,
    output wire packet_valid,
    input wire packet_ready,
    output reg urgent_stop=0,
    output reg fault=0
);
    localparam BITS=$clog2(DEPTH);
    reg [511:0] packets[0:DEPTH-1];
    reg [6:0] lengths[0:DEPTH-1];
    reg [BITS-1:0] wr=0,rd=0;
    reg [BITS:0] count=0;
    reg [511:0] priority_packet=0;
    reg [6:0] priority_length=0;
    reg priority_valid=0;
    reg [1:0] state=0;
    reg [7:0] bytes[0:63];
    reg [6:0] index=0,total=0;
    reg [7:0] sum=0;
    reg [$clog2(INTERBYTE_TIMEOUT+1)-1:0] age=0;
    wire [511:0] assembled;
    genvar i;
    generate for(i=0;i<64;i=i+1) begin: collect
        assign assembled[i*8 +: 8]=i==index ? data : i<index ? bytes[i] : 8'd0;
    end endgenerate
    wire complete=data_valid && state==2 && index>=4 && index+1==total;
    wire [7:0] checksum=sum+data;
    wire valid=complete && checksum==255;
    wire priority_stop=valid && assembled[23:16]==254 && assembled[39:32]==8'ha0
        && ((total==7 && assembled[47:40]==0)
            || (total==8 && assembled[47:40]==4 && assembled[55:48]>=4 && assembled[55:48]<=12));
    assign packet_valid=priority_valid || count!=0;
    assign packet=priority_valid ? priority_packet : packets[rd];
    assign packet_length=priority_valid ? priority_length : lengths[rd];
    wire pop=packet_valid && packet_ready;
    wire pop_normal=pop && !priority_valid;
    wire malformed=(complete && !valid)
        || (data_valid && state==2 && index==3 && (data<2 || data>60))
        || (state!=0 && !data_valid && age==INTERBYTE_TIMEOUT-1);
    wire overflow=valid && !priority_stop && count==DEPTH && !pop_normal;
    wire push=valid && !priority_stop && !overflow;
    always @(posedge clk) begin
        urgent_stop<=0;fault<=0;
        if(rst) begin wr<=0;rd<=0;count<=0;priority_valid<=0;state<=0;age<=0;index<=0;total<=0;sum<=0;end
        else begin
            if(state!=0 && !data_valid) age<=age+1;else age<=0;
            if(data_valid) begin
                case(state)
                    0: if(data==255) begin bytes[0]<=255;state<=1;end
                    1: if(data==255) begin bytes[1]<=255;state<=2;index<=2;sum<=0;end else state<=0;
                    2: begin
                        bytes[index]<=data;sum<=sum+data;
                        if(index==3) total<=data+4;
                        if(complete) state<=0;else index<=index+1;
                    end
                    default: state<=0;
                endcase
            end
            if(pop && priority_valid) priority_valid<=0;
            if(pop_normal) rd<=rd+1'b1;
            if(push) begin packets[wr]<=assembled;lengths[wr]<=total;wr<=wr+1'b1;end
            case({push,pop_normal})
                2'b10: count<=count+1'b1;
                2'b01: count<=count-1'b1;
                default: ;
            endcase
            if(priority_stop) begin
                priority_packet<=assembled;priority_length<=total;priority_valid<=1;
                urgent_stop<=1;wr<=0;rd<=0;count<=0;
            end
            if(malformed || overflow) begin
                fault<=1;wr<=0;rd<=0;count<=0;state<=0;age<=0;
                // A previously accepted STOP remains pending even after a bad later frame.
            end
        end
    end
endmodule
