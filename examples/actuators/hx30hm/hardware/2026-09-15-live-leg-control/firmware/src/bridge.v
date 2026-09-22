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

    uart_rx #(.CLKS_PER_BIT(HOST_CLKS)) u_hrx (
        .clk(clk), .rst(rst), .rx(host_rx), .enable(1'b1),
        .data(h_data), .valid(h_valid));

    // Collect a whole packet: FF FF ID LEN, then LEN more bytes.
    // Total length is therefore LEN + 4.
    localparam C_H1 = 3'd0, C_H2 = 3'd1, C_BODY = 3'd2, C_SEND = 3'd3,
               C_CHECK=3'd4, C_LOCAL=3'd5, C_RECEIPT=3'd6;

    reg [2:0] cstate = C_H1;
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
    generate if(SAFETY_ENABLE) begin: safety
        wire emergency;
        button #(.DEBOUNCE_CYC(50000)) stop_button (
            .clk(clk),.rst(rst),.raw(key2),.state(emergency),.pressed(),.released());
        hx_safety #(.FEEDBACK_TIMEOUT(SAFETY_FEEDBACK_TIMEOUT),
            .COMMAND_TIMEOUT(SAFETY_COMMAND_TIMEOUT), .STOP_REPEAT(SAFETY_STOP_REPEAT)) supervisor (
            .clk(clk),.rst(rst),.check_host(check_safety),.host_packet(safety_packet),.host_length(ptot),
            .host_checksum(packet_checksum),.emergency_stop(emergency),.bridge_fault(bridge_fault),
            .allow_forward(safety_allow),.local_command(safety_local),.local_status(safety_status),
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
    assign servo_tx = (RELEASE_TX && cstate != C_SEND && !stop_active && !s_tx_busy)
                    ? 1'bz : s_tx_pin;

    reg [$clog2(GUARD+1)-1:0] guard = 0;
    wire listening = (cstate != C_SEND) && !stop_active && !s_tx_busy && (guard == 0);

    always @(posedge clk) begin
        s_tx_start <= 1'b0;
        stop_accepted <= 1'b0;

        if (rst) begin
            cstate <= C_H1;
            pcnt   <= 0;
            guard  <= 0;
            stop_active <= 0;
            stop_index <= 0;
            stop_gap <= 0;
            reply_wait <= 0;
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
                if(stop_index==16) stop_active<=0;
                else stop_index<=stop_index+1;
            end

            case (cstate)
                C_H1: if (h_valid && h_data == 8'hFF) begin
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
                    if(safety_local) cstate<=C_LOCAL;
                    else if(safety_allow) begin pidx<=0; cstate<=C_SEND; end
                    else begin pcnt<=0; cstate<=C_H1; end
                end
                C_LOCAL: begin pcnt<=0; cstate<=C_H1; end
                C_RECEIPT: if(!s_tx_busy && !stop_active) cstate<=C_LOCAL;
                C_SEND: if (!stop_active && !s_tx_busy) begin
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
    reg [7:0] local_buf[0:18];
    reg local_pending=0;
    assign bridge_fault=((s_valid || local_pending) && (wptr+8'd1)==rptr) || (h_valid && cstate==C_SEND);

    reg [4:0] local_index=0;
    reg [7:0] local_sum;
    integer si;
    always @* begin
        local_sum=8'hfe+8'd15;
        for(si=0;si<13;si=si+1) local_sum=local_sum+safety_status[si*8 +: 8];
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
        end else begin
            if(cstate==C_LOCAL) begin
                local_buf[0]<=255; local_buf[1]<=255; local_buf[2]<=254;
                local_buf[3]<=15; local_buf[4]<=0;
                for(li=0;li<13;li=li+1) local_buf[li+5]<=safety_status[li*8 +: 8];
                local_buf[18]<=~local_sum;
                local_pending<=1; local_index<=0;
            end
            if (s_valid) begin
                fifo[wptr] <= s_data;
                wptr       <= wptr + 8'd1;
            end else if(local_pending) begin
                fifo[wptr]<=local_buf[local_index]; wptr<=wptr+1;
                if(local_index==18) local_pending<=0;
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
