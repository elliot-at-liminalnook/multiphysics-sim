`timescale 1ns/1ps
module bridge_release_tb;
    reg clk = 0;
    always #5 clk = ~clk;
    reg host_rx = 1;
    wire host_tx, servo_tx;
    tri1 sig;
    reg servo_drive = 0, servo_data = 1;
    assign sig = servo_tx;
    assign sig = servo_drive ? servo_data : 1'bz;
    top #(.HOST_CLKS(100), .SERVO_CLKS(20), .GUARD(40), .RELEASE_TX(1)) dut (
        .clk(clk), .host_rx(host_rx), .host_tx(host_tx),
        .servo_tx(servo_tx), .servo_rx(sig), .led_done(), .led_ready());
    reg rst = 1;
    wire [7:0] command_byte, reply_byte;
    wire command_valid, reply_valid;
    uart_rx #(.CLKS_PER_BIT(20)) cmd_rx (
        .clk(clk), .rst(rst), .rx(sig), .enable(!servo_drive),
        .data(command_byte), .valid(command_valid));
    uart_rx #(.CLKS_PER_BIT(100)) reply_rx (
        .clk(clk), .rst(rst), .rx(host_tx), .enable(1'b1),
        .data(reply_byte), .valid(reply_valid));
    reg [7:0] command [0:5];
    reg [7:0] reply [0:5];
    integer nc = 0, nr = 0, i;
    always @(posedge clk) begin
        if (!rst && command_valid) begin
            if (nc >= 6 || command_byte !== command[nc])
                $fatal(1, "Wrong outgoing byte %0d: %02x", nc, command_byte);
            nc = nc + 1;
        end
        if (!rst && reply_valid) begin
            if (nr >= 6 || reply_byte !== reply[nr])
                $fatal(1, "Echo leaked or wrong reply byte %0d: %02x", nr, reply_byte);
            nr = nr + 1;
        end
        if (!rst && servo_drive && servo_tx !== 1'bz)
            $fatal(1, "FPGA still driving during servo reply");
        if (!rst && dut.s_tx_busy && servo_tx === 1'bz)
            $fatal(1, "FPGA released before transmission finished");
    end
    task host_byte;
        input [7:0] value;
        integer bitnum;
        begin
            @(negedge clk); host_rx = 0;
            repeat (100) @(negedge clk);
            for (bitnum = 0; bitnum < 8; bitnum = bitnum + 1) begin
                host_rx = value[bitnum]; repeat (100) @(negedge clk);
            end
            host_rx = 1; repeat (100) @(negedge clk);
        end
    endtask
    task servo_byte;
        input [7:0] value;
        integer bitnum;
        begin
            @(negedge clk); servo_data = 0;
            repeat (20) @(negedge clk);
            for (bitnum = 0; bitnum < 8; bitnum = bitnum + 1) begin
                servo_data = value[bitnum]; repeat (20) @(negedge clk);
            end
            servo_data = 1; repeat (20) @(negedge clk);
        end
    endtask
    initial begin
        command[0]=8'hff; command[1]=8'hff; command[2]=12;
        command[3]=2; command[4]=1; command[5]=8'hf0;
        reply[0]=8'hff; reply[1]=8'hff; reply[2]=12;
        reply[3]=2; reply[4]=0; reply[5]=8'hf1;
        repeat (100) @(negedge clk); rst = 0;
        if (servo_tx !== 1'bz) $fatal(1, "TX not released while idle");
        for (i = 0; i < 6; i = i + 1) host_byte(command[i]);
        wait (nc == 6);
        wait (servo_tx === 1'bz);
        repeat (100) @(negedge clk);
        servo_drive = 1;
        for (i = 0; i < 6; i = i + 1) servo_byte(reply[i]);
        servo_drive = 0;
        wait (nr == 6);
        repeat (1500) @(negedge clk);
        if (nc != 6 || nr != 6) $fatal(1, "Unexpected packet lengths");
        $display("PASS: full command, released TX during reply, no host echo, correct response");
        $finish;
    end
    initial begin #2000000; $fatal(1, "Timeout"); end
endmodule
