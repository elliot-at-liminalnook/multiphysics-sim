// Device-time scheduler + packet construction + raw-reply association.
// The surrounding bridge owns UART arbitration, independent supervision, the
// compiled A1 controller, START identity and durable event/terminal transport.
// This core never arms, renews a lease, or computes a second control law.
module experiment_transactions #(
    parameter MIN_PERIOD=2000000,MAX_PERIOD=7500000,
    parameter MAX_DURATION=600000000,MAX_FRAMES=256,RX_TIMEOUT=100000
)(
    input wire clk,rst,start,cancel,
    input wire [8:0] configured_mask,configured_frames,
    input wire [31:0] configured_period,
    input wire [15:0] kp,kd,kv,duty_limit,
    input wire plan_valid,row_ready,
    input wire [143:0] targets,deltas,
    output wire [8:0] frame_index,
    output wire bank_locked,
    input wire supervisor_latched,
    input wire [8:0] armed_mask,fresh_mask,
    input wire [71:0] sample_sequences,
    output wire packet_valid,
    input wire packet_ready,
    output wire [511:0] packet,
    output wire [6:0] packet_length,
    input wire servo_valid,
    input wire [7:0] servo_data,
    // Must describe the batch whose FINAL UART byte just finished.
    input wire control_wire_done,
    input wire [143:0] transmitted_pwm,
    output wire event_valid,
    input wire event_ready,
    output wire [1:0] event_kind,
    output wire event_ok,
    output wire [7:0] event_id,event_sample_sequence,
    output wire [8:0] event_frame,
    output wire [63:0] event_request_ticks,event_completion_ticks,
    output wire [143:0] event_data,
    output wire [5:0] event_width,
    output wire [7:0] event_error,
    output wire stop_required,
    input wire stop_pair_wire_done,
    output wire active,terminal,
    output wire [7:0] result,
    output wire [63:0] ticks,start_ticks,stop_ticks,stop_pair_ticks,
    output wire interrupted_request,
    output wire [1:0] interrupted_kind,
    output wire [7:0] interrupted_id,
    output wire [63:0] interrupted_request_ticks
);
    wire request_valid,request_ready,builder_valid,builder_fault;
    wire [1:0] request_kind;
    wire [7:0] request_id;
    wire accepted=request_valid && request_ready;
    reg last_start=0;
    reg [8:0] mask=0;
    reg [15:0] saved_kp=0,saved_kd=0,saved_kv=0,saved_limit=0;
    reg [143:0] pwm=0;
    reg control_pending=0,association_fault=0;
    wire reply_valid,reply_ok,reply_fault;
    wire [1:0] reply_kind;
    wire [7:0] reply_id,reply_error;
    wire [5:0] reply_width;
    wire [119:0] reply_data;
    reg [71:0] matched_sequences=0;
    reg [7:0] poll_id=4,poll_before=0;
    reg telemetry_wait=0;
    // A supervisor sequence change alone is insufficient: associate it with
    // the complete raw telemetry for this accepted read before advancing.
    wire [7:0] observed_sequence=sample_sequences[(poll_id-4)*8 +: 8];
    wire adapter_fault=builder_fault || association_fault
        || (reply_fault && !(reply_valid && reply_kind==2));
    assign bank_locked=active || stop_required || event_valid;
    // A logger can reserve/encode an event between bus transactions. Loss of
    // credit during a transaction still causes the scheduler to stop.
    assign packet_valid=builder_valid && event_ready;
    assign event_data=event_kind==1 ? pwm : {24'd0,reply_data};
    assign event_width=event_kind==1 ? 6'd18 : reply_width;
    assign event_error=event_kind==1 ? 8'd0 : reply_error;
    always @(posedge clk) begin
        last_start<=start;association_fault<=0;
        if(rst) begin
            last_start<=0;mask<=0;matched_sequences<=0;telemetry_wait<=0;
            control_pending<=0;pwm<=0;
        end else begin
            if(start && !last_start && !bank_locked) begin
                mask<=configured_mask;saved_kp<=kp;saved_kd<=kd;saved_kv<=kv;saved_limit<=duty_limit;
                matched_sequences<=sample_sequences;telemetry_wait<=0;control_pending<=0;pwm<=0;
            end
            if(accepted) begin
                if(request_kind==0) begin
                    poll_id<=request_id;poll_before<=sample_sequences[(request_id-4)*8 +: 8];
                    telemetry_wait<=0;
                end
                if(request_kind==1) control_pending<=1;
            end
            if(reply_valid && reply_kind==0 && reply_ok) telemetry_wait<=1;
            if(telemetry_wait && observed_sequence!=poll_before && fresh_mask[poll_id-4]) begin
                matched_sequences[(poll_id-4)*8 +: 8]<=observed_sequence;telemetry_wait<=0;
            end
            if(control_wire_done && active) begin
                if(!control_pending) association_fault<=1;
                else begin pwm<=transmitted_pwm;control_pending<=0;end
            end
            if(cancel || stop_required || !active) begin telemetry_wait<=0;control_pending<=0;end
        end
    end
    experiment_scheduler #(.MIN_PERIOD(MIN_PERIOD),.MAX_PERIOD(MAX_PERIOD),
        .MAX_DURATION(MAX_DURATION),.MAX_FRAMES(MAX_FRAMES)) scheduler(
        .clk(clk),.rst(rst),.start(start),.cancel(cancel),
        .configured_mask(configured_mask),.configured_frames(configured_frames),.configured_period(configured_period),
        .plan_valid(plan_valid),.supervisor_latched(supervisor_latched),.adapter_fault(adapter_fault),
        .armed_mask(armed_mask),.fresh_mask(fresh_mask),.sample_sequences(matched_sequences),
        .frame_index(frame_index),.row_ready(row_ready),.request_valid(request_valid),.request_control(),
        .request_kind(request_kind),.request_id(request_id),.request_ready(request_ready),
        .control_wire_done(control_wire_done && control_pending),
        .audit_reply_valid(reply_valid && reply_kind==2),.audit_reply_ok(reply_ok),.audit_reply_id(reply_id),
        .event_ready(event_ready),.event_valid(event_valid),.event_control(),.event_kind(event_kind),
        .event_ok(event_ok),.event_id(event_id),.event_frame(event_frame),
        .event_request_ticks(event_request_ticks),.event_completion_ticks(event_completion_ticks),
        .event_sample_sequence(event_sample_sequence),.stop_required(stop_required),
        .stop_pair_wire_done(stop_pair_wire_done),.active(active),.terminal(terminal),.result(result),
        .start_ticks(start_ticks),.stop_ticks(stop_ticks),.stop_pair_ticks(stop_pair_ticks),
        .interrupted_request(interrupted_request),.interrupted_kind(interrupted_kind),
        .interrupted_id(interrupted_id),.interrupted_request_ticks(interrupted_request_ticks),.ticks(ticks));
    experiment_packet builder(.clk(clk),.rst(rst),.request_valid(request_valid),
        .request_kind(request_kind),.request_id(request_id),.mask(mask),
        .kp(saved_kp),.kd(saved_kd),.kv(saved_kv),.duty_limit(saved_limit),.targets(targets),.deltas(deltas),
        .request_ready(request_ready),.packet_valid(builder_valid),.packet_ready(packet_ready && event_ready),
        .packet(packet),.packet_length(packet_length),.fault(builder_fault));
    experiment_reply #(.RX_TIMEOUT(RX_TIMEOUT)) replies(.clk(clk),.rst(rst),.cancel(cancel || stop_required),
        .request_accepted(accepted),.request_kind(request_kind),.request_id(request_id),
        .expected_pwm(pwm[(request_id-4)*16 +: 16]),.servo_valid(servo_valid),.servo_data(servo_data),
        .pending(),.reply_valid(reply_valid),.reply_ok(reply_ok),.reply_kind(reply_kind),.reply_id(reply_id),
        .reply_error(reply_error),.reply_width(reply_width),.reply_data(reply_data),.fault(reply_fault));
endmodule
