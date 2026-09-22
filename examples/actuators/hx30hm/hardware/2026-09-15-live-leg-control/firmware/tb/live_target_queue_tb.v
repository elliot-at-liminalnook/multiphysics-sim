`timescale 1ns/1ps
module live_target_queue_tb;
 reg clk=0;always #5 clk=~clk;
 reg rst=1,check=0,packet_ok=1,transport_fault=0,locked=0,active=0,start=0,base_valid=1;
 reg [511:0] packet=0;reg [6:0] length=0;reg [8:0] mask=9'b111000000;reg [31:0] period=500000;
 reg [143:0] homes=0;reg [15:0] index=0;
 wire handled,enabled,busy,fault,ready;wire [31:0] run;wire [15:0] frames,written;wire [143:0] targets,deltas;
 live_target_queue #(.FRESH_TICKS(500)) q(.clk(clk),.rst(rst),.check(check),.packet_ok(packet_ok),.transport_fault(transport_fault),
 .locked(locked),.active(active),.start(start),.packet(packet),.packet_length(length),.base_valid(base_valid),.mask(mask),
 .period(period),.homes(homes),.read_index(index),.handled(handled),.enabled(enabled),.busy(busy),.fault(fault),.row_ready(ready),
 .run_id(run),.frames(frames),.written(written),.targets(targets),.deltas(deltas));
 task send; begin @(negedge clk);check=1;@(negedge clk);check=0;while(busy)@(negedge clk);repeat(3)@(negedge clk);end endtask
 task reset;
 begin @(negedge clk);rst=1;active=0;locked=0;start=0;index=0;repeat(2)@(negedge clk);rst=0;end endtask
 task configure;
 begin packet=0;length=14;packet[15:0]=16'hffff;packet[23:16]=254;packet[31:24]=10;packet[39:32]=8'ha4;
 packet[47:40]=0;packet[55:48]=1;packet[87:56]=32'h1234;packet[103:88]=1200;send;
 if(!enabled || frames!=1200 || run!=32'h1234)$fatal(1,"configure");end endtask
 integer a,n;
 task append;
 input [15:0] first; input integer count;input integer value;
 begin packet=0;length=14+6*count;packet[15:0]=16'hffff;packet[23:16]=254;packet[31:24]=length-4;packet[39:32]=8'ha4;
 packet[47:40]=1;packet[79:48]=32'h1234;packet[95:80]=first;packet[103:96]=count;
 for(n=0;n<count;n=n+1)for(a=0;a<3;a=a+1)packet[104+(n*3+a)*16 +: 16]=value;
 send;end endtask
 initial begin
 homes[96 +: 16]=2048;homes[112 +: 16]=2100;homes[128 +: 16]=2200;
 reset;configure;append(0,8,0);append(8,8,16);
 if(written!=16 || !ready || targets[96 +: 16]!=2048)$fatal(1,"initial queue");
 active=1;locked=1;index=8;repeat(4)@(negedge clk);
 if(targets[96 +: 16]!=2064 || $signed(deltas[96 +: 16])!=16)$fatal(1,"row delta/absolute");
 append(16,8,-16);if(!enabled || written!=24)$fatal(1,"ring refill");
 index=16;repeat(4)@(negedge clk);if(targets[112 +: 16]!=2084 || $signed(deltas[112 +: 16])!=-32)$fatal(1,"ring wrap");
 index=24;repeat(3)@(negedge clk);if(ready)$fatal(1,"underflow exposed data");
 reset;configure;append(0,8,0);append(8,8,0);append(16,1,0);if(enabled)$fatal(1,"overflow accepted");
 reset;configure;append(0,1,1);if(enabled || written!=0)$fatal(1,"nonzero first row");
 reset;configure;append(0,8,0);append(8,1,33);if(enabled || written!=8)$fatal(1,"slew accepted");
 reset;configure;append(0,8,0);append(7,1,0);if(enabled)$fatal(1,"rewrite accepted");
 reset;configure;append(0,8,0);active=1;locked=1;repeat(505)@(negedge clk);if(enabled)$fatal(1,"stale lease accepted");
 reset;configure;append(0,8,0);active=1;locked=1;transport_fault=1;@(negedge clk);transport_fault=0;@(negedge clk);if(enabled)$fatal(1,"transport fault ignored");
 $display("PASS live queue: ring refill, bounds, no rewrite, underflow, stale and transport stop");$finish;
 end
 initial begin #1000000;$fatal(1,"timeout");end
endmodule
