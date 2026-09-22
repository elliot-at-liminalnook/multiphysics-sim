// Transparent USB-UART <-> servo-bus bridge. A debugging instrument.
//
// The dock's FT2232 channel B is a UART on B3/C3 (verified against Sipeed's
// own UART/simple_uart example). This design relays packets between that
// port at 115200 and the servo's single-wire bus at 1 Mbaud, so a script on
// the Mac can talk to the servo directly -- read registers back, try a
// command, see the actual error byte.
//
// Without this, every question about the servo has to be answered through
// one LED. With it, you get the bytes.
//
// Two things the bridge must do beyond copying bytes:
//
//  1. STORE AND FORWARD, not byte-by-byte. At 115200 the host's bytes are
//     ~87 us apart; blindly re-emitting them at 1 Mbaud would stretch a
//     packet across 1.1 ms of bus time with 77 us gaps mid-frame, and the
//     servo's receiver would time out partway through. So the bridge parses
//     the length field, collects the whole packet, and then sends it as one
//     contiguous burst.
//
//  2. GATE THE ECHO, exactly as hx_bus does -- the servo bus is one wire and
//     we hear ourselves on it.
//
// The reverse path needs a FIFO for the opposite reason: replies arrive at
// 1 Mbaud (10 us/byte) and leave at 115200 (87 us/byte).

