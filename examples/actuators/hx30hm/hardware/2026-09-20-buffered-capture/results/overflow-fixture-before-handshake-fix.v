`timescale 1ns/1ps
module frame_log_overflow_tb;
    reg clk=0;always #10 clk=~clk;
    reg rst=1,raw_valid=0,event_pulse=0,input_valid=0,output_ready=0;
    reg [7:0] raw_data=0;reg [511:0] input_packet=0;reg [6:0] input_length=53;
    wire input_ready,output_valid,busy,fault,capture_active;
    wire [2047:0] output_packet;wire [8:0] output_length;
    experiment_frame_log dut(.*);
    integer i,seen=0;
    task send;
        begin @(negedge clk);input_valid=1;while(!input_ready)@(negedge clk);@(negedge clk);input_valid=0;end
    endtask
    always @(posedge clk)if(output_valid && output_ready)begin
        if(seen==0 && output_packet[56 +: 8]!=0)$fatal(1,"lost START");
        if(seen==1)begin
            if(output_packet[40 +: 8]!=2 || output_packet[176 +: 8]!=128
                || output_packet[208 +: 8]!=3 || output_packet[216 +: 16]!=12
                || output_length!=158)$fatal(1,"missing explicit overflow evidence");
            for(i=0;i<128;i=i+1)if(output_packet[(29+i)*8 +: 8]!=i)$fatal(1,"raw prefix overwritten");
        end
        if(seen==2 && output_packet[56 +: 8]!=4)$fatal(1,"lost TERMINAL");
        seen=seen+1;
    end
    integer j;
    initial begin
        repeat(3)@(negedge clk);rst=0;
        input_packet[56 +: 8]=0;input_packet[40*8 +: 16]=448;
        input_packet[42*8 +: 16]=25;input_packet[44*8 +: 32]=250000;send;
        for(j=0;j<140;j=j+1)begin @(negedge clk);raw_valid=1;raw_data=j;end
        @(negedge clk);raw_valid=0;
        if(!fault)$fatal(1,"overflow did not fault");
        input_packet=0;input_packet[56 +: 8]=4;input_length=64;send;
        repeat(3)@(negedge clk);output_ready=1;
        wait(seen==3);@(negedge clk);output_ready=0;
        if(busy || capture_active || !fault)$fatal(1,"bad terminal/latched-fault state");
        $display("PASS raw overflow: retained first 128 bytes, exact 12-byte loss marker, START/partial/TERMINAL drain");$finish;
    end
    initial begin #100000;$fatal(1,"timeout");end
endmodule
