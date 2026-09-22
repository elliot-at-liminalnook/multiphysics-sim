// Timestamped bus transactions, independent of control and bus arbitration.
// FIFO overflow drops telemetry, never stalls the servo/control sequencer.
// Times bracket request acceptance -> validated reply/error, NOT internal ADC
// sampling. See ../capture.md for the versioned payload and timing contract.
module capture_stream #(
    parameter CLOCK_HZ = 50_000_000,
    parameter HOST_CLKS_PER_BIT = 50,
    parameter FIFO_BITS = 4
)(
    input wire clk, rst,
    input wire request,
    input wire [7:0] id, instruction,
    input wire [3:0] request_length,
    input wire [63:0] request_params,
    input wire [7:0] control_flags,
    input wire reply_valid, reply_bad, reply_timeout,
    input wire [7:0] error_byte,
    input wire [3:0] reply_length,
    input wire [63:0] reply_params,
    output wire tx
);
    // The bus parser only writes returned bytes; unused lanes may retain an
    // older reply (or power-up X). Never transmit those lanes or CRC over them.
    function [63:0] clean_reply;
        input [63:0] value;
        input [3:0] length;
        integer k;
        begin
            clean_reply = 0;
            for(k=0;k<8;k=k+1)
                if(k<length) clean_reply[k*8 +: 8] = value[k*8 +: 8];
        end
    endfunction
    localparam DEPTH = 1 << FIFO_BITS;
    reg [63:0] ticks = 0, start_ticks = 0;
    reg [31:0] transaction = 0, pending_transaction = 0, dropped = 0;
    reg pending = 0;
    reg [7:0] pending_id = 0, pending_instruction = 0, pending_flags = 0;
    reg [3:0] pending_length = 0;
    reg [63:0] pending_params = 0;
    reg [447:0] fifo [0:DEPTH-1];
    reg [FIFO_BITS-1:0] wr = 0, rd = 0;
    reg [FIFO_BITS:0] count = 0;
    reg [447:0] payload = 0;
    reg send = 0;
    wire frame_busy;
    wire terminal = pending && (reply_valid || reply_bad || reply_timeout);
    wire enqueue = terminal && count < DEPTH;
    wire dequeue = count != 0 && !frame_busy && !send;
    wire [7:0] outcome = reply_timeout ? 8'd2 : reply_bad ? 8'd1 : 8'd0;
    wire [31:0] clock_hz = CLOCK_HZ;
    wire [5:0] address;
    wire [7:0] pay_data = payload >> (address * 8);
    wire [7:0] uart_data;
    wire uart_start, uart_busy;
    reg [7:0] sequence = 0;

    link_tx frame(.clk(clk),.rst(rst),.send(send),.op(8'h83),.plen(8'd56),
        .seq(sequence),.busy(frame_busy),.pay_addr(address),.pay_data(pay_data),
        .tx_data(uart_data),.tx_start(uart_start),.tx_busy(uart_busy));
    uart_tx #(.CLKS_PER_BIT(HOST_CLKS_PER_BIT)) uart(.clk(clk),.rst(rst),
        .start(uart_start),.data(uart_data),.tx(tx),.busy(uart_busy));

    always @(posedge clk) begin
        send <= 0;
        if (rst) begin
            ticks <= 0; start_ticks <= 0; transaction <= 0; pending <= 0;
            dropped <= 0; wr <= 0; rd <= 0; count <= 0; sequence <= 0;
        end else begin
            ticks <= ticks + 1'b1;
            if (request) begin
                start_ticks <= ticks;
                pending_transaction <= transaction;
                transaction <= transaction + 1'b1;
                pending_id <= id; pending_instruction <= instruction;
                pending_params <= request_params; pending_length <= request_length;
                pending_flags <= control_flags;
                pending <= 1;
            end
            if (terminal) pending <= 0;
            if (terminal && !enqueue) dropped <= dropped + 1'b1;
            if (enqueue) begin
                // Packed little endian within each multibyte scalar.
                fifo[wr] <= {24'd0, clock_hz, (reply_valid ? error_byte : 8'd0),
                    (reply_valid ? clean_reply(reply_params,reply_length) : 64'd0), pending_params,
                    ticks, start_ticks, dropped, pending_transaction,
                    8'd0, pending_flags, (reply_valid ? {4'd0,reply_length} : 8'd0),
                    {4'd0,pending_length}, pending_instruction, pending_id, outcome, 8'd1};
                wr <= wr + 1'b1;
            end
            if (dequeue) begin
                payload <= fifo[rd]; rd <= rd + 1'b1;
                sequence <= sequence + 1'b1; send <= 1;
            end
            case ({enqueue,dequeue})
                2'b10: count <= count + 1'b1;
                2'b01: count <= count - 1'b1;
                default: ;
            endcase
        end
    end
endmodule