module top #(
    parameter HOST_CLKS  = 434,      // 50 MHz / 115200 (0.007% error)
    parameter SERVO_CLKS = 50,       // 50 MHz / 1 Mbaud, exact
    parameter GUARD      = 100,
    parameter RELEASE_TX = 0,      // diagnostic variant: high-Z while receiving
    parameter SAFETY_ENABLE = 0,
    parameter BUFFER_HOST = 0,
    parameter TRAJECTORY_ENABLE = 0,
    parameter EXPERIMENT_ENABLE = 0,
    parameter FIXED_GAINS = 0,
    parameter LOG_STRIDE = 1,
    parameter FIXED_KP = 4096, FIXED_KD = 0, FIXED_KV = 4096,
    parameter SAFETY_FEEDBACK_TIMEOUT = 10000000,
    parameter SAFETY_COMMAND_TIMEOUT = 15000000,
    parameter SAFETY_STOP_REPEAT = 2500000,
    parameter SAFETY_STOP_GAP = 100000, // 2 ms for servo command processing
    parameter SAFETY_REPLY_WAIT = 100000 // bounded 2 ms initial reply turnaround
)(
    input  wire clk,
    input  wire host_rx,             // B3  <- USB-UART
    output wire host_tx,             // C3  -> USB-UART
    output wire servo_tx,            // G5  -> 1k -> SIG
    input  wire servo_rx,            // F5  <- 1k <- SIG
    input  wire key2,                // dock S2, active-high emergency stop
    output wire led_done,
    output wire led_ready
);

    reg [3:0] por = 4'd0;
    wire      rst = (por != 4'hF);
    always @(posedge clk)
        if (por != 4'hF) por <= por + 1'b1;

    // ---------------------------------------------------------------- host in
    wire [7:0] h_data;
    wire       h_valid;
    wire [7:0] raw_h_data;
    wire raw_h_valid;
    wire [511:0] queued_packet;
    wire [6:0] queued_length;
    wire queued_valid,queued_ready,urgent_host_stop,host_queue_fault;
    assign h_data=raw_h_data;
    assign h_valid=BUFFER_HOST ? 1'b0 : raw_h_valid;

    uart_rx #(.CLKS_PER_BIT(HOST_CLKS)) u_hrx (
        .clk(clk), .rst(rst), .rx(host_rx), .enable(1'b1),
        .data(raw_h_data), .valid(raw_h_valid));

    // Collect a whole packet: FF FF ID LEN, then LEN more bytes.
    // Total length is therefore LEN + 4.
    localparam C_H1 = 3'd0, C_H2 = 3'd1, C_BODY = 3'd2, C_SEND = 3'd3,
               C_CHECK=3'd4, C_LOCAL=3'd5, C_RECEIPT=3'd6;

    reg [3:0] cstate = C_H1;
    reg [7:0] pbuf [0:63];
    reg [6:0] pcnt = 0;              // bytes collected
    reg [6:0] ptot = 0;              // total expected
    reg [6:0] pidx = 0;              // transmit index
    reg [7:0] packet_checksum=0;

    reg [7:0] s_tx_data;
    reg       s_tx_start;
    wire      s_tx_busy;
    wire      s_tx_pin;
    wire [7:0] s_data;
    wire s_valid;
    wire [511:0] safety_packet;
    genvar pi;
    generate for(pi=0;pi<64;pi=pi+1) begin: packet_view
        assign safety_packet[pi*8 +: 8]=pbuf[pi];
    end endgenerate
    wire safety_allow, safety_local, safety_latched, safety_stop;
    wire [7:0] safety_reason, safety_fault_id;
    wire [103:0] safety_status;
    wire bridge_fault;
    wire [143:0] control_positions;
    wire [71:0] control_sequences;
    reg control_fault=0;
    localparam C_CTL_AXIS=8, C_CTL_SUM=9, C_CTL_FAULT=10, C_CTL_WRITE=13;
    reg [15:0] pending_wire_duty=0;
    reg [8:0] control_mask=0, previous_mask=0;
    reg [15:0] targets[0:8], previous_positions[0:8], anchors[0:8];
    reg [7:0] used_sequences[0:8];
    reg signed [15:0] deltas[0:8];
    reg [15:0] kp=0,kd=0,kv=0,duty_limit=0;
    reg [3:0] axis=0;
    reg [4:0] control_wait=0;
    reg [6:0] build_index=0;
    reg [7:0] build_sum=0;
    wire [15:0] axis_position=control_positions[16*axis +: 16];
    wire [15:0] axis_previous=previous_mask[axis] ? previous_positions[axis] : axis_position;
    wire [15:0] axis_anchor=previous_mask[axis] ? anchors[axis] : axis_position;
    wire signed [31:0] axis_error=$signed({1'b0,targets[axis]})-$signed({1'b0,axis_position});
    wire signed [31:0] axis_travel=$signed({1'b0,axis_position})-$signed({1'b0,axis_anchor});
    wire signed [15:0] axis_duty;
    wire [15:0] wire_duty=axis_duty<0 ? (-axis_duty | 16'd1024) : axis_duty;
    // These widths encode existing checked bounds, not different arithmetic:
    // encoder 0..4095, delta -32..32, gains 0..4096, limit 0..1000.
    // The full-width values below still reject invalid input before any send.
    fixed_pd_pipeline law(.clk(clk),.target($signed({20'd0,targets[axis][11:0]})),.position($signed({20'd0,axis_position[11:0]})),
        .previous($signed({20'd0,axis_previous[11:0]})),.delta({{25{deltas[axis][6]}},deltas[axis][6:0]}),
        .kp(FIXED_GAINS ? FIXED_KP : $signed({19'd0,kp[12:0]})),.kd(FIXED_GAINS ? FIXED_KD : $signed({19'd0,kd[12:0]})),.kv(FIXED_GAINS ? FIXED_KV : $signed({19'd0,kv[12:0]})),
        .limit($signed({22'd0,duty_limit[9:0]})),.duty(axis_duty));
    wire [15:0] incoming_mask={pbuf[6],pbuf[5]};
    generate if(BUFFER_HOST) begin: buffered_host
        host_packet_queue host_queue(.clk(clk),.rst(rst),.data(raw_h_data),.data_valid(raw_h_valid),
            .packet(queued_packet),.packet_length(queued_length),.packet_valid(queued_valid),
            .packet_ready(queued_ready),.urgent_stop(urgent_host_stop),.fault(host_queue_fault));
    end else begin: unbuffered_host
        assign queued_packet=0;assign queued_length=0;assign queued_valid=0;
        assign urgent_host_stop=0;assign host_queue_fault=0;
    end endgenerate
    wire supervisor_stop_now;
    wire [511:0] supervisor_packet=supervisor_stop_now ? {456'd0,56'h5e00a003feffff} : safety_packet;
    wire [6:0] supervisor_length=supervisor_stop_now ? 7'd7 : ptot;
    wire [7:0] supervisor_checksum=supervisor_stop_now ? 8'd255 : packet_checksum;
    integer ci;
    reg stop_active=0;
    reg [4:0] stop_index=0;
    reg stop_accepted=0;
    localparam REPLY_MAX = SAFETY_REPLY_WAIT > SERVO_CLKS*20 ? SAFETY_REPLY_WAIT : SERVO_CLKS*20;
    reg [$clog2(SAFETY_STOP_GAP+1)-1:0] stop_gap=0;
    reg [$clog2(REPLY_MAX+1)-1:0] reply_wait=0;
    reg [15:0] rx_quiet=0;
    always @(posedge clk) begin
        if(rst || s_valid || !servo_rx) rx_quiet<=0;
        else if(rx_quiet<SERVO_CLKS*20) rx_quiet<=rx_quiet+1;
    end
    wire begin_stop=SAFETY_ENABLE && safety_stop && !stop_active
        && cstate!=C_SEND && !s_tx_busy && stop_gap==0 && reply_wait==0 && rx_quiet>=SERVO_CLKS*20;
    wire check_safety=(cstate==C_CHECK) && !stop_active && !begin_stop && !s_tx_busy && stop_gap==0;
    localparam TRAJECTORY_ENABLED=TRAJECTORY_ENABLE && BUFFER_HOST && SAFETY_ENABLE;
    localparam EXPERIMENT_ENABLED=EXPERIMENT_ENABLE && TRAJECTORY_ENABLED;
    localparam C_TRAJ_WAIT=11,C_TRAJ_REPLY=12;
    wire trajectory_handled,trajectory_valid,trajectory_busy,trajectory_fault,trajectory_row_ready;
    wire [8:0] trajectory_mask,trajectory_frames,trajectory_written;
    wire [31:0] trajectory_period,trajectory_crc;
    wire [15:0] trajectory_kp,trajectory_kd,trajectory_kv,trajectory_limit;
    wire [143:0] trajectory_homes,trajectory_targets,trajectory_deltas;
    reg [7:0] trajectory_op=0;
    reg trajectory_failed=0;
    reg experiment_start=0,experiment_owner=0,experiment_control_done=0;
    reg experiment_stop_done=0,stop_wire_pending=0,previous_experiment_stop=0;
    reg [31:0] experiment_run_id=0;
    reg [143:0] experiment_pwm=0;
    wire experiment_start_ready,experiment_active,experiment_locked,experiment_fault,experiment_stop;
    wire experiment_packet_valid,experiment_packet_ready,experiment_log_valid,experiment_log_ready;
    wire [511:0] experiment_packet,legacy_log_packet;
    wire [7:0] experiment_log_byte;
    wire legacy_log_valid,legacy_log_ready,core_locked,core_fault,batch_busy,batch_fault,batch_capture_active,event_capture_pulse;
    wire [6:0] legacy_log_length;
    assign experiment_locked=core_locked || batch_busy;
    assign experiment_fault=core_fault || batch_fault;
    wire [6:0] experiment_packet_length;
    wire [8:0] experiment_log_length;
    wire [8:0] experiment_frame;
    wire control_valid=ptot==52 && packet_checksum==255 && incoming_mask!=0 && incoming_mask<=511
        && !safety_latched && ((incoming_mask & safety_status[47:32])==incoming_mask)
        && ((incoming_mask & safety_status[63:48])==incoming_mask)
        && (!FIXED_GAINS || ({pbuf[8],pbuf[7]}==FIXED_KP && {pbuf[10],pbuf[9]}==FIXED_KD && {pbuf[12],pbuf[11]}==FIXED_KV))
        && {pbuf[8],pbuf[7]}<=4096 && {pbuf[10],pbuf[9]}<=4096
        && {pbuf[12],pbuf[11]}<=4096 && {pbuf[14],pbuf[13]}<=1000
        && ({pbuf[14],pbuf[13]}<=100 || (EXPERIMENT_ENABLED && experiment_owner));
    wire start_packet=EXPERIMENT_ENABLED && !experiment_owner && pbuf[2]==254 && pbuf[4]==8'ha2 && pbuf[5]==3;
    wire permitted_run_host=(pbuf[2]==254 && pbuf[4]==8'ha0
        && ((ptot==7 && (pbuf[5]==0 || pbuf[5]==2))
            || (ptot==8 && (pbuf[5]==3 || pbuf[5]==4)) || (ptot==9 && pbuf[5]==5)))
        || (pbuf[2]==254 && pbuf[4]==8'ha2 && ptot==7 && pbuf[5]==4) || start_packet;
    wire blocked_run_host=experiment_locked && !experiment_owner && !permitted_run_host;
    reg start_pose_matches;
    integer ei;
    always @* begin
        start_pose_matches=1;
        for(ei=0;ei<9;ei=ei+1) if(trajectory_mask[ei] && control_positions[ei*16 +: 16]!=trajectory_homes[ei*16 +: 16]) start_pose_matches=0;
    end
    wire experiment_start_valid=ptot==15 && packet_checksum==255 && !experiment_locked && experiment_start_ready
        && (!FIXED_GAINS || (trajectory_kp==FIXED_KP && trajectory_kd==FIXED_KD && trajectory_kv==FIXED_KV))
        && trajectory_valid && !trajectory_busy && trajectory_row_ready && start_pose_matches
        && {pbuf[9],pbuf[8],pbuf[7],pbuf[6]}==trajectory_crc && !safety_latched
        && (safety_status[40:32]&trajectory_mask)==trajectory_mask
        && (safety_status[56:48]&trajectory_mask)==trajectory_mask;
    assign supervisor_stop_now=urgent_host_stop || (experiment_stop && !previous_experiment_stop && !safety_latched);
    generate if(EXPERIMENT_ENABLED) begin: experiment
        experiment_session #(.RUN_RELATIVE32(1)) session(.clk(clk),.rst(rst),.start(experiment_start),.cancel(urgent_host_stop),
            .start_ready(experiment_start_ready),.run_id(experiment_run_id),.plan_crc(trajectory_crc),
            .period(trajectory_period),.mask(trajectory_mask),.frames(trajectory_frames),
            .kp(trajectory_kp),.kd(trajectory_kd),.kv(trajectory_kv),.duty_limit(trajectory_limit),
            .plan_valid(trajectory_valid),.row_ready(trajectory_row_ready),.targets(trajectory_targets),.deltas(trajectory_deltas),
            .supervisor_latched(safety_latched),.armed_mask(safety_status[40:32]),.fresh_mask(safety_status[56:48]),.sample_sequences(control_sequences),
            .servo_valid(s_valid),.servo_data(s_data),.packet_valid(experiment_packet_valid),.packet_ready(experiment_packet_ready),
            .packet(experiment_packet),.packet_length(experiment_packet_length),.control_wire_done(experiment_control_done),.transmitted_pwm(experiment_pwm),
            .log_valid(legacy_log_valid),.log_ready(legacy_log_ready),.log_packet(legacy_log_packet),.log_length(legacy_log_length),
            .event_capture_pulse(event_capture_pulse),
            .stop_required(experiment_stop),.stop_pair_wire_done(experiment_stop_done),.active(experiment_active),.locked(core_locked),
            .fault(core_fault),.frame_index(experiment_frame));
        experiment_frame_log #(.LOG_STRIDE(LOG_STRIDE)) batched(.clk(clk),.rst(rst),.raw_valid(s_valid),.raw_data(s_data),
            .event_pulse(event_capture_pulse),.input_valid(legacy_log_valid),.input_ready(legacy_log_ready),
            .input_packet(legacy_log_packet),.input_length(legacy_log_length),
            .output_valid(experiment_log_valid),.output_ready(experiment_log_ready),
            .read_index(experiment_log_index[7:0]),.read_data(experiment_log_byte),.output_length(experiment_log_length),.busy(batch_busy),.capture_active(batch_capture_active),.fault(batch_fault));
    end else begin
        assign experiment_start_ready=0;assign experiment_active=0;assign core_locked=0;assign core_fault=0;assign experiment_stop=0;
        assign batch_capture_active=0;assign batch_busy=0;assign batch_fault=0;assign legacy_log_valid=0;assign legacy_log_length=0;assign legacy_log_packet=0;assign legacy_log_ready=0;assign event_capture_pulse=0;
        assign experiment_packet_valid=0;assign experiment_packet=0;assign experiment_packet_length=0;
        assign experiment_log_valid=0;assign experiment_log_byte=0;assign experiment_log_length=0;assign experiment_frame=0;
    end endgenerate
    generate if(TRAJECTORY_ENABLED) begin: trajectory
        trajectory_upload upload(.clk(clk),.rst(rst),.active(experiment_locked),.check(check_safety && !start_packet && !blocked_run_host),
            .packet_ok(packet_checksum==255),.transport_fault(host_queue_fault),
            .packet(safety_packet),.packet_length(ptot),.read_index(experiment_locked ? experiment_frame : 9'd0),
            .handled(trajectory_handled),.valid(trajectory_valid),.busy(trajectory_busy),
            .fault(trajectory_fault),.row_ready(trajectory_row_ready),.mask(trajectory_mask),
            .frames(trajectory_frames),.written(trajectory_written),.period(trajectory_period),
            .crc32(trajectory_crc),.kp(trajectory_kp),.kd(trajectory_kd),.kv(trajectory_kv),
            .duty_limit(trajectory_limit),.homes(trajectory_homes),.row_targets(trajectory_targets),
            .row_deltas(trajectory_deltas));
    end else begin: no_trajectory
        assign trajectory_handled=0;assign trajectory_valid=0;assign trajectory_busy=0;
        assign trajectory_fault=0;assign trajectory_row_ready=0;assign trajectory_mask=0;
        assign trajectory_frames=0;assign trajectory_written=0;assign trajectory_period=0;
        assign trajectory_crc=0;assign trajectory_kp=0;assign trajectory_kd=0;assign trajectory_kv=0;
        assign trajectory_limit=0;assign trajectory_homes=0;assign trajectory_targets=0;assign trajectory_deltas=0;
    end endgenerate
    // Capability bit 1 adds device-timed execution in the explicit experiment profile.
    wire [7:0] trajectory_capabilities=EXPERIMENT_ENABLED ? ((FIXED_GAINS ? 8'd39 : 8'd35) | (LOG_STRIDE==2 ? 8'd64 : 8'd0)) : 8'd1;
    wire [199:0] trajectory_status={trajectory_capabilities,32'd50000000,trajectory_crc,trajectory_period,
        7'd0,trajectory_written,7'd0,trajectory_frames,7'd0,trajectory_mask,
        7'd0,trajectory_busy,7'd0,trajectory_valid,7'd0,trajectory_failed,trajectory_op,8'ha2,8'd1};
    generate if(SAFETY_ENABLE) begin: safety
        wire emergency;
        button #(.DEBOUNCE_CYC(50000)) stop_button (
            .clk(clk),.rst(rst),.raw(key2),.state(emergency),.pressed(),.released());
        hx_safety #(.FEEDBACK_TIMEOUT(SAFETY_FEEDBACK_TIMEOUT),
            .COMMAND_TIMEOUT(SAFETY_COMMAND_TIMEOUT), .STOP_REPEAT(SAFETY_STOP_REPEAT)) supervisor (
            .clk(clk),.rst(rst),.check_host((check_safety && !blocked_run_host && !start_packet) || supervisor_stop_now),.host_packet(supervisor_packet),.host_length(supervisor_length),
            .host_checksum(supervisor_checksum),.emergency_stop(emergency),.bridge_fault(bridge_fault),
            .allow_forward(safety_allow),.local_command(safety_local),.local_status(safety_status),.positions(control_positions),.sample_sequences(control_sequences),
            .servo_valid(s_valid),.servo_data(s_data),.latched(safety_latched),
            .reason(safety_reason),.fault_id(safety_fault_id),
            .stop_request(safety_stop),.stop_accepted(stop_accepted));
    end else begin: no_safety
        assign safety_allow=1'b1;
        assign safety_local=1'b0;
        assign safety_status=104'd0;
        assign safety_latched=1'b0;
        assign safety_reason=8'd0;
        assign safety_fault_id=8'd0;
        assign safety_stop=1'b0;
    end endgenerate
    // Two broadcast packets: PWM=0, then torque enable=0. Repeat while
    // latched, even if the host has disconnected. No ACK is expected.
    function [7:0] stop_byte;
        input [4:0] index;
        begin case(index)
            0,1,9,10: stop_byte=8'hff;
            2,11: stop_byte=8'hfe;
            3: stop_byte=5;
            4,13: stop_byte=3;
            5: stop_byte=8'h2c;
            8: stop_byte=8'hcd;
            12: stop_byte=4;
            14: stop_byte=8'h28;
            16: stop_byte=8'hd2;
            default: stop_byte=0;
        endcase end
    endfunction

    uart_tx #(.CLKS_PER_BIT(SERVO_CLKS)) u_stx (
        .clk(clk), .rst(rst), .start(s_tx_start), .data(s_tx_data),
        .tx(s_tx_pin), .busy(s_tx_busy));

    // Keep driving across the entire packet, including inter-byte gaps and
    // the final stop bit. The receiver's echo gate alone does NOT release TX.
    // Default retains the original resistor-combiner behavior for comparison.
    wire servo_tx_enable = !(RELEASE_TX && cstate != C_SEND && !stop_active && !s_tx_busy);
    assign servo_tx = servo_tx_enable ? s_tx_pin : 1'bz;

    reg [$clog2(GUARD+1)-1:0] guard = 0;
    wire listening = (cstate != C_SEND) && !stop_active && !s_tx_busy && (guard == 0);

    always @(posedge clk) begin
        s_tx_start <= 1'b0;
        control_fault<=0;
        experiment_start<=0;experiment_control_done<=0;experiment_stop_done<=0;
        previous_experiment_stop<=experiment_stop;
        if(stop_wire_pending && !s_tx_busy) begin
            experiment_stop_done<=1;stop_wire_pending<=0;
        end
        if(experiment_owner && cstate==C_H1 && !s_tx_busy && reply_wait==0 && rx_quiet>=SERVO_CLKS*20)
            experiment_owner<=0;
        if(trajectory_fault) trajectory_failed<=1;
        if(safety_latched) previous_mask<=0;
        stop_accepted <= 1'b0;

        if (rst) begin
            cstate <= C_H1;
            pcnt   <= 0;
            guard  <= 0;
            stop_active <= 0;
            stop_index <= 0;
            stop_gap <= 0;
            reply_wait <= 0;
            trajectory_failed<=0;trajectory_op<=0;
            experiment_owner<=0;experiment_start<=0;experiment_run_id<=0;
            experiment_control_done<=0;experiment_stop_done<=0;stop_wire_pending<=0;
            previous_experiment_stop<=0;experiment_pwm<=0;
        end else begin
            if(stop_gap!=0) stop_gap<=stop_gap-1;
            if(reply_wait!=0) reply_wait<=reply_wait-1;
            // Do not mistake the turnaround before the first reply byte for
            // an idle bus. Thereafter allow 20 quiet bit times after each byte.
            if(s_valid) reply_wait<=SERVO_CLKS*20;
            if (s_tx_busy)      guard <= GUARD;
            else if (guard != 0) guard <= guard - 1'b1;
            if(begin_stop) begin
                stop_active<=1; stop_index<=0; stop_accepted<=1;
            end
            if(stop_active && !s_tx_busy && stop_gap==0) begin
                s_tx_data<=stop_byte(stop_index); s_tx_start<=1;
                // Broadcasts have no reply to provide processing turnaround.
                // Space zero/off commands and the following host transaction.
                if(stop_index==8 || stop_index==16) stop_gap<=SAFETY_STOP_GAP;
                if(stop_index==16) begin stop_active<=0;stop_wire_pending<=experiment_stop;end
                else stop_index<=stop_index+1;
            end

            case (cstate)
                C_H1: if(queued_valid && queued_ready) begin
                    for(ci=0;ci<64;ci=ci+1) pbuf[ci]<=queued_packet[ci*8 +: 8];
                    ptot<=queued_length;pcnt<=queued_length;packet_checksum<=255;pidx<=0;
                    cstate<=SAFETY_ENABLE ? C_CHECK : C_SEND;experiment_owner<=0;
                end else if(experiment_packet_valid && experiment_packet_ready) begin
                    for(ci=0;ci<64;ci=ci+1) pbuf[ci]<=experiment_packet[ci*8 +: 8];
                    ptot<=experiment_packet_length;pcnt<=experiment_packet_length;packet_checksum<=255;pidx<=0;
                    experiment_owner<=1;cstate<=C_CHECK;
                end else if (h_valid && h_data == 8'hFF) begin
                    pbuf[0] <= 8'hFF; pcnt <= 7'd1; cstate <= C_H2;
                end

                C_H2: if (h_valid) begin
                    if (h_data == 8'hFF) begin
                        pbuf[1] <= 8'hFF; pcnt <= 7'd2; cstate <= C_BODY;
                        packet_checksum<=0;
                    end else
                        cstate <= C_H1;
                end

                C_BODY: if (h_valid) begin
                    pbuf[pcnt] <= h_data;
                    packet_checksum<=packet_checksum+h_data;
                    // The length byte is the 4th, and fixes the total.
                    if (pcnt == 7'd3) begin
                        ptot <= h_data + 7'd4;
                        if(SAFETY_ENABLE && (h_data<2 || h_data>60)) cstate<=C_H1;
                    end
                    if (pcnt >= 7'd3 && (pcnt + 7'd1) >= ptot && pcnt != 7'd3) begin
                        pidx   <= 0;
                        cstate <= SAFETY_ENABLE ? C_CHECK : C_SEND;
                    end else if (pcnt == 7'd63)
                        cstate <= C_H1;              // overlong: resynchronise
                    else
                        pcnt <= pcnt + 7'd1;
                end

                // Blast the collected packet back to back at 1 Mbaud.
                C_CHECK: if(check_safety) begin
                    if(blocked_run_host) begin
                        // Preserve the original run/bank; reject host motor traffic.
                        control_fault<=1;cstate<=C_CTL_FAULT;
                    end else if(start_packet) begin
                        trajectory_op<=3;trajectory_failed<=!experiment_start_valid;
                        if(experiment_start_valid) begin
                            experiment_run_id<={pbuf[13],pbuf[12],pbuf[11],pbuf[10]};
                            experiment_start<=1;previous_mask<=0;experiment_pwm<=0;
                            for(ci=0;ci<9;ci=ci+1) begin
                                previous_positions[ci]<=trajectory_homes[ci*16 +: 16];
                                anchors[ci]<=trajectory_homes[ci*16 +: 16];
                            end
                        end
                        cstate<=C_TRAJ_WAIT;
                    end else if(trajectory_handled) begin
                        trajectory_op<=pbuf[5];
                        if(pbuf[5]!=4) trajectory_failed<=0;
                        cstate<=C_TRAJ_WAIT;
                    end else if(pbuf[2]==254 && pbuf[4]==8'ha1) begin
                        if(!control_valid) begin control_fault<=1; cstate<=C_CTL_FAULT; end
                        else begin
                            control_mask<=incoming_mask[8:0];
                            if(experiment_owner) experiment_pwm<=0;
                            kp<={pbuf[8],pbuf[7]}; kd<={pbuf[10],pbuf[9]};
                            kv<={pbuf[12],pbuf[11]}; duty_limit<={pbuf[14],pbuf[13]};
                            for(ci=0;ci<9;ci=ci+1) begin
                                targets[ci]<={pbuf[16+ci*4],pbuf[15+ci*4]};
                                deltas[ci]<={pbuf[18+ci*4],pbuf[17+ci*4]};
                            end
                            pbuf[4]<=8'h83; pbuf[5]<=8'h2c; pbuf[6]<=2;
                            axis<=0; control_wait<=0; build_index<=7; cstate<=C_CTL_AXIS;
                        end
                    end else if(safety_local) cstate<=C_LOCAL;
                    else if(safety_allow) begin pidx<=0; cstate<=C_SEND; end
                    else begin pcnt<=0; cstate<=C_H1; end
                end
                C_CTL_AXIS: begin
                    if(safety_latched) begin control_fault<=1; cstate<=C_CTL_FAULT; end
                    else if(control_wait<20) control_wait<=control_wait+1;
                    else if(control_mask[axis] && (axis_position>4095 || targets[axis]>4095
                        || axis_error>100 || axis_error< -100 || axis_travel>180 || axis_travel< -180
                        || deltas[axis]>32 || deltas[axis]< -32
                        || (previous_mask[axis] && used_sequences[axis]==control_sequences[8*axis +: 8]))) begin
                        control_fault<=1; cstate<=C_CTL_FAULT;
                    end else begin
                        if(control_mask[axis]) begin
                            pending_wire_duty<=wire_duty;
                            if(experiment_owner) experiment_pwm[axis*16 +: 16]<=wire_duty;
                            previous_positions[axis]<=axis_position; anchors[axis]<=axis_anchor;
                            used_sequences[axis]<=control_sequences[8*axis +: 8];
                            control_wait<=0;cstate<=C_CTL_WRITE;
                        end else if(axis==8) begin
                            previous_mask<=control_mask;ptot<=build_index+1;
                            pbuf[3]<=build_index-3;pidx<=2;build_sum<=0;cstate<=C_CTL_SUM;
                        end else begin axis<=axis+1;control_wait<=0;end
                    end
                end
                C_CTL_WRITE: begin
                    if(safety_latched) begin control_fault<=1;cstate<=C_CTL_FAULT;end
                    else begin
                        // One addressed byte write per clock avoids three wide
                        // parallel write ports. The packet is still atomic on wire.
                        pbuf[build_index]<=control_wait==0 ? axis+4 : control_wait==1
                            ? pending_wire_duty[7:0] : pending_wire_duty[15:8];
                        build_index<=build_index+1;
                        if(control_wait==2) begin
                            if(axis==8) begin
                                previous_mask<=control_mask;ptot<=build_index+2;
                                pbuf[3]<=build_index-2;pidx<=2;build_sum<=0;cstate<=C_CTL_SUM;
                            end else begin axis<=axis+1;control_wait<=0;cstate<=C_CTL_AXIS;end
                        end else control_wait<=control_wait+1;
                    end
                end
                C_CTL_SUM: begin
                    if(pidx==ptot-1) begin
                        pbuf[pidx]<=~build_sum; packet_checksum<=255; cstate<=C_CHECK;
                    end else begin build_sum<=build_sum+pbuf[pidx]; pidx<=pidx+1; end
                end
                C_CTL_FAULT: begin experiment_owner<=0;cstate<=C_LOCAL;end
                C_TRAJ_WAIT: if(!trajectory_busy) cstate<=C_TRAJ_REPLY;
                C_TRAJ_REPLY: begin pcnt<=0;cstate<=C_H1;end
                C_LOCAL: begin pcnt<=0; cstate<=C_H1; end
                C_RECEIPT: if(!s_tx_busy && !stop_active) begin
                    if(experiment_owner) begin experiment_control_done<=1;experiment_owner<=0;cstate<=C_H1;end
                    else cstate<=C_LOCAL;
                end
                C_SEND: if(pidx==0 && (urgent_host_stop || host_queue_fault)) begin
                    pcnt<=0;cstate<=C_H1;
                end else if (!stop_active && !s_tx_busy) begin
                    s_tx_data  <= pbuf[pidx];
                    s_tx_start <= 1'b1;
                    if (pidx + 7'd1 == ptot) begin
                        if(SAFETY_ENABLE && pbuf[2]!=254) reply_wait<=SAFETY_REPLY_WAIT;
                        pcnt   <= 0;
                        cstate <= (SAFETY_ENABLE && pbuf[4]==8'h83) ? C_RECEIPT : C_H1;
                    end else
                        pidx <= pidx + 7'd1;
                end

                default: cstate <= C_H1;
            endcase
        end
    end

    // --------------------------------------------------------------- servo in
    uart_rx #(.CLKS_PER_BIT(SERVO_CLKS)) u_srx (
        .clk(clk), .rst(rst), .rx(servo_rx), .enable(listening),
        .data(s_data), .valid(s_valid));

    // 256-byte circular FIFO absorbs the 1 Mbaud -> 115200 rate step.
    reg [7:0] fifo [0:255];
    reg [7:0] wptr = 0, rptr = 0;
    wire fifo_empty = (wptr == rptr);
    wire fifo_full=(wptr+8'd1)==rptr;
    wire [7:0] fifo_free=rptr-wptr-8'd1;
    reg [7:0] local_buf[0:30];
    reg [5:0] local_length=19;
    reg local_pending=0;
    reg experiment_log_sending=0;
    reg [8:0] experiment_log_index=0;
    reg [1:0] experiment_log_read_wait=0;
    wire bus_idle=cstate==C_H1 && !stop_active && !begin_stop && !s_tx_busy
        && stop_gap==0 && reply_wait==0 && rx_quiet>=SERVO_CLKS*20 && !local_pending
        && !experiment_owner && !experiment_log_sending;
    assign queued_ready=BUFFER_HOST && bus_idle && (!TRAJECTORY_ENABLED || fifo_free>=(EXPERIMENT_ENABLED ? 128 : 31))
        && !experiment_log_valid;
    wire control_bus_idle=cstate==C_H1 && !stop_active && !begin_stop && !s_tx_busy
        && stop_gap==0 && reply_wait==0 && rx_quiet>=SERVO_CLKS*20 && !local_pending && !experiment_owner;
    assign experiment_packet_ready=EXPERIMENT_ENABLED && control_bus_idle && !queued_valid && !safety_latched;
    wire begin_experiment_log=EXPERIMENT_ENABLED && experiment_log_valid && bus_idle
        && fifo_free>=experiment_log_length && experiment_log_length>=6 && experiment_log_length<=255;
    assign experiment_log_ready=experiment_log_sending && !s_valid && !fifo_full
        && experiment_log_read_wait==2 && experiment_log_index+1==experiment_log_length;
    assign bridge_fault=host_queue_fault || control_fault || trajectory_fault || experiment_fault
        || (((s_valid && !batch_capture_active) || local_pending || experiment_log_sending) && fifo_full)
        || (s_valid && !batch_capture_active && experiment_log_sending) || (h_valid && cstate==C_SEND);

    reg [5:0] local_index=0;
    reg [7:0] local_sum;
    reg [7:0] trajectory_sum;
    integer si;
    always @* begin
        local_sum=8'hfe+8'd15;
        for(si=0;si<13;si=si+1) local_sum=local_sum+safety_status[si*8 +: 8];
        trajectory_sum=8'hfe+8'd27;
        for(si=0;si<25;si=si+1) trajectory_sum=trajectory_sum+trajectory_status[si*8 +: 8];
    end
    integer li;

    reg [7:0] h_tx_data;
    reg       h_tx_start;
    wire      h_tx_busy;

    uart_tx #(.CLKS_PER_BIT(HOST_CLKS)) u_htx (
        .clk(clk), .rst(rst), .start(h_tx_start), .data(h_tx_data),
        .tx(host_tx), .busy(h_tx_busy));

    always @(posedge clk) begin
        h_tx_start <= 1'b0;
        if (rst) begin
            wptr <= 0;
            rptr <= 0;
            local_pending<=0;
            local_index<=0;
            local_length<=19;experiment_log_sending<=0;experiment_log_index<=0;experiment_log_read_wait<=0;
        end else begin
            if(begin_experiment_log) begin experiment_log_sending<=1;experiment_log_index<=0;experiment_log_read_wait<=0;end
            if(cstate==C_LOCAL) begin
                local_buf[0]<=255; local_buf[1]<=255; local_buf[2]<=254;
                local_buf[3]<=15; local_buf[4]<=0;
                for(li=0;li<13;li=li+1) local_buf[li+5]<=safety_status[li*8 +: 8];
                local_buf[18]<=~local_sum;
                local_pending<=1; local_index<=0;local_length<=19;
            end else if(cstate==C_TRAJ_REPLY) begin
                local_buf[0]<=255;local_buf[1]<=255;local_buf[2]<=254;
                local_buf[3]<=27;local_buf[4]<=0;
                for(li=0;li<25;li=li+1) local_buf[li+5]<=trajectory_status[li*8 +: 8];
                local_buf[30]<=~trajectory_sum;
                local_pending<=1;local_index<=0;local_length<=31;
            end
            if(experiment_log_sending && !s_valid && !local_pending && !fifo_full) begin
                if(experiment_log_read_wait<2)experiment_log_read_wait<=experiment_log_read_wait+1;
                else begin
                fifo[wptr]<=experiment_log_byte;wptr<=wptr+1;experiment_log_read_wait<=0;
                if(experiment_log_ready) begin experiment_log_sending<=0;experiment_log_index<=0;end
                else experiment_log_index<=experiment_log_index+1;
                end
            end else if (s_valid && !batch_capture_active && !fifo_full) begin
                fifo[wptr] <= s_data;
                wptr       <= wptr + 8'd1;
            end else if(local_pending && !s_valid && !fifo_full) begin
                fifo[wptr]<=local_buf[local_index]; wptr<=wptr+1;
                if(local_index+1==local_length) local_pending<=0;
                else local_index<=local_index+1;
            end
            if (!fifo_empty && !h_tx_busy && !h_tx_start) begin
                h_tx_data  <= fifo[rptr];
                h_tx_start <= 1'b1;
                rptr       <= rptr + 8'd1;
            end
        end
    end

    // led_done flickers on servo traffic; led_ready is the heartbeat.
    reg [19:0] act = 0;
    always @(posedge clk)
        if (s_valid)      act <= 20'hFFFFF;
        else if (act != 0) act <= act - 1'b1;
    assign led_done = safety_latched || (act != 0);

    reg [24:0] hb = 0;
    always @(posedge clk) hb <= hb + 1'b1;
    assign led_ready = hb[24];

endmodule
