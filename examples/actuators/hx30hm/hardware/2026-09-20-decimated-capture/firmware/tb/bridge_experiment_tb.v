`begin_keywords "1800-2017"
`timescale 1ns/1ps
// Complete UART/FPGA/controller/recorder/supervisor path. Synthetic stationary
// motors expose exact full-duty saturation and audit identity, not motor physics.
module bridge_experiment_tb #(parameter FAST=1, parameter LOG_STRIDE=2);
    localparam MOTORS=FAST ? 3 : 9, FIRST_ID=FAST ? 10 : 4, FRAMES=FAST ? 25 : 5;
    localparam MASK=FAST ? 448 : 511;
    localparam HCLKS=50;
    integer scenario=0; initial if(!$value$plusargs("case=%d",scenario)) scenario=0;
    reg clk=0;always #10 clk=~clk;
    reg rst=1,host_rx=1,drive=0,servo_data=1;
    wire host_tx,servo_tx;
    `ifdef VERILATOR
    // Two-state simulators need the explicit output-enable bus resolution.
    wire sig=drive ? servo_data : dut.servo_tx_enable ? dut.s_tx_pin : 1'b1;
`else
    tri1 sig;assign sig=servo_tx;assign sig=drive ? servo_data : 1'bz;
`endif
    top #(.HOST_CLKS(HCLKS),.SERVO_CLKS(50),.GUARD(100),.RELEASE_TX(1),.SAFETY_ENABLE(1),
        .BUFFER_HOST(1),.TRAJECTORY_ENABLE(1),.EXPERIMENT_ENABLE(1),.FIXED_GAINS(1),.LOG_STRIDE(LOG_STRIDE),
        .SAFETY_COMMAND_TIMEOUT(4000000),
        .SAFETY_FEEDBACK_TIMEOUT(10000000),.SAFETY_STOP_GAP(100000),.SAFETY_REPLY_WAIT(100000)) dut(
        .clk(clk),.host_rx(host_rx),.host_tx(host_tx),.servo_rx(sig),.servo_tx(servo_tx),.key2(1'b0),.led_done(),.led_ready());
    wire [7:0] sb,hb;wire sv,hv;
    uart_rx #(.CLKS_PER_BIT(50)) smon(.clk(clk),.rst(rst),.rx(sig),.enable(!drive),.data(sb),.valid(sv));
    uart_rx #(.CLKS_PER_BIT(HCLKS)) hmon(.clk(clk),.rst(rst),.rx(host_tx),.enable(1'b1),.data(hb),.valid(hv));
    reg [7:0] command[0:63],response[0:63],host_frame[0:255],message[0:63];
    reg [15:0] pwm[0:8];reg torque[0:8];
    reg [511:0] packets[0:26];reg [6:0] lengths[0:26];reg [31:0] crc[0:0];
    reg reply_pending=0;integer reply_id=0,reply_address=0,reply_width=0;
    integer cn=0,ct=0,hn=0,ht=0,controls=0,positive=0,negative=0,stops=0;
    integer statuses=0,receipts=0,starts=0,terminals=0,records=0,raw_replies=0;
    integer terminal_result=-1,frame=0,axis=0,i,j,base,fd,deadline=0;
    reg [7:0] cs=0,hs=0;reg running=0;
    reg [63:0] last_control_ticks=0;
    always @(posedge clk) begin
`ifndef VERILATOR
        if(!rst && drive && servo_tx!==1'bz) $fatal(1,"physical tri-state contention");
        if(!rst && dut.s_tx_busy && servo_tx===1'bz) $fatal(1,"physical output released before final wire bit");
`endif
        if(!rst && drive && dut.servo_tx_enable) $fatal(1,"bus contention");
        if(!rst && dut.s_tx_busy && !dut.servo_tx_enable) $fatal(1,"released before final wire bit");
        if(!rst && sv) begin
            command[cn]=sb;if(cn==2)cs=sb;else if(cn>2)cs=cs+sb;
            if(cn==3)ct=sb+4;cn=cn+1;
            if(cn>=6 && cn==ct) begin
                if(cs!==255)$fatal(1,"outgoing checksum");
                if(command[4]==2) begin
                    if(reply_pending)$fatal(1,"overlapping physical requests");
                    reply_id=command[2];reply_address=command[5];reply_width=command[6];reply_pending=1;
                end else if(command[2]==254 && command[4]==3) begin
                    if(command[5]==8'h2c)begin
                        if(command[6]!=0 || command[7]!=0)$fatal(1,"nonzero broadcast");
                        for(i=0;i<9;i=i+1)pwm[i]=0;
                    end else if(command[5]==8'h28)begin
                        if(command[6]!=0)$fatal(1,"bad stop");for(i=0;i<9;i=i+1)torque[i]=0;stops=stops+1;
                    end else $fatal(1,"unexpected broadcast");
                end else if(command[4]==8'h83) begin
                    if(command[5]!=8'h2c || command[6]!=2 || ct!=8+3*MOTORS)$fatal(1,"bad scoped drive");
                    for(i=0;i<MOTORS;i=i+1) begin
                        if(command[7+i*3]!=i+FIRST_ID)$fatal(1,"motor order");
                        pwm[i+FIRST_ID-4]={command[9+i*3],command[8+i*3]};
                        if((pwm[i+FIRST_ID-4]&1023)>1000)$fatal(1,"PWM exceeds full scale");
                        if(pwm[i+FIRST_ID-4]==1000)positive=positive+1;
                        if(pwm[i+FIRST_ID-4]==2024)negative=negative+1;
                    end
                    controls=controls+1;
                end else if(command[4]==3 && command[2]>=4 && command[2]<=12 && command[5]==8'h28)
                    torque[command[2]-4]=command[6];
                else $fatal(1,"unexpected motor traffic");
                cn=0;
            end
        end
        if(!rst && hv)begin
            $fdisplay(fd,"%02x",hb);
            host_frame[hn]=hb;
            if(hn<2 && hb!=255)$fatal(1,"interleaved host packet/header");
            if(hn==2)hs=hb;else if(hn>2)hs=hs+hb;
            if(hn==3)begin ht=hb+4;if(ht>255 || ht<6)$fatal(1,"invalid host frame size");end
            hn=hn+1;
            if(hn>=6 && hn==ht)begin
                if(hs!==255)$fatal(1,"interleaved host frame/checksum");
                if(host_frame[2]==253)begin
                    if(host_frame[5]!=1 && host_frame[5]!=2)$fatal(1,"record version");
                    if({host_frame[11],host_frame[10],host_frame[9],host_frame[8]}!=32'h12345678)$fatal(1,"run identity changed");
                    records=records+(host_frame[5]==2 ? host_frame[23] : 1);
                    if(host_frame[5]==1 && host_frame[7]==0)starts=starts+1;
                    if(host_frame[5]==1 && host_frame[7]==4)begin terminals=terminals+1;terminal_result=host_frame[15];end
                end else if(host_frame[2]==254)begin
                    if(ht==31)begin
                        if(host_frame[29]!=(LOG_STRIDE==2 ? 103 : 39))$fatal(1,"missing autonomous capability");receipts=receipts+1;
                    end else if(ht==19)statuses=statuses+1;
                    else $fatal(1,"unexpected local reply");
                end else raw_replies=raw_replies+1;
                $fflush(fd);hn=0;
            end
        end
    end
    task host_byte;
        input[7:0] value;integer k;
        begin @(negedge clk);host_rx=0;repeat(HCLKS)@(negedge clk);
            for(k=0;k<8;k=k+1)begin host_rx=value[k];repeat(HCLKS)@(negedge clk);end
            host_rx=1;repeat(HCLKS)@(negedge clk);
        end
    endtask
    task send;
        input integer count;integer k;reg[7:0] sum;
        begin sum=0;for(k=2;k<count-1;k=k+1)sum=sum+message[k];message[count-1]=~sum;
            for(k=0;k<count;k=k+1)host_byte(message[k]);
        end
    endtask
    task servo_byte;
        input[7:0] value;integer k;
        begin @(negedge clk);servo_data=0;repeat(50)@(negedge clk);
            for(k=0;k<8;k=k+1)begin servo_data=value[k];repeat(50)@(negedge clk);end
            servo_data=1;repeat(50)@(negedge clk);
        end
    endtask
    integer n,k,size,position;reg[7:0] sum;
    initial forever begin
        wait(reply_pending);wait(!dut.s_tx_busy && !dut.servo_tx_enable);repeat(1500)@(negedge clk);
        if(!(running && scenario==1 && reply_id==(FAST ? 10 : 6) && reply_address==8'h38))begin
            size=reply_width+6;for(n=0;n<64;n=n+1)response[n]=0;
            response[0]=255;response[1]=255;response[2]=reply_id;response[3]=reply_width+2;
            if(reply_address==8'h38)begin
                position=2000+reply_id-4;response[5]=position;response[6]=position>>8;response[11]=120;response[12]=40;
            end else if(reply_address==8'h28)begin
                response[5]=torque[reply_id-4];response[9]=pwm[reply_id-4];response[10]=pwm[reply_id-4]>>8;
                if(running && scenario==4 && reply_id==(FAST ? 11 : 8) && controls>=2)response[9]=response[9]+1;
            end else $fatal(1,"unsupported synthetic read");
            sum=0;for(n=2;n<size-1;n=n+1)sum=sum+response[n];response[size-1]=~sum;
            if(running && scenario==6 && reply_address==8'h38) response[size-1]=response[size-1]^1;
            if(running && scenario==7 && reply_address==8'h38) size=7;
            drive=1;for(k=0;k<size;k=k+1)servo_byte(response[k]);drive=0;
        end
        reply_pending=0;
    end
    initial begin
        fd=$fopen(FAST ? $sformatf("impl/bridge_experiment_fast_case%0d.hex",scenario) : $sformatf("impl/bridge_experiment_case%0d.hex",scenario),"w");
        if(FAST) begin
            $readmemh("tb/trajectory-fast-loop/packets.hex",packets,0,26);
            $readmemh("tb/trajectory-fast-loop/lengths.hex",lengths,0,26);
            $readmemh("tb/trajectory-fast-loop/crc.hex",crc);
        end else begin
            $readmemh("tb/trajectory-full-drive/packets.hex",packets,0,6);
            $readmemh("tb/trajectory-full-drive/lengths.hex",lengths,0,6);
            $readmemh("tb/trajectory-full-drive/crc.hex",crc);
        end
        for(j=0;j<9;j=j+1)begin pwm[j]=0;torque[j]=0;end
        repeat(40)@(negedge clk);rst=0;wait(stops>0);wait(!dut.stop_active && !dut.s_tx_busy && dut.stop_gap==0);
        // Match the physical host: upload while unarmed, then inspect and arm.
        for(j=0;j<FRAMES+2;j=j+1)begin
            for(i=0;i<64;i=i+1)message[i]=packets[j][i*8 +: 8];base=receipts;send(lengths[j]);wait(receipts>base);
            if(dut.trajectory_failed)$fatal(1,"upload rejected");
        end
        for(j=FIRST_ID;j<=12;j=j+1)begin
            message[0]=255;message[1]=255;message[2]=j;message[3]=4;message[4]=2;message[5]=8'h38;message[6]=15;
            base=raw_replies;send(8);wait(raw_replies>base);
            message[2]=254;message[3]=4;message[4]=8'ha0;message[5]=1;message[6]=j;
            base=statuses;send(8);wait(statuses>base);
            message[2]=j;message[3]=4;message[4]=3;message[5]=8'h28;message[6]=1;
            send(8);wait(torque[j-4]==1);wait(dut.cstate==0 && !dut.s_tx_busy && dut.reply_wait==0);
        end
        if(!dut.trajectory_valid || dut.safety_latched)$fatal(1,"pre-start readiness");
        message[0]=255;message[1]=255;message[2]=254;message[3]=11;message[4]=8'ha2;message[5]=3;
        for(i=0;i<4;i=i+1)message[6+i]=crc[0][i*8 +: 8];
        message[10]=8'h78;message[11]=8'h56;message[12]=8'h34;message[13]=8'h12;
        base=receipts;running=1;send(15);wait(receipts>base);
        if(dut.trajectory_failed)$fatal(1,"valid START rejected");
        if(scenario==3)begin
            wait(controls>=2);message[0]=255;message[1]=255;message[2]=254;message[3]=3;message[4]=8'ha0;message[5]=0;send(7);
        end
        wait(terminals==1);wait(!dut.experiment_locked);repeat(10000)@(negedge clk);
        if(starts!=1 || !dut.safety_latched)$fatal(1,"lost start/stop state");
        for(j=0;j<9;j=j+1)if(pwm[j]!=0 || torque[j]!=0)$fatal(1,"nonzero final drive");
        if(scenario==0 && (controls!=FRAMES || positive<MOTORS || negative<MOTORS || terminal_result!=0 || records!=2+((FRAMES+LOG_STRIDE-1)/LOG_STRIDE)*(2*MOTORS+1)))
            $fatal(1,"incomplete full-scale run: controls %0d +full %0d -full %0d result %0d records %0d",controls,positive,negative,terminal_result,records);
        if(scenario!=0 && terminal_result==0)$fatal(1,"fault scenario reported success");
        if(scenario==2 && dut.safety_reason!=8)$fatal(1,"independent command watchdog bypassed");
        $display("PASS full UART case %0d: controls %0d, +100%% %0d, -100%% %0d, terminal %0d; all modeled motors zero/off",scenario,controls,positive,negative,terminal_result);
        $fclose(fd);$finish;
    end
    // Exercise interleaved host heartbeats during autonomous UART ownership.
    // The normal run lasts longer than this test's independent 80 ms lease.
    initial begin #1; if(scenario!=2) begin
        wait(starts==1);
        while(terminals==0) begin
            if(dut.experiment_active) begin
                message[0]=255;message[1]=255;message[2]=254;message[3]=5;
                message[4]=8'ha0;message[5]=5;message[6]=MASK&255;message[7]=MASK>>8;
                send(9);
            end
            repeat(2500000)@(negedge clk);
        end
    end
    end
    initial begin #1; if(scenario==5) begin
        wait(controls>=2); force dut.h_tx_busy=1'b1;
        wait(dut.safety_latched); wait(!dut.stop_active && !dut.s_tx_busy);
        release dut.h_tx_busy;
    end end
    reg last_observed_event=0;
    always @(posedge clk) begin
        if(dut.experiment.session.event_valid && !last_observed_event)
            $display("EVENT %0d %0d %0d %0d %0d",dut.experiment.session.event_frame,dut.experiment.session.event_kind,dut.experiment.session.event_id,dut.experiment.session.request_ticks,dut.experiment.session.completion_ticks);
        last_observed_event<=dut.experiment.session.event_valid;
    end
    integer progress_ticks=0;
    always @(posedge clk)begin
        progress_ticks=progress_ticks+1;
        if(progress_ticks%1000000==0)$display("progress ticks %0d bridge %0d owner %0d frame %0d controls %0d records %0d latch %0d reason %0d scheduler %0d event %0d ready %0d",progress_ticks,dut.cstate,dut.experiment_owner,dut.experiment_frame,controls,records,dut.safety_latched,dut.safety_reason,dut.experiment.session.transactions.scheduler.state,dut.experiment.session.event_valid,dut.experiment.session.event_ready);
    end
    initial begin repeat(40000000)@(negedge clk);$fatal(1,"global test timeout case %0d state %0d owner %0d latch %0d reason %0d logs %0d",scenario,dut.cstate,dut.experiment_owner,dut.safety_latched,dut.safety_reason,records);end
endmodule

`end_keywords
