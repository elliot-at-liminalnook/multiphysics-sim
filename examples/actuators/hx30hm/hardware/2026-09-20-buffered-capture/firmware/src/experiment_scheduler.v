// Device-clock sequencer for the supervised FPGA experiment adapter.
// No UART ownership, ARM, heartbeat renewal, or motor arithmetic lives here.
// request_ready must mean the complete transaction can start on an idle bus.
// A control completion is a final-wire-byte receipt, NOT a servo execution ACK.
module experiment_scheduler #(
    parameter RUN_RELATIVE32=0,
    parameter MIN_PERIOD=2000000,       // 40 ms for groups larger than three
    parameter FAST_MIN_PERIOD=125000,  // 10 ms for at most three selected motors
    parameter MAX_PERIOD=7500000,       // 150 ms
    parameter MAX_DURATION=600000000,   // 12 s
    parameter MAX_FRAMES=256
)(
    input wire clk,rst,
    input wire start,cancel,
    input wire [8:0] configured_mask,
    input wire [8:0] configured_frames,
    input wire [31:0] configured_period,
    input wire plan_valid,
    input wire supervisor_latched,
    input wire adapter_fault,
    input wire [8:0] armed_mask,fresh_mask,
    input wire [71:0] sample_sequences,
    // Trajectory storage must validate a row before it becomes available.
    output reg [8:0] frame_index=0,
    input wire row_ready,
    // Atomic packet adapter: kind 0 = telemetry READ 0x38/15, 1 = controller A1, 2 = audit READ 0x28/6.
    output wire request_valid,
    output wire request_control,
    output wire [1:0] request_kind,
    output wire [7:0] request_id,
    input wire request_ready,
    input wire control_wire_done,
    input wire audit_reply_valid,audit_reply_ok,
    input wire [7:0] audit_reply_id,
    // Event sink must reserve space. Overflow aborts rather than hiding samples.
    input wire event_ready,
    output reg event_valid=0,
    output reg event_control=0,
    output reg [1:0] event_kind=0,
    output reg event_ok=1,
    output reg [7:0] event_id=0,
    output reg [8:0] event_frame=0,
    output reg [63:0] event_request_ticks=0,
    output reg [63:0] event_completion_ticks=0,
    output reg [7:0] event_sample_sequence=0,
    // Route to independent supervisor STOP. Never interpret this as motor stationarity.
    output wire stop_required,
    input wire stop_pair_wire_done,
    output wire active,
    output reg terminal=0,
    output reg [7:0] result=0,
    output reg [63:0] start_ticks=0,
    output reg [63:0] stop_ticks=0,
    output reg [63:0] stop_pair_ticks=0,
    output reg interrupted_request=0,
    output reg [1:0] interrupted_kind=0,
    output reg [7:0] interrupted_id=0,
    output reg [63:0] interrupted_request_ticks=0,
    output wire [63:0] ticks
);
    localparam IDLE=0,LOAD=1,POLL=2,WAIT_FEEDBACK=3,CONTROL=4,
        WAIT_CONTROL=5,WAIT_TICK=6,STOP=7,AUDIT=8,WAIT_AUDIT=9;
    // result: 0 completed, 1 invalid start, 2 supervisor, 3 deadline,
    // 4 cancelled, 5 event overflow, 6 start/config collision, 7 torque/PWM audit,
    // 8 transaction adapter or raw-feedback association failure.
    reg [3:0] state=IDLE;
    reg previous_start=0;
    reg [8:0] mask=0,frames=0;
    reg [31:0] period=0,phase=0;
    reg [3:0] axis=0;
    reg [7:0] before_sequence=0;
    reg [63:0] request_ticks=0;
    // The independently checked period bound is below 2^23.
    wire [31:0] compact_duration=configured_frames*configured_period[22:0];
    wire [63:0] configured_duration=MAX_PERIOD<8388608 ? {32'd0,compact_duration} : {55'd0,configured_frames}*configured_period;
    function [3:0] motor_count;
        input [8:0] bits;
        integer n;
        begin motor_count=0;for(n=0;n<9;n=n+1)motor_count=motor_count+bits[n];end
    endfunction
    wire valid_start=!event_valid && plan_valid && configured_mask!=0 && configured_frames>=1
        && configured_frames<=MAX_FRAMES
        && (configured_period>=MIN_PERIOD || (motor_count(configured_mask)<=3 && configured_period>=FAST_MIN_PERIOD))
        && configured_period<=MAX_PERIOD && configured_duration<=MAX_DURATION
        && !supervisor_latched && !adapter_fault && (armed_mask&configured_mask)==configured_mask
        && (fresh_mask&configured_mask)==configured_mask;
    assign active=(state!=IDLE && state!=STOP);
    assign stop_required=(state==STOP);
    // Withdraw pending requests on stop/deadline; no late acceptance/catch-up burst.
    wire permitted=active && !event_valid && plan_valid && !start && !cancel && !supervisor_latched && !adapter_fault
        && (armed_mask&mask)==mask && phase<period-1;
    assign request_valid=permitted && (((state==POLL || state==AUDIT) && mask[axis]) || (state==CONTROL && (fresh_mask&mask)==mask));
    assign request_control=(state==CONTROL);
    assign request_kind=request_control ? 1 : state==AUDIT ? 2 : 0;
    assign request_id=request_control ? 8'd254 : {4'd0,axis}+8'd4;
    task stop;
        input [7:0] why;
        begin
            state<=STOP;result<=why;stop_ticks<=ticks;
            interrupted_request<=(state==WAIT_FEEDBACK || state==WAIT_CONTROL || state==WAIT_AUDIT);
            interrupted_kind<=state==WAIT_CONTROL ? 1 : state==WAIT_AUDIT ? 2 : 0;
            interrupted_id<=state==WAIT_CONTROL ? 254 : {4'd0,axis}+8'd4;
            interrupted_request_ticks<=request_ticks;
        end
    endtask
    // Finite captures need <600 million clocks. The compact profile restarts
    // its 32-bit counter at idle and saturates if a stopped bus never drains.
    // Event timestamps remain zero-extended 64-bit device ticks on the wire.
    generate if(RUN_RELATIVE32) begin: relative_clock
        reg [31:0] counter=0;
        assign ticks={32'd0,counter};
        always @(posedge clk)begin
            if(rst || (state==IDLE && !start)) counter<=0;
            else if(counter!=32'hffffffff) counter<=counter+1;
        end
    end else begin: absolute_clock
        reg [63:0] counter=0;assign ticks=counter;
        always @(posedge clk)if(rst)counter<=0;else counter<=counter+1;
    end endgenerate
    always @(posedge clk) begin
        if(event_valid && event_ready) event_valid<=0;
        terminal<=0;
        previous_start<=start;
        if(rst) begin
            state<=IDLE;event_valid<=0;previous_start<=0;mask<=0;frames<=0;period<=0;phase<=0;axis<=0;
            frame_index<=0;result<=0;start_ticks<=0;stop_ticks<=0;stop_pair_ticks<=0;interrupted_request<=0;
        end else begin
            if(state==IDLE) begin
                if(start && !previous_start) begin
                    result<=0;frame_index<=0;axis<=0;phase<=0;start_ticks<=ticks;interrupted_request<=0;
                    if(valid_start) begin mask<=configured_mask;frames<=configured_frames;
                        period<=configured_period;state<=LOAD;end
                    else stop(1);
                end
            end else if(state==STOP) begin
                // Remain stopped through an attempted re-start. A new edge after terminal is needed.
                if(stop_pair_wire_done) begin state<=IDLE;terminal<=1;stop_pair_ticks<=ticks;end
            end else if(start || !plan_valid) stop(6);
            else if(cancel) stop(4);
            else if(supervisor_latched || (armed_mask&mask)!=mask) stop(2);
            else if(adapter_fault) stop(8);
            else if(event_valid && !event_ready) stop(5);
            else if(phase==period-1) begin
                if(state!=WAIT_TICK) stop(3);
                else if(frame_index+1>=frames) stop(0);
                else begin frame_index<=frame_index+1;axis<=0;phase<=0;state<=LOAD;end
            end else begin
                phase<=phase+1;
                case(state)
                    LOAD: if(row_ready) begin axis<=0;state<=POLL;end
                    POLL: if(!mask[axis]) begin
                        if(axis==8) state<=CONTROL;else axis<=axis+1;
                    end else if(request_valid && request_ready) begin
                        before_sequence<=sample_sequences[axis*8 +: 8];
                        request_ticks<=ticks;state<=WAIT_FEEDBACK;
                    end
                    WAIT_FEEDBACK: if(sample_sequences[axis*8 +: 8]!=before_sequence && fresh_mask[axis]) begin
                        if(!event_ready) stop(5);
                        else begin
                            event_valid<=1;event_control<=0;event_kind<=0;event_ok<=1;event_id<={4'd0,axis}+8'd4;
                            event_frame<=frame_index;event_request_ticks<=request_ticks;
                            event_completion_ticks<=ticks;event_sample_sequence<=sample_sequences[axis*8 +: 8];
                            if(axis==8) state<=CONTROL;else begin axis<=axis+1;state<=POLL;end
                        end
                    end
                    CONTROL: if((fresh_mask&mask)!=mask) stop(2); else if(request_valid && request_ready) begin request_ticks<=ticks;state<=WAIT_CONTROL;end
                    WAIT_CONTROL: if(control_wire_done) begin
                        if(!event_ready) stop(5);
                        else begin event_valid<=1;event_control<=1;event_kind<=1;event_ok<=1;event_id<=254;
                            event_frame<=frame_index;event_request_ticks<=request_ticks;
                            event_completion_ticks<=ticks;event_sample_sequence<=0;axis<=0;state<=AUDIT;end
                    end
                    AUDIT: if(!mask[axis]) begin
                        if(axis==8) state<=WAIT_TICK;else axis<=axis+1;
                    end else if(request_valid && request_ready) begin request_ticks<=ticks;state<=WAIT_AUDIT;end
                    WAIT_AUDIT: if(audit_reply_valid && audit_reply_id=={4'd0,axis}+8'd4) begin
                        if(!event_ready) stop(5);
                        else begin
                            event_valid<=1;event_control<=0;event_kind<=2;event_ok<=audit_reply_ok;
                            event_id<=audit_reply_id;event_frame<=frame_index;event_request_ticks<=request_ticks;
                            event_completion_ticks<=ticks;event_sample_sequence<=0;
                            if(!audit_reply_ok) stop(7);
                            else if(axis==8) state<=WAIT_TICK;else begin axis<=axis+1;state<=AUDIT;end
                        end
                    end
                    WAIT_TICK: ;
                    default: stop(6);
                endcase
            end
        end
    end
endmodule
