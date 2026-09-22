// Converts a scheduler transaction into one complete bridge packet. The A1
// packet contains setpoints/gains, never host-computed PWM. Request acceptance
// occurs only when the downstream bridge accepts the complete packet.
module experiment_packet(
    input wire clk,rst,
    input wire request_valid,
    input wire [1:0] request_kind,
    input wire [7:0] request_id,
    input wire [8:0] mask,
    input wire [15:0] kp,kd,kv,duty_limit,
    input wire [143:0] targets,deltas,
    output wire request_ready,
    output wire packet_valid,
    input wire packet_ready,
    output reg [511:0] packet=0,
    output reg [6:0] packet_length=0,
    output reg fault=0
);
    localparam IDLE=0,BUILD=1,READY=2,REJECT=3;
    reg [1:0] state=IDLE;
    reg [1:0] kind=0;
    reg [7:0] id=0;
    reg [8:0] saved_mask=0;
    reg [15:0] saved_kp=0,saved_kd=0,saved_kv=0,saved_limit=0;
    reg [143:0] saved_targets=0,saved_deltas=0;
    reg [6:0] index=0;
    reg [7:0] sum=0,value;
    reg [3:0] axis;
    wire allowed=mask!=0 && ((request_kind==1 && request_id==254)
        || ((request_kind==0 || request_kind==2) && request_id>=4 && request_id<=12 && mask[request_id-4]));
    assign packet_valid=request_valid && state==READY;
    assign request_ready=packet_valid && packet_ready;
    always @* begin
        value=0;axis=0;
        case(index)
            0,1:value=255;
            2:value=id;
            3:value=kind==1 ? 48 : 4;
            4:value=kind==1 ? 8'ha1 : 2;
            5:value=kind==1 ? saved_mask[7:0] : kind==0 ? 8'h38 : 8'h28;
            6:value=kind==1 ? {7'd0,saved_mask[8]} : kind==0 ? 15 : 6;
            7:value=saved_kp[7:0];8:value=saved_kp[15:8];
            9:value=saved_kd[7:0];10:value=saved_kd[15:8];
            11:value=saved_kv[7:0];12:value=saved_kv[15:8];
            13:value=saved_limit[7:0];14:value=saved_limit[15:8];
            default:begin
                axis=(index-15)>>2;
                case((index-15)&3)
                    0:value=saved_targets[axis*16 +: 8];
                    1:value=saved_targets[axis*16+8 +: 8];
                    2:value=saved_deltas[axis*16 +: 8];
                    3:value=saved_deltas[axis*16+8 +: 8];
                endcase
            end
        endcase
        if(index==packet_length-1) value=~sum;
    end
    always @(posedge clk) begin
        fault<=0;
        if(rst || !request_valid) begin state<=IDLE;index<=0;end
        else case(state)
            IDLE: if(!allowed) begin state<=REJECT;fault<=1;end
                else begin
                    kind<=request_kind;id<=request_id;saved_mask<=mask;
                    saved_kp<=kp;saved_kd<=kd;saved_kv<=kv;saved_limit<=duty_limit;
                    saved_targets<=targets;saved_deltas<=deltas;
                    packet<=0;packet_length<=request_kind==1 ? 52 : 8;sum<=0;index<=0;state<=BUILD;
                end
            BUILD: begin
                packet[index*8 +: 8]<=value;
                if(index>=2) sum<=sum+value;
                if(index==packet_length-1) state<=READY;else index<=index+1;
            end
            READY: if(packet_ready) state<=IDLE;
            default: ;
        endcase
    end
endmodule
