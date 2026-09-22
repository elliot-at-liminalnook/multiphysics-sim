`timescale 1ns/1ps
// Wire-level test: serializers, checksum validation, half-duplex ownership,
// forwarded replies, local status, latching, and autonomous stop transmission.
module bridge_control_tb;
    reg clk=0; always #5 clk=~clk;
    reg host_rx=1,key2=0,rst=1,servo_drive=0,servo_data=1;
    wire host_tx,servo_tx;
    tri1 sig;
    assign sig=servo_tx;
    assign sig=servo_drive ? servo_data : 1'bz;
    top #(.HOST_CLKS(100),.SERVO_CLKS(20),.GUARD(40),.RELEASE_TX(1),.SAFETY_ENABLE(1),
        .SAFETY_FEEDBACK_TIMEOUT(2000000),.SAFETY_COMMAND_TIMEOUT(3000000),.SAFETY_STOP_REPEAT(50000),.SAFETY_STOP_GAP(2000),.SAFETY_REPLY_WAIT(5000)) dut (
        .clk(clk),.host_rx(host_rx),.host_tx(host_tx),.servo_rx(sig),.servo_tx(servo_tx),.key2(key2),.led_done(),.led_ready());
    wire [7:0] cb,hb;
    wire cv,hv;
    uart_rx #(.CLKS_PER_BIT(20)) cr (.clk(clk),.rst(rst),.rx(sig),.enable(!servo_drive),.data(cb),.valid(cv));
    uart_rx #(.CLKS_PER_BIT(100)) hr (.clk(clk),.rst(rst),.rx(host_tx),.enable(1'b1),.data(hb),.valid(hv));
    reg [7:0] cp[0:63],hp[0:4095],p[0:63],r[0:20];
    integer cn=0,total=0,hn=0,zeros=0,offs=0,reads=0,motions=0,locals=0,off_writes=0;
    reg [7:0] cs=0;
    reg [7:0] device=12; reg [15:0] encoder=2048; integer last_pwm=0;
    integer cycle=0,last_zero_end=-100000;
    always @(posedge clk) begin
        cycle=cycle+1;
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
                    if(total!=9 || cp[6]!=0 || cp[7]!=0)$fatal(1,"bad zero packet");zeros=zeros+1;last_zero_end=cycle;
                end else if(cp[2]==254 && cp[4]==3 && cp[5]==8'h28)begin
                    if(cycle-last_zero_end < 3400)$fatal(1,"broadcast stop commands lack processing gap");
                    if(total!=8 || cp[6]!=0)$fatal(1,"bad torque-off packet");offs=offs+1;
                end else if(cp[2]==254 && cp[4]==8'h83)begin
                    if(cp[5]!=8'h2c || cp[6]!=2)$fatal(1,"bad SYNC packet");
                    last_pwm={cp[9],cp[8]};
                    motions=motions+1;
                end else if(cp[2]==12 && cp[4]==3 && cp[5]==8'h2c)motions=motions+1;
                else if(cp[2]==12 && cp[4]==3 && cp[5]==8'h28)off_writes=off_writes+1;
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
            p[4]=8'ha0;p[5]=op;p[6]=device;host_packet(p[3]+4);
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
            p[0]=255;p[1]=255;p[2]=device;p[3]=4;p[4]=2;p[5]=8'h38;p[6]=15;host_packet(8);
            wait(reads==oldreads+1);wait(servo_tx===1'bz);repeat(2500)@(negedge clk);
            for(k=0;k<21;k=k+1)r[k]=0;
            r[0]=255;r[1]=255;r[2]=device;r[3]=17;r[5]=encoder[7:0];r[6]=encoder[15:8];r[11]=120;r[12]=temp;r[18]=100;
            sum=0;for(k=2;k<20;k=k+1)sum=sum+r[k];r[20]=~sum;
            servo_drive=1;for(k=0;k<21;k=k+1)servo_byte(r[k]);servo_drive=0;
            wait(hn==base+21);repeat(100)@(negedge clk);
            for(k=0;k<21;k=k+1)if(hp[base+k]!==r[k])$fatal(1,"reply forwarding mismatch");
        end
    endtask
    task pwm;
        begin p[0]=255;p[1]=255;p[2]=12;p[3]=5;p[4]=3;p[5]=8'h2c;p[6]=100;p[7]=0;host_packet(9);repeat(3000)@(negedge clk);wait(dut.cstate==0);end
    endtask
    task sync_drive;
        integer base,k;reg[7:0] sum;
        begin base=hn;p[0]=255;p[1]=255;p[2]=254;p[3]=7;p[4]=8'h83;p[5]=8'h2c;p[6]=2;p[7]=12;p[8]=100;p[9]=0;
            host_packet(11);wait(hn==base+19);repeat(100)@(negedge clk);
            sum=0;for(k=2;k<19;k=k+1)sum=sum+hp[base+k];
            if(sum!=255 || hp[base+2]!=254 || hp[base+6]!=0 || motions!=2)$fatal(1,"SYNC receipt before forwarding or bad status");
        end
    endtask
    task dropped_off;
        integer before;
        begin before=off_writes;
            p[0]=255;p[1]=255;p[2]=12;p[3]=4;p[4]=3;p[5]=8'h28;p[6]=0;host_packet(8);
            wait(off_writes==before+1);wait(servo_tx===1'bz);
            if(!dut.safety.supervisor.armed[8])$fatal(1,"lost torque-off disabled wire-level watchdog");
        end
    endtask
    task verified_off;
        integer oldreads,base,k;reg[7:0] sum;
        begin oldreads=reads;base=hn;
            p[0]=255;p[1]=255;p[2]=12;p[3]=4;p[4]=2;p[5]=8'h28;p[6]=1;host_packet(8);
            wait(reads==oldreads+1);wait(servo_tx===1'bz);repeat(100)@(negedge clk);
            r[0]=255;r[1]=255;r[2]=12;r[3]=3;r[4]=0;r[5]=0;r[6]=8'hf0;
            servo_drive=1;for(k=0;k<7;k=k+1)servo_byte(r[k]);servo_drive=0;
            wait(hn==base+7);repeat(100)@(negedge clk);
            if(dut.safety.supervisor.armed[8])$fatal(1,"wire-level verified torque-off did not disarm");
        end
    endtask

    task control;
        input [8:0] mask; input integer target,delta,gp,gd,gv,lim;
        integer k,base;
        begin
            base=hn; p[0]=255;p[1]=255;p[2]=254;p[3]=48;p[4]=8'ha1;
            p[5]=mask[7:0];p[6]={7'd0,mask[8]};
            p[7]=gp;p[8]=gp>>8;p[9]=gd;p[10]=gd>>8;p[11]=gv;p[12]=gv>>8;p[13]=lim;p[14]=lim>>8;
            for(k=0;k<9;k=k+1)begin p[15+4*k]=target;p[16+4*k]=target>>8;p[17+4*k]=delta;p[18+4*k]=delta>>8;end
            host_packet(52); wait(hn==base+19);repeat(100)@(negedge clk);
        end
    endtask
    integer n,prior;
    initial begin
        repeat(30)@(negedge clk);rst=0;
        wait(offs==1);local(2,1,1);
        telemetry(40);local(1,0,0);
        control(256,2070,0,256,0,0,50);
        if(motions!=1 || last_pwm!=22)$fatal(1,"FPGA positive feedback calculation %0d",last_pwm);
        encoder=2058;telemetry(40);
        control(256,2040,-2,256,256,256,50);
        if(motions!=2 || last_pwm!=1056)$fatal(1,"FPGA previous-sample / negative calculation %0d",last_pwm);
        // Reusing the same telemetry sample must latch and send a stop.
        prior=offs;control(256,2040,0,256,0,0,50);wait(offs>prior);local(2,1,11);
        encoder=2048;telemetry(40);local(1,0,0);
        control(256,2123,0,256,0,0,75);
        if(motions!=3 || last_pwm!=75)$fatal(1,"bounded 7.5 percent stage failed");
        local(0,1,10);
        for(n=4;n<=12;n=n+1)begin device=n;telemetry(40);local(1,0,0);end
        control(511,2060,3,256,0,256,50);
        if(motions!=4 || total!=35 || last_pwm!=15)$fatal(1,"nine-axis synchronization failed");
        // A batch renewal only renews selected already-armed leases.
        p[0]=255;p[1]=255;p[2]=254;p[3]=5;p[4]=8'ha0;p[5]=5;p[6]=0;p[7]=1;host_packet(9);
        repeat(25000)@(negedge clk);
        if(dut.safety.supervisor.command_age[8]>27000 || dut.safety.supervisor.command_age[0]<40000)
            $fatal(1,"batch lease mask not respected");
        p[6]=255;p[7]=1;host_packet(9);repeat(25000)@(negedge clk);
        for(n=0;n<9;n=n+1) if(dut.safety.supervisor.command_age[n]>27000)$fatal(1,"batch renewal missed axis");
        // A single invalid axis prevents the entire batch from leaving the FPGA.
        for(n=4;n<=12;n=n+1)begin device=n;telemetry(40);end
        prior=offs;control(511,2200,0,256,0,0,50);wait(offs>prior);local(2,1,11);
        if(motions!=4)$fatal(1,"invalid batch partly forwarded");
        $display("PASS FPGA controller: actual UART feedback, previous-state arithmetic, signed PWM, nine-axis batch, reused-feedback and travel rejection");$finish;
    end
    initial begin #100000000;$fatal(1,"controller UART timeout cn=%0d hn=%0d motions=%0d reason=%0d state=%0d",cn,hn,motions,dut.safety_reason,dut.cstate);end
endmodule
