`timescale 1ns/1ps
module experiment_event_packet_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,valid=0,ready=0;
    reg [2:0] kind=0;
    reg [31:0] run_id=32'h31415926;
    reg [8:0] frame=0;
    reg [7:0] id=254,outcome=0,sequence=0,error=0;
    reg [63:0] request_ticks=64'h100000020,completion_ticks=64'h100000020;
    reg [5:0] reported=16;
    reg [4:0] stored=16;
    reg [215:0] data=0;
    wire record_ready,packet_valid,fault;
    wire [511:0] packet;
    wire [6:0] length;
    experiment_event_packet dut(.clk(clk),.rst(rst),.record_valid(valid),.record_ready(record_ready),
        .kind(kind),.run_id(run_id),.frame(frame),.motor_id(id),.outcome(outcome),.sequence(sequence),
        .device_error(error),.request_ticks(request_ticks),.completion_ticks(completion_ticks),
        .reported_width(reported),.stored_width(stored),.data(data),.packet_valid(packet_valid),
        .packet_ready(ready),.packet(packet),.packet_length(length),.fault(fault));
    reg [31:0] crc[0:0];
    reg [511:0] snapshot;
    reg [7:0] sum;
    reg [143:0] pwms;
    reg [63:0] base=64'h100000020;
    integer log,i,j,f,axis,packets=0,faults=0,groups=0;
    always @(posedge clk) if(fault) faults=faults+1;
    task emit;
        input write_log;
        begin
            wait(record_ready);@(negedge clk);valid=1;@(negedge clk);valid=0;
            wait(packet_valid);@(negedge clk);
            if(length!=37+stored || packet[39:0]!={8'd0,(8'd33+{3'd0,stored}),8'hfd,16'hffff}) $fatal(1,"event envelope");
            sum=0;for(j=2;j<length;j=j+1) sum=sum+packet[j*8 +: 8];
            if(sum!=255) $fatal(1,"event checksum");
            if(packet[63:40]!={5'd0,kind,8'ha3,8'd1} || packet[95:64]!=run_id
                || packet[111:96]!={7'd0,frame} || packet[135:112]!={sequence,outcome,id}
                || packet[199:136]!=request_ticks || packet[263:200]!=completion_ticks
                || packet[287:264]!={3'd0,stored,2'd0,reported,error}) $fatal(1,"event fields");
            for(j=0;j<stored;j=j+1) if(packet[(36+j)*8 +: 8]!=data[j*8 +: 8]) $fatal(1,"raw event data");
            if(write_log) begin
                for(j=0;j<length;j=j+1) $fwrite(log,"%02x",packet[j*8 +: 8]);
                $fwrite(log,"\n");packets=packets+1;
            end
            snapshot=packet;repeat(5) begin @(negedge clk);if(!packet_valid || record_ready || packet!=snapshot) $fatal(1,"stalled evidence changed");end
            ready=1;@(negedge clk);ready=0;
        end
    endtask
    task reject;
        begin @(negedge clk);valid=1;@(negedge clk);valid=0;repeat(70) @(negedge clk);if(packet_valid) $fatal(1,"invalid record encoded");end
    endtask
    initial begin
        $readmemh("tb/trajectory-v1/crc.hex",crc);
        log=$fopen("impl/device-events.hex","w");if(!log) $fatal(1,"cannot open fixture");
        repeat(3) @(negedge clk);rst=0;
        data={88'd0,32'd50000000,32'd2500000,16'd3,16'd511,crc[0]};emit(1);
        for(f=0;f<3;f=f+1) begin
            frame=f;request_ticks=base+f*64'd2500000+100;
            for(axis=0;axis<9;axis=axis+1) begin
                kind=1;id=axis+4;reported=15;stored=15;sequence=(254+f)&255;
                completion_ticks=request_ticks+100;data=0;data[15:0]=2048+axis*10;
                data[55:48]=121;data[63:56]=51;data[119:104]=26;emit(1);
                request_ticks=completion_ticks+100;
            end
            kind=2;id=254;reported=18;stored=18;sequence=0;data=0;
            for(axis=0;axis<9;axis=axis+1) pwms[axis*16 +: 16]=f==0 ? 0 : 75+(((axis%2)==(f==1 ? 1 : 0)) ? 1024 : 0);
            data[143:0]=pwms;completion_ticks=request_ticks+100;emit(1);request_ticks=completion_ticks+100;
            for(axis=0;axis<9;axis=axis+1) begin
                kind=3;id=axis+4;reported=6;stored=6;completion_ticks=request_ticks+100;
                data=0;data[7:0]=1;data[47:32]=pwms[axis*16 +: 16];emit(1);request_ticks=completion_ticks+100;
            end
        end
        kind=4;id=254;reported=27;stored=27;request_ticks=base;completion_ticks=base+7500500;
        data=0;data[63:0]=base+7500000;data[127:64]=completion_ticks;emit(1);groups=groups+1;
        if(packets!=59 || faults!=0) $fatal(1,"incomplete fixture");
        $fclose(log);
        // Error/overlong audits keep reported width separate from retained bytes.
        kind=3;id=4;reported=58;stored=15;outcome=1;error=8;data=215'h123456;emit(0);
        reported=0;stored=0;data=0;emit(0);groups=groups+1;
        // Invalid metadata cannot generate a plausible success packet.
        kind=7;reject;kind=1;id=3;reported=15;stored=15;outcome=0;error=0;reject;
        id=4;stored=14;reject;stored=15;request_ticks=completion_ticks+1;reject;
        request_ticks=base;kind=4;id=254;reported=27;stored=27;data=0;reject;
        if(faults!=5) $fatal(1,"missing event validation faults");groups=groups+1;
        $display("PASS event serializer: %0d groups; 59 complete-run packets, 64-bit clocks, signed audits, error widths, stable backpressure and invalid-record rejection",groups);$finish;
    end
    initial begin repeat(20000) @(negedge clk);$fatal(1,"event serializer timeout");end
endmodule
