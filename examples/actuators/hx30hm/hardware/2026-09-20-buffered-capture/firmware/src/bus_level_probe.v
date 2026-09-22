// Temporary DC measurement aid. Keep BOTH existing 1k series resistors.
// G5 drives a continuous low through its TX resistor; no UART packets are
// generated. Measure the servo-side SIG junction relative to common GND.
// Reload `make bridge-flash` after measurements to restore communication.
module top (
    input wire clk,
    output wire servo_tx,
    input wire servo_rx,
    output wire led_done,
    output wire led_ready
);
    assign servo_tx = 1'b0;
    reg [1:0] rx_sync = 2'b11;
    always @(posedge clk) rx_sync <= {rx_sync[0], servo_rx};
    assign led_done = rx_sync[1];
    reg [24:0] heartbeat = 0;
    always @(posedge clk) heartbeat <= heartbeat + 1'b1;
    assign led_ready = heartbeat[24];
endmodule
