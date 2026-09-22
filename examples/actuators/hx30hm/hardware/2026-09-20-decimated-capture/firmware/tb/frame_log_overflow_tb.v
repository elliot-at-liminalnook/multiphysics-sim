`timescale 1ns/1ps
module frame_log_overflow_tb #(parameter MODE=0);
    reg clk=0;always #10 clk=~clk;
    reg rst=1,raw_valid=0,event_pulse=0,input_valid=0,output_ready=0;
    reg [7:0] raw_data=0,read_index=0;
    reg [511:0] input_packet=0;reg [6:0] input_length=53;
    wire input_ready,output_valid,busy,fault,capture_active;
    wire [7:0] read_data;wire [8:0] output_length;
    experiment_frame_log #(.LOG_STRIDE(2)) dut(.*);
    reg [7:0] captured[0:255];integer j,k,n,seen;
    task send;
        begin @(negedge clk);input_valid=1;@(posedge clk);while(!input_ready)@(posedge clk);@(negedge clk);input_valid=0;end
    endtask
    initial begin
        repeat(3)@(negedge clk);rst=0;
        input_packet[56 +: 8]=0;input_packet[40*8 +: 16]=448;
        input_packet[42*8 +: 16]=25;input_packet[44*8 +: 32]=250000;send;
        for(j=0;j<(MODE==0 ? 140 : 21);j=j+1)begin @(negedge clk);raw_valid=1;raw_data=j;end
        @(negedge clk);raw_valid=0;
        if(MODE==0 && !fault)$fatal(1,"overflow did not fault");
        if(MODE==1)begin
            event_pulse=1;@(negedge clk);event_pulse=0;
            input_packet=0;input_packet[56 +: 8]=1;input_packet[14*8 +: 8]=10;
            input_packet[16*8 +: 8]=7;input_packet[17*8 +: 64]=100;
            input_packet[25*8 +: 64]=200;input_packet[34*8 +: 8]=15;send;
        end
        input_packet=0;input_packet[56 +: 8]=4;input_length=64;send;
        for(seen=0;seen<3;seen=seen+1)begin
            wait(output_valid);n=output_length;
            for(k=0;k<n;k=k+1)begin
                @(negedge clk);read_index=k;repeat(2)@(negedge clk);captured[k]=read_data;
            end
            if(seen==0 && captured[7]!=0)$fatal(1,"lost START");
            if(seen==1 && MODE==0)begin
                if(captured[5]!=2 || captured[22]!=128 || captured[26]!=15
                    || {captured[28],captured[27]}!=12 || n!=158)$fatal(1,"missing explicit overflow evidence");
                for(k=0;k<128;k=k+1)if(captured[29+k]!=k)$fatal(1,"raw prefix overwritten");
            end
            if(seen==1 && MODE==1)begin
                if(n!=64 || captured[22]!=21 || captured[23]!=1 || captured[26]!=13
                    || captured[50]!=1 || captured[51]!=10 || captured[52]!=7
                    || {captured[55],captured[54],captured[53]}!=100
                    || {captured[58],captured[57],captured[56]}!=200
                    || captured[59]!=21 || captured[62]!=15)$fatal(1,"event copy or raw reference was lost");
                for(k=0;k<21;k=k+1)if(captured[29+k]!=k)$fatal(1,"partial raw prefix changed");
            end
            if(seen==2 && captured[7]!=4)$fatal(1,"lost TERMINAL");
            @(negedge clk);output_ready=1;@(negedge clk);output_ready=0;
        end
        if(busy || capture_active || (MODE==0 && !fault) || (MODE==1 && fault))$fatal(1,"bad terminal/latched-fault state");
        $display("PASS frame logger mode %0d: raw/metadata retained, START/partial/TERMINAL drain",MODE);$finish;
    end
    initial begin #100000;$fatal(1,"timeout");end
endmodule
