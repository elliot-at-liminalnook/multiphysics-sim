`timescale 1ns/1ps
module bridge_trajectory_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,host_rx=1;
    wire host_tx,servo_tx;
    tri1 sig;assign sig=servo_tx;
    top #(.HOST_CLKS(20),.SERVO_CLKS(10),.GUARD(20),.RELEASE_TX(1),.SAFETY_ENABLE(1),
        .BUFFER_HOST(1),.TRAJECTORY_ENABLE(1),.SAFETY_STOP_REPEAT(50000),.SAFETY_STOP_GAP(200)) dut(
        .clk(clk),.host_rx(host_rx),.host_tx(host_tx),.servo_rx(sig),.servo_tx(servo_tx),.key2(1'b0),.led_done(),.led_ready());
    wire [7:0] hb,sb;wire hv,sv;
    uart_rx #(.CLKS_PER_BIT(20)) hmon(.clk(clk),.rst(rst),.rx(host_tx),.enable(1'b1),.data(hb),.valid(hv));
    uart_rx #(.CLKS_PER_BIT(10)) smon(.clk(clk),.rst(rst),.rx(sig),.enable(1'b1),.data(sb),.valid(sv));
    reg [511:0] packets[0:257],message;
    reg [6:0] lengths[0:257];
    reg [271:0] header[0:0];
    reg [31:0] expected[0:0];
    reg [7:0] received[0:4095],command[0:63];
    integer hn=0,sn=0,st=0,stop_pairs=0,groups=0,n,k,b,base;
    reg [7:0] sum=0;
    reg saw_reply_backpressure=0;integer capture;
    always @(posedge clk) begin
        if(!rst && hv) begin received[hn]=hb;hn=hn+1;$fdisplay(capture,"%02x",hb);end
        if(!rst && dut.queued_valid && !dut.queued_ready && dut.fifo_free<31) saw_reply_backpressure=1;
        if(!rst && sv) begin
            command[sn]=sb;if(sn==3) st=sb+4;sn=sn+1;
            if(sn>=6 && sn==st) begin
                // No uploaded instruction, arm, controller or motion can leak.
                if(command[2]!=254 || command[4]!=3 || (command[5]!=8'h2c && command[5]!=8'h28))
                    $fatal(1,"trajectory upload leaked onto motor wire");
                if(command[6]!=0 || (command[5]==8'h2c && command[7]!=0)) $fatal(1,"nonzero motor output");
                if(command[5]==8'h28) stop_pairs=stop_pairs+1;
                sn=0;
            end
        end
        if(!rst && dut.safety.supervisor.armed!=0) $fatal(1,"upload armed a motor");
    end
    task host_byte;
        input [7:0] value;integer bit_index;
        begin @(negedge clk);host_rx=0;repeat(20) @(negedge clk);
            for(bit_index=0;bit_index<8;bit_index=bit_index+1) begin host_rx=value[bit_index];repeat(20) @(negedge clk);end
            host_rx=1;repeat(20) @(negedge clk);
        end
    endtask
    task send;
        input [511:0] value;input integer size;
        begin for(b=0;b<size;b=b+1) host_byte(value[b*8 +: 8]);end
    endtask
    task check_reply;
        input integer start,op,failed,ready,written;
        begin
            wait(hn>=start+31);repeat(30) @(negedge clk);
            sum=0;for(k=2;k<31;k=k+1) sum=sum+received[start+k];
            if(received[start]!=255 || received[start+1]!=255 || received[start+2]!=254 || received[start+3]!=27
                || received[start+4]!=0 || received[start+5]!=1 || received[start+6]!=8'ha2 || sum!=255)
                $fatal(1,"malformed trajectory UART reply");
            if(received[start+7]!=op || received[start+8]!=failed || received[start+9]!=ready || received[start+10]!=0
                || {received[start+16],received[start+15]}!=written || received[start+29]!=1)
                $fatal(1,"wrong trajectory outcome op=%0d failed=%0d ready=%0d written=%0d",received[start+7],received[start+8],received[start+9],{received[start+16],received[start+15]});
            if({received[start+28],received[start+27],received[start+26],received[start+25]}!=50000000) $fatal(1,"wrong FPGA clock");
            if(ready && {received[start+24],received[start+23],received[start+22],received[start+21]}!=expected[0]) $fatal(1,"seal receipt precedes CRC verification");
        end
    endtask
    task upload;
        begin for(n=0;n<5;n=n+1) begin
            base=hn;send(packets[n],lengths[n]);
            check_reply(base,n==0 ? 0 : n==4 ? 2 : 1,0,n==4,n==0 ? 0 : n==4 ? 3 : n);
        end end
    endtask
    task reseal_packet;
        input integer size;
        begin sum=0;for(k=2;k<size-1;k=k+1) sum=sum+message[k*8 +: 8];message[(size-1)*8 +: 8]=~sum;end
    endtask
    task query;
        begin message=0;message[15:0]=16'hffff;message[23:16]=254;message[31:24]=3;message[39:32]=8'ha2;message[47:40]=4;reseal_packet(7);end
    endtask
    initial begin
        capture=$fopen("impl/trajectory-uart-replies.hex","w");
        $readmemh("tb/trajectory-v1/header.hex",header);$readmemh("tb/trajectory-v1/crc.hex",expected);
        $readmemh("tb/trajectory-v1/packets.hex",packets,0,4);$readmemh("tb/trajectory-v1/lengths.hex",lengths,0,4);
        repeat(30) @(negedge clk);rst=0;wait(stop_pairs>=1);
        upload;if(!dut.trajectory_valid) $fatal(1,"sealed plan missing");groups=groups+1;
        base=hn;query;send(message,7);check_reply(base,4,0,1,3);groups=groups+1;
        base=hn;message=packets[1];send(message,45);check_reply(base,1,1,0,3);groups=groups+1;
        base=hn;query;send(message,7);check_reply(base,4,1,0,3);groups=groups+1;
        upload;groups=groups+1;
        // STOP arriving while a 31-byte upload status is still being transmitted:
        // both replies must remain intact and STOP must reach the supervisor now.
        base=hn;query;send(message,7);wait(hn>base);
        message=0;message[15:0]=16'hffff;message[23:16]=254;message[31:24]=3;message[39:32]=8'ha0;message[47:40]=0;reseal_packet(7);send(message,7);
        if(!dut.safety_latched || dut.safety_reason!=10) $fatal(1,"priority STOP delayed by upload reply");
        check_reply(base,4,0,1,3);wait(hn>=base+50);repeat(30) @(negedge clk);
        sum=0;for(k=2;k<19;k=k+1) sum=sum+received[base+31+k];
        if(sum!=255 || received[base+34]!=15 || received[base+37]!=1 || received[base+38]!=10) $fatal(1,"STOP/status replies interleaved");groups=groups+1;
        base=hn;message=packets[0];message[55:48]=2;reseal_packet(42);send(message,42);check_reply(base,0,1,0,3);groups=groups+1;
        // Query bursts fill the host reply FIFO; retained receipts must not wrap.
        // Host ingress may reject overflow, which remains a supervisor fault.
        upload;query;base=hn;
        repeat(12) send(message,7);
        for(n=0;n<12;n=n+1) check_reply(base+n*31,4,0,1,3);
        if(!saw_reply_backpressure) $fatal(1,"reply backpressure was not exercised");
        groups=groups+1;
        $fclose(capture);
        $display("PASS bridge trajectory UART: %0d groups, upload receipts, CRC seal, STOP ordering, no motor writes",groups);$finish;
    end
    initial begin repeat(3000000) @(negedge clk);$fatal(1,"UART test timeout hn=%0d state=%0d",hn,dut.cstate);end
endmodule
