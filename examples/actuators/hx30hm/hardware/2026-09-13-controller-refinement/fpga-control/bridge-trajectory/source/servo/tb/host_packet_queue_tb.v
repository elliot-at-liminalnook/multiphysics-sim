`timescale 1ns/1ps
module host_packet_queue_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,valid=0,ready=0;
    reg [7:0] data=0;
    wire [511:0] packet;
    wire [6:0] length;
    wire available,urgent,fault;
    integer faults=0,stops=0;
    reg [511:0] saved,max_frame;
    reg [7:0] sum;
    integer k,prior_faults;
    host_packet_queue #(.INTERBYTE_TIMEOUT(30)) dut(.clk(clk),.rst(rst),.data(data),.data_valid(valid),
        .packet(packet),.packet_length(length),.packet_valid(available),.packet_ready(ready),.urgent_stop(urgent),.fault(fault));
    always @(negedge clk) begin if(fault) faults=faults+1;if(urgent) stops=stops+1;end
    task byte_in;
        input [7:0] b;
        begin @(negedge clk);data=b;valid=1;@(negedge clk);valid=0;end
    endtask
    task local_packet;
        input [7:0] op;
        input corrupt;
        reg [7:0] checksum;
        begin checksum=~(8'hfe+8'd3+8'ha0+op);byte_in(255);byte_in(255);byte_in(254);byte_in(3);byte_in(8'ha0);byte_in(op);byte_in(checksum^corrupt);@(negedge clk);end
    endtask
    task reset;
        begin @(negedge clk);rst=1;ready=0;valid=0;repeat(3) @(negedge clk);rst=0;end
    endtask
    task pop;
        begin @(negedge clk);ready=1;@(negedge clk);ready=0;end
    endtask
    initial begin
        reset;
        byte_in(255);byte_in(255);byte_in(254);byte_in(3);byte_in(8'ha0);byte_in(2);
        if(available) $fatal(1,"partial packet exposed");byte_in(8'h5c);
        repeat(3) @(negedge clk);if(!available || length!=7 || packet[47:40]!=2) $fatal(1,"complete status missing");
        saved=packet;repeat(20) @(negedge clk);if(packet!==saved) $fatal(1,"packet changed while stalled");
        local_packet(1,0);pop;if(packet[47:40]!=1) $fatal(1,"normal order changed");pop;if(available) $fatal(1,"queue did not drain");
        $display("PASS 1: complete packets only, stable under stalls, ordered consumption");
        reset;local_packet(2,0);local_packet(1,0);local_packet(0,0);
        if(!urgent && stops!=1) $fatal(1,"priority STOP missing");
        if(packet[47:40]!=0) $fatal(1,"STOP behind normal traffic");pop;if(available) $fatal(1,"STOP left older commands queued");
        $display("PASS 2: full queue cannot block STOP; older commands discarded");
        reset;local_packet(2,0);local_packet(0,1);repeat(3) @(negedge clk);
        if(available || faults!=1 || stops!=1) $fatal(1,"bad checksum accepted or not faulted");
        $display("PASS 3: bad checksum stops/discards rather than forwarding");
        reset;local_packet(2,0);local_packet(2,0);local_packet(2,0);repeat(3) @(negedge clk);
        if(available || faults!=2) $fatal(1,"overflow hidden");
        $display("PASS 4: normal overflow faults and discards queue");
        reset;byte_in(255);byte_in(255);byte_in(254);byte_in(61);repeat(3) @(negedge clk);
        if(faults!=3) $fatal(1,"overlong length not rejected");
        reset;byte_in(255);repeat(35) @(negedge clk);if(faults!=4) $fatal(1,"partial packet never timed out");
        $display("PASS 5: length and inter-byte bounds enforced");
        reset;local_packet(0,0);local_packet(2,1);repeat(3) @(negedge clk);
        if(!available || packet[47:40]!=0) $fatal(1,"later fault erased valid STOP");
        $display("PASS 6: pending STOP survives malformed later traffic");
        reset;prior_faults=faults;local_packet(2,0);local_packet(2,0);
        max_frame=0;max_frame[7:0]=255;max_frame[15:8]=255;max_frame[23:16]=12;max_frame[31:24]=60;max_frame[39:32]=2;
        for(k=5;k<63;k=k+1)max_frame[k*8 +: 8]=k;
        sum=0;for(k=2;k<63;k=k+1)sum=sum+max_frame[k*8 +: 8];max_frame[511:504]=~sum;
        for(k=0;k<63;k=k+1)byte_in(max_frame[k*8 +: 8]);
        @(negedge clk);ready=1;valid=1;data=max_frame[511:504];@(negedge clk);ready=0;valid=0;
        repeat(3)@(negedge clk);if(faults!=prior_faults || length!=7) $fatal(1,"simultaneous full dequeue/enqueue failed");
        pop;if(length!=64 || packet!==max_frame) $fatal(1,"maximum-sized packet corrupted");pop;
        $display("PASS 7: 64-byte packets and simultaneous full dequeue/enqueue");
        reset;local_packet(2,0);byte_in(255);byte_in(255);byte_in(254);byte_in(4);byte_in(8'ha0);byte_in(4);byte_in(12);byte_in(8'h4d);
        repeat(3)@(negedge clk);if(length!=8 || packet[47:40]!=4 || packet[55:48]!=12) $fatal(1,"DISARM priority packet missing");
        pop;if(available) $fatal(1,"DISARM retained queued work");
        $display("PASS 8: addressed DISARM takes priority and clears old work");
        $display("PASS host packet queue");$finish;
    end
    initial begin #100000;$fatal(1,"queue timeout");end
endmodule
