// Composition used by the real bridge: one transaction core and one recorder.
// START is externally validated against the sealed plan/current pose. The bridge
// retains UART ownership, the compiled controller and independent supervision.
module experiment_session #(parameter RUN_RELATIVE32=0)(
    input wire clk,rst,start,cancel,
    output wire start_ready,
    input wire [31:0] run_id,plan_crc,period,
    input wire [8:0] mask,
    input wire [15:0] frames,
    input wire [15:0] kp,kd,kv,duty_limit,
    input wire plan_valid,row_ready,
    input wire [143:0] targets,deltas,
    input wire supervisor_latched,
    input wire [8:0] armed_mask,fresh_mask,
    input wire [71:0] sample_sequences,
    input wire servo_valid,
    input wire [7:0] servo_data,
    output wire packet_valid,
    input wire packet_ready,
    output wire [511:0] packet,
    output wire [6:0] packet_length,
    input wire control_wire_done,
    input wire [143:0] transmitted_pwm,
    output wire log_valid,
    input wire log_ready,
    output wire [511:0] log_packet,
    output wire [6:0] log_length,
    output wire stop_required,
    input wire stop_pair_wire_done,
    output wire active,locked,fault,
    output wire [15:0] frame_index
);
    wire event_valid,event_ready,event_ok,terminal,bank_locked,log_busy;
    wire [1:0] event_kind,interrupted_kind;
    wire [7:0] event_id,event_sequence,event_error,result,interrupted_id;
    wire [15:0] event_frame;
    wire [63:0] request_ticks,completion_ticks,ticks,start_ticks,stop_ticks,stop_pair_ticks,interrupted_ticks;
    wire [143:0] event_data;
    wire [5:0] event_width;
    wire interrupted;
    assign locked=bank_locked || log_busy || start;
    experiment_transactions #(.RUN_RELATIVE32(RUN_RELATIVE32)) transactions(.clk(clk),.rst(rst),.start(start),.cancel(cancel || fault),
        .configured_mask(mask),.configured_frames(frames),.configured_period(period),
        .kp(kp),.kd(kd),.kv(kv),.duty_limit(duty_limit),.plan_valid(plan_valid),.row_ready(row_ready),
        .targets(targets),.deltas(deltas),.frame_index(frame_index),.bank_locked(bank_locked),
        .supervisor_latched(supervisor_latched),.armed_mask(armed_mask),.fresh_mask(fresh_mask),.sample_sequences(sample_sequences),
        .packet_valid(packet_valid),.packet_ready(packet_ready),.packet(packet),.packet_length(packet_length),
        .servo_valid(servo_valid),.servo_data(servo_data),.control_wire_done(control_wire_done),.transmitted_pwm(transmitted_pwm),
        .event_valid(event_valid),.event_ready(event_ready),.event_kind(event_kind),.event_ok(event_ok),
        .event_id(event_id),.event_sample_sequence(event_sequence),.event_frame(event_frame),
        .event_request_ticks(request_ticks),.event_completion_ticks(completion_ticks),.event_data(event_data),
        .event_width(event_width),.event_error(event_error),.stop_required(stop_required),.stop_pair_wire_done(stop_pair_wire_done),
        .active(active),.terminal(terminal),.result(result),.ticks(ticks),.start_ticks(start_ticks),
        .stop_ticks(stop_ticks),.stop_pair_ticks(stop_pair_ticks),.interrupted_request(interrupted),
        .interrupted_kind(interrupted_kind),.interrupted_id(interrupted_id),.interrupted_request_ticks(interrupted_ticks));
    experiment_event_stream recorder(.clk(clk),.rst(rst),.start(start),.start_ready(start_ready),
        .run_id(run_id),.plan_crc(plan_crc),.period(period),.mask(mask),.frames(frames),.start_ticks(ticks),
        .event_valid(event_valid),.event_ready(event_ready),.event_kind(event_kind),.event_ok(event_ok),
        .event_id(event_id),.event_sequence(event_sequence),.event_error(event_error),.event_frame(event_frame),
        .event_request_ticks(request_ticks),.event_completion_ticks(completion_ticks),.event_data(event_data),.event_width(event_width),
        .terminal(terminal),.result(result),.terminal_frame(frame_index),.stop_ticks(stop_ticks),.stop_pair_ticks(stop_pair_ticks),
        .interrupted_request(interrupted),.interrupted_kind(interrupted_kind),.interrupted_id(interrupted_id),.interrupted_request_ticks(interrupted_ticks),
        .packet_valid(log_valid),.packet_ready(log_ready),.packet(log_packet),.packet_length(log_length),.busy(log_busy),.fault(fault));
endmodule
