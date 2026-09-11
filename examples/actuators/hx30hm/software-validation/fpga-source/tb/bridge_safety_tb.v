`timescale 1ns/1ps
// Wire-level test: serializers, checksum validation, half-duplex ownership,
// forwarded replies, local status, latching, and autonomous stop transmission.
module bridge_safety_tb;
    reg clk=0; always #5 clk=~clk;
    reg host_rx=1,key2=0,rst=1,servo_drive=0,servo_data=1;
    wire host_tx,servo_tx;
    tri1 sig;
    assign sig=servo_tx;
    assign sig=servo_drive ? servo_data : 1'bz;
    top #(.HOST_CLKS(100),.SERVO_CLKS(20),.GUARD(40),.RELEASE_TX(1),.SAFETY_ENABLE(1),
        .SAFETY_FEEDBACK_TIMEOUT(200000),.SAFETY_COMMAND_TIMEOUT(300000),.SAFETY_STOP_REPEAT(50000)) dut (
        .clk(clk),.host_rx(host_rx),.host_tx(host_tx),.servo_rx(sig),.servo_tx(servo_tx),.key2(key2),.led_done(),.led_ready());
    wire [7:0] cb,hb;
    wire cv,hv;
    uart_rx #(.CLKS_PER_BIT(20)) cr (.clk(clk),.rst(rst),.rx(sig),.enable(!servo_drive),.data(cb),.valid(cv));
    uart_rx #(.CLKS_PER_BIT(100)) hr (.clk(clk),.rst(rst),.rx(host_tx),.enable(1'b1),.data(hb),.valid(hv));
    reg [7:0] cp[0:63],hp[0:4095],p[0:63],r[0:20];
    integer cn=0,total=0,hn=0,zeros=0,offs=0,reads=0,motions=0,locals=0;
    reg [7:0] cs=0;
    always @(posedge clk) begin
        if(!rst && cv)begin
            cp[cn]=cb;
            if(cn<2 && cb!=255)$fatal(1,"bad command header");
            if(cn==2)cs=cb;
            if(cn>2)cs=cs+cb;
            if(cn==3)total=cb+4;
            cn=cn+1;
            if(cn>=6 && cn==total)begin
                if(cs!=255)$fatal(1,"bad outgoing checksum");
                if(cp[4]==8'ha0)$fatal(1,"local command leaked to servo bus");
                if(cp[4]==2)reads=reads+1;
                else if(cp[2]==254 && cp[4]==3 && cp[5]==8'h2c)begin
                    if(total!=9 || cp[6]!=0 || cp[7]!=0)$fatal(1,"bad zero packet");zeros=zeros+1;
                end else if(cp[2]==254 && cp[4]==3 && cp[5]==8'h28)begin
                    if(total!=8 || cp[6]!=0)$fatal(1,"bad torque-off packet");offs=offs+1;
                end else if(cp[2]==254 && cp[4]==8'h83)begin
                    if(total!=11 || cp[5]!=8'h2c || cp[6]!=2 || cp[7]!=12 || cp[8]!=100 || cp[9]!=0)$fatal(1,"bad SYNC packet");
                    motions=motions+1;
                end else if(cp[2]==12 && cp[4]==3 && cp[5]==8'h2c)motions=motions+1;
                else $fatal(1,"unexpected forwarded packet");
                cn=0;
            end
        end
        if(!rst && hv)begin hp[hn]=hb;hn=hn+1;end
        if(!rst && servo_drive && servo_tx!==1'bz)$fatal(1,"bus contention during reply");
        if(!rst && dut.s_tx_busy && servo_tx===1'bz)$fatal(1,"early bus release");
    end
    task host_byte;
        input [7:0] b;integer k;
        begin @(negedge clk);host_rx=0;repeat(100)@(negedge clk);
            for(k=0;k<8;k=k+1)begin host_rx=b[k];repeat(100)@(negedge clk);end
            host_rx=1;repeat(100)@(negedge clk);
        end
    endtask
    task host_packet;
        input integer size;integer k;reg[7:0] sum;
        begin sum=0;for(k=2;k<size-1;k=k+1)sum=sum+p[k];p[size-1]=~sum;
            for(k=0;k<size;k=k+1)host_byte(p[k]);
        end
    endtask
    task servo_byte;
        input [7:0] b;integer k;
        begin @(negedge clk);servo_data=0;repeat(20)@(negedge clk);
            for(k=0;k<8;k=k+1)begin servo_data=b[k];repeat(20)@(negedge clk);end
            servo_data=1;repeat(20)@(negedge clk);
        end
    endtask
    task local;
        input[7:0] op;input integer want_latch,want_reason;
        integer base,k;reg[7:0] sum;
        begin base=hn;p[0]=255;p[1]=255;p[2]=254;p[3]=(op==2 || op==0)?3:4;
            p[4]=8'ha0;p[5]=op;p[6]=12;host_packet(p[3]+4);
            wait(hn==base+19);repeat(100)@(negedge clk);
            sum=0;for(k=2;k<19;k=k+1)sum=sum+hp[base+k];
            if(hp[base]!=255 || hp[base+1]!=255 || hp[base+2]!=254 || hp[base+3]!=15 || hp[base+4]!=0 || hp[base+5]!=1 || sum!=255)
                $fatal(1,"bad FPGA status frame");
            if(hp[base+6]!=want_latch || hp[base+7]!=want_reason)$fatal(1,"wrong status latch=%0d reason=%0d",hp[base+6],hp[base+7]);
            locals=locals+1;
        end
    endtask
    task telemetry;
        input [7:0] temp;
        integer oldreads,base,k;reg[7:0] sum;
        begin oldreads=reads;base=hn;
            p[0]=255;p[1]=255;p[2]=12;p[3]=4;p[4]=2;p[5]=8'h38;p[6]=15;host_packet(8);
            wait(reads==oldreads+1);wait(servo_tx===1'bz);repeat(100)@(negedge clk);
            for(k=0;k<21;k=k+1)r[k]=0;
            r[0]=255;r[1]=255;r[2]=12;r[3]=17;r[6]=8;r[11]=120;r[12]=temp;r[18]=100;
            sum=0;for(k=2;k<20;k=k+1)sum=sum+r[k];r[20]=~sum;
            servo_drive=1;for(k=0;k<21;k=k+1)servo_byte(r[k]);servo_drive=0;
            wait(hn==base+21);repeat(100)@(negedge clk);
            for(k=0;k<21;k=k+1)if(hp[base+k]!==r[k])$fatal(1,"reply forwarding mismatch");
        end
    endtask
    task pwm;
        begin p[0]=255;p[1]=255;p[2]=12;p[3]=5;p[4]=3;p[5]=8'h2c;p[6]=100;p[7]=0;host_packet(9);repeat(3000)@(negedge clk);end
    endtask
    task sync_drive;
        integer base,k;reg[7:0] sum;
        begin base=hn;p[0]=255;p[1]=255;p[2]=254;p[3]=7;p[4]=8'h83;p[5]=8'h2c;p[6]=2;p[7]=12;p[8]=100;p[9]=0;
            host_packet(11);wait(hn==base+19);repeat(100)@(negedge clk);
            sum=0;for(k=2;k<19;k=k+1)sum=sum+hp[base+k];
            if(sum!=255 || hp[base+2]!=254 || hp[base+6]!=0 || motions!=2)$fatal(1,"SYNC receipt before forwarding or bad status");
        end
    endtask
    integer before_stop;
    initial begin
        repeat(30)@(negedge clk);rst=0;
        wait(offs==1);local(2,1,1);
        pwm;if(motions!=0)$fatal(1,"startup motion forwarded");
        telemetry(40);local(1,0,0);pwm;if(motions!=1)$fatal(1,"armed motion not forwarded");
        sync_drive;before_stop=offs;telemetry(60);wait(offs>before_stop);local(2,1,2);
        pwm;if(motions!=2)$fatal(1,"latched motion forwarded");
        telemetry(40);local(1,0,0);before_stop=offs;
        // Stop traffic must arise with no further host input.
        wait(dut.safety_reason==7);wait(offs>before_stop);local(2,1,7);
        before_stop=offs;wait(offs>before_stop);
        telemetry(40);local(1,0,0);key2=1;
        wait(dut.safety_reason==9);local(1,1,9);key2=0;
        $display("PASS: UART safety: %0d zero/off pairs, %0d local responses, telemetry forwarding, motion gating, thermal/stale/physical stop and held-stop rearm rejection",offs,locals);
        $finish;
    end
    initial begin #20000000;$fatal(1,"wire test timeout cn=%0d hn=%0d zeros=%0d offs=%0d reads=%0d reason=%0d",cn,hn,zeros,offs,reads,dut.safety_reason);end
endmodule
