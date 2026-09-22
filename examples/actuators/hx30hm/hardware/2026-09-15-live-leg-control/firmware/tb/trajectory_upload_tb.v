`timescale 1ns/1ps
module trajectory_upload_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,active=0,data_valid=0,allow=1;
    reg [7:0] data=0;
    wire [511:0] packet;
    wire [6:0] length;
    wire queued,queue_fault,urgent,handled,valid,busy,fault,row_ready;
    wire [8:0] mask,frames,written;
    wire [31:0] period,crc32;
    wire [15:0] kp,kd,kv,duty_limit;
    wire [143:0] homes,targets,deltas;
    reg [8:0] index=0;
    host_packet_queue #(.INTERBYTE_TIMEOUT(50)) queue(
        .clk(clk),.rst(rst),.data(data),.data_valid(data_valid),.packet(packet),.packet_length(length),
        .packet_valid(queued),.packet_ready(allow),.urgent_stop(urgent),.fault(queue_fault));
    trajectory_upload dut(.clk(clk),.rst(rst),.active(active),.check(queued && allow),.packet_ok(1'b1),
        .transport_fault(queue_fault),.packet(packet),.packet_length(length),.read_index(index),
        .handled(handled),.valid(valid),.busy(busy),.fault(fault),.row_ready(row_ready),
        .mask(mask),.frames(frames),.written(written),.period(period),.crc32(crc32),
        .kp(kp),.kd(kd),.kv(kv),.duty_limit(duty_limit),.homes(homes),.row_targets(targets),.row_deltas(deltas));
    reg [511:0] packets[0:257],message;
    reg [6:0] lengths[0:257];
    reg [271:0] header[0:0];
    reg [31:0] expected[0:0];
    integer n,b,k,groups=0,errors=0;
    reg [7:0] sum;
    always @(posedge clk) if(fault || queue_fault) errors=errors+1;
    task reset_case;
        begin @(negedge clk);rst=1;active=0;data_valid=0;allow=1;index=0;
            repeat(3) @(negedge clk);rst=0;errors=0;end
    endtask
    task send;
        input [511:0] p;
        input integer size;
        begin
            for(b=0;b<size;b=b+1) begin
                @(negedge clk);data=p[b*8 +: 8];data_valid=1;
                @(negedge clk);data_valid=0;
            end
            repeat(3) @(negedge clk);
        end
    endtask
    task reseal_packet;
        input integer size;
        begin sum=0;for(k=2;k<size-1;k=k+1) sum=sum+message[k*8 +: 8];message[(size-1)*8 +: 8]=~sum;end
    endtask
    task upload;
        begin
            for(n=0;n<header[0][24:16]+2;n=n+1) send(packets[n],lengths[n]);
            while(busy) @(negedge clk);repeat(2) @(negedge clk);
            if(!valid || errors || crc32!=expected[0] || mask!=header[0][8:0] || frames!=header[0][24:16]
                || period!=header[0][63:32] || homes!=header[0][271:128]) $fatal(1,"encoded upload did not validate");
        end
    endtask
    task rejected;
        begin repeat(3) @(negedge clk);if(valid || row_ready || errors==0) $fatal(1,"unsafe upload accepted group %0d",groups);end
    endtask
    initial begin
        $readmemh("tb/trajectory-v1/header.hex",header);
        $readmemh("tb/trajectory-v1/crc.hex",expected);
        $readmemh("tb/trajectory-v1/packets.hex",packets,0,header[0][24:16]+1);
        $readmemh("tb/trajectory-v1/lengths.hex",lengths,0,header[0][24:16]+1);
        reset_case;upload;
        for(n=0;n<frames;n=n+1) begin
            index=n;@(negedge clk);if(!row_ready) $fatal(1,"missing uploaded row");
            for(k=0;k<9;k=k+1) if(targets[k*16 +: 16]!==packets[n+1][64+k*32 +: 16]
                || deltas[k*16 +: 16]!==packets[n+1][80+k*32 +: 16]) $fatal(1,"wire target/delta differ from Rust");
        end
        groups=groups+1;
        reset_case;upload;message=packets[0];message[55:48]=2;reseal_packet(42);send(message,42);rejected;groups=groups+1;
        reset_case;upload;message=packets[0];message[65]=1;reseal_packet(42);send(message,42);rejected;
        reset_case;upload;message=packets[0];message[81]=1;reseal_packet(42);send(message,42);rejected;groups=groups+1;
        reset_case;send(packets[0],42);message=packets[1];message[57]=1;reseal_packet(45);send(message,45);rejected;groups=groups+1;
        reset_case;upload;message=packets[4];message[47:40]=3;reseal_packet(11);send(message,11);rejected;groups=groups+1;
        reset_case;upload;message=packets[0];message[24 +: 8]=39;reseal_packet(43);send(message,43);rejected;groups=groups+1;
        reset_case;upload;message=packets[1];message[352]=~message[352];send(message,45);rejected;groups=groups+1;
        reset_case;upload;send(packets[1],10);repeat(55) @(negedge clk);rejected;groups=groups+1;
        reset_case;upload;active=1;send(packets[0],42);rejected;groups=groups+1;
        reset_case;upload;
        message=0;message[15:0]=16'hffff;message[23:16]=254;message[31:24]=3;message[39:32]=8'ha0;message[47:40]=1;
        reseal_packet(7);send(message,7);if(!valid || errors) $fatal(1,"unrelated status invalidated trajectory");groups=groups+1;
        reset_case;for(n=0;n<4;n=n+1) send(packets[n],lengths[n]);send(packets[4],11);
        if(!busy) $fatal(1,"expected verification scan");send(packets[1],45);rejected;groups=groups+1;
        $display("PASS trajectory upload: %0d groups, Rust packets through host queue to sealed RAM",groups);$finish;
    end
    initial begin repeat(100000) @(negedge clk);$fatal(1,"test timeout");end
endmodule
