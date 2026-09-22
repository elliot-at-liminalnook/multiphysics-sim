`timescale 1ns/1ps
module experiment_transactions_tb #(parameter LOGGED=0);
    reg clk=0;always #5 clk=~clk;
    reg rst=1,start=0,cancel=0,plan_valid=1,row_ready=1,latched=0,credit=1;
    reg [8:0] mask=511,frames=2,armed=511,fresh=511;
    reg [31:0] period=2500;
    reg [15:0] kp=1536,kd=512,kv=1536,lim=75;
    reg [143:0] targets=0,deltas=0,wire_pwm=0;
    reg [71:0] sequences=0;
    reg servo_valid=0,control_done=0,stop_done=0;
    reg [7:0] servo_data=0;
    wire packet_valid,active,stop,terminal,event_valid,event_ok,locked;
    wire [511:0] packet;
    wire [6:0] length;
    wire [8:0] frame,event_frame;
    wire [1:0] event_kind;
    wire [7:0] event_id,event_sequence,event_error,result;
    wire [143:0] event_data;
    wire [5:0] event_width;
    wire [63:0] req_ticks,done_ticks,start_ticks,stop_ticks,stop_pair_ticks,ticks;
    wire interrupted;
    wire [1:0] interrupted_kind;
    wire [7:0] interrupted_id;
    wire [63:0] interrupted_ticks;
    wire stream_credit,stream_busy,stream_packet_valid,stream_fault;
    wire [511:0] stream_packet;
    wire [6:0] stream_length;
    wire core_credit=credit && (!LOGGED || stream_credit);
    reg busy=0,allow=1,drop_raw=0,drop_sequence=0,bad_audit=0,bad_telemetry=0;
    wire ready=!busy && allow;
    experiment_transactions #(.MIN_PERIOD(500),.MAX_PERIOD(3000),.MAX_DURATION(12000),.RX_TIMEOUT(60)) dut(
        .clk(clk),.rst(rst),.start(start),.cancel(cancel),.configured_mask(mask),.configured_frames(frames),
        .configured_period(period),.kp(kp),.kd(kd),.kv(kv),.duty_limit(lim),.plan_valid(plan_valid),
        .row_ready(row_ready),.targets(targets),.deltas(deltas),.frame_index(frame),.bank_locked(locked),
        .supervisor_latched(latched),.armed_mask(armed),.fresh_mask(fresh),.sample_sequences(sequences),
        .packet_valid(packet_valid),.packet_ready(ready),.packet(packet),.packet_length(length),
        .servo_valid(servo_valid),.servo_data(servo_data),.control_wire_done(control_done),.transmitted_pwm(wire_pwm),
        .event_valid(event_valid),.event_ready(core_credit),.event_kind(event_kind),.event_ok(event_ok),
        .event_id(event_id),.event_sample_sequence(event_sequence),.event_frame(event_frame),
        .event_request_ticks(req_ticks),.event_completion_ticks(done_ticks),.event_data(event_data),
        .event_width(event_width),.event_error(event_error),.stop_required(stop),.stop_pair_wire_done(stop_done),
        .active(active),.terminal(terminal),.result(result),.start_ticks(start_ticks),.stop_ticks(stop_ticks),
        .stop_pair_ticks(stop_pair_ticks),.ticks(ticks),.interrupted_request(interrupted),.interrupted_kind(interrupted_kind),
        .interrupted_id(interrupted_id),.interrupted_request_ticks(interrupted_ticks));
    generate if(LOGGED) begin: logged
        experiment_event_stream stream(.clk(clk),.rst(rst),.start(start),.start_ready(),
            .run_id(32'd17),.plan_crc(32'h98c3f9fc),.period(period),.mask(mask),.frames(frames),.start_ticks(ticks),
            .event_valid(event_valid && credit),.event_ready(stream_credit),.event_kind(event_kind),.event_ok(event_ok),
            .event_id(event_id),.event_sequence(event_sequence),.event_error(event_error),.event_frame(event_frame),
            .event_request_ticks(req_ticks),.event_completion_ticks(done_ticks),.event_data(event_data),.event_width(event_width),
            .terminal(terminal),.result(result),.terminal_frame(frame),.stop_ticks(stop_ticks),.stop_pair_ticks(stop_pair_ticks),
            .interrupted_request(interrupted),.interrupted_kind(interrupted_kind),.interrupted_id(interrupted_id),
            .interrupted_request_ticks(interrupted_ticks),.packet_valid(stream_packet_valid),.packet_ready(1'b1),
            .packet(stream_packet),.packet_length(stream_length),.busy(stream_busy),.fault(stream_fault));
    end else begin
        assign stream_credit=1;assign stream_busy=0;assign stream_packet_valid=0;assign stream_packet=0;assign stream_length=0;assign stream_fault=0;
    end endgenerate
    reg [7:0] reply[0:20],sum,accepted_id;
    reg [1:0] accepted_kind;
    integer cursor=0,size=0,delay_count=0,i,n,groups=0,packets=0,controls=0,events=0,reads=0,audits=0;
    integer streamed=0;
    always @(posedge clk) begin
        if(rst) streamed=0;
        else begin
            if(stream_fault) $fatal(1,"integrated stream fault");
            if(stream_packet_valid) begin
                if(streamed==0 && stream_packet[63:56]!=0) $fatal(1,"stream missing START");
                streamed=streamed+1;
            end
        end
    end
    reg [8:0] read_masks[0:1],audit_masks[0:1];
    reg [143:0] saved_event;
    // The bridge model accepts whole packets, then returns raw bytes and an
    // independent supervisor sample sequence. It does not implement motor physics.
    always @(posedge clk) begin
        control_done<=0;servo_valid<=0;
        if(rst) begin
            busy<=0;sequences<=0;packets=0;controls=0;cursor=0;delay_count=0;
        end else if(packet_valid && ready) begin
            busy<=1;packets=packets+1;accepted_id=packet[23:16];
            sum=0;for(n=2;n<length;n=n+1) sum=sum+packet[n*8 +: 8];
            if(sum!=255 || packet[15:0]!=16'hffff || packet[31:24]!=length-4) $fatal(1,"invalid emitted packet");
            if(packet[39:32]==8'ha1) begin
                if(length!=52 || accepted_id!=254 || packet[55:40]!=mask
                    || packet[71:56]!=1536 || packet[87:72]!=512 || packet[103:88]!=1536 || packet[119:104]!=75)
                    $fatal(1,"controller packet lost frozen settings");
                accepted_kind=1;controls=controls+1;delay_count=20;
                // Distinct signed register words expose wrong-axis audit mapping.
                for(n=0;n<9;n=n+1) wire_pwm[n*16 +: 16]<=mask[n] ? 25+n+(n%2 ? 1024 : 0) : 0;
            end else begin
                if(length!=8 || packet[39:32]!=2 || accepted_id<4 || accepted_id>12 || !mask[accepted_id-4])
                    $fatal(1,"invalid read identity");
                accepted_kind=packet[47:40]==8'h38 ? 0 : 2;
                size=accepted_kind==0 ? 21 : 12;
                if((accepted_kind==0 && packet[55:40]!=16'h0f38) || (accepted_kind==2 && packet[55:40]!=16'h0628)) $fatal(1,"read address/width");
                for(n=0;n<21;n=n+1) reply[n]=0;
                reply[0]=255;reply[1]=255;reply[2]=accepted_id;reply[3]=size-4;
                if(accepted_kind==0) begin
                    reply[5]=accepted_id;reply[6]=5;reply[11]=121;reply[12]=51;reply[18]=26;
                    if(bad_telemetry) reply[4]=8;
                end else begin
                    reply[5]=1;reply[9]=wire_pwm[(accepted_id-4)*16 +: 8];
                    reply[10]=wire_pwm[(accepted_id-4)*16+8 +: 8];
                    if(bad_audit) reply[9]=reply[9]+1;
                end
                sum=0;for(n=2;n<size-1;n=n+1) sum=sum+reply[n];reply[size-1]=~sum;
                cursor=0;delay_count=2;
            end
        end else if(busy) begin
            if(delay_count>0) delay_count=delay_count-1;
            else if(accepted_kind==1) begin busy<=0;control_done<=1;end
            else if(cursor<size) begin
                servo_valid<=!(drop_raw && accepted_kind==0);servo_data<=reply[cursor];
                cursor=cursor+1;delay_count=1;
            end else begin
                if(accepted_kind==0 && !drop_sequence) sequences[(accepted_id-4)*8 +: 8]<=sequences[(accepted_id-4)*8 +: 8]+1;
                busy<=0;
            end
        end
    end
    always @(negedge clk) begin
        if(rst) begin
            events=0;reads=0;audits=0;
            for(i=0;i<2;i=i+1) begin read_masks[i]=0;audit_masks[i]=0;end
        end else if(event_valid && core_credit) begin
            if(event_frame>=frames || done_ticks<=req_ticks || req_ticks<start_ticks+event_frame*period || done_ticks>=start_ticks+(event_frame+1)*period)
                $fatal(1,"invalid event timing/identity");
            if(event_kind==0) begin
                if(event_data[15:0]!=1280+event_id || event_data[55:48]!=121 || event_data[63:56]!=51
                    || event_data[119:104]!=26 || event_width!=15 || event_sequence!=event_frame+1 || event_error!=0)
                    $fatal(1,"raw telemetry not paired with correct supervisor sample");
                if(read_masks[event_frame][event_id-4]) $fatal(1,"duplicate telemetry");
                read_masks[event_frame][event_id-4]=1;reads=reads+1;
            end else if(event_kind==1) begin
                if(event_width!=18 || event_data!=wire_pwm || read_masks[event_frame]!=mask) $fatal(1,"control data differs from final transmitted batch");
            end else if(event_kind==2) begin
                if(event_width!=6 || event_data[7:0]!=1 || event_data[47:32]!=(wire_pwm[(event_id-4)*16 +: 16]+(bad_audit ? 1 : 0))) $fatal(1,"audit payload lost");
                if(event_ok==bad_audit || audit_masks[event_frame][event_id-4]) $fatal(1,"audit outcome/duplicate");
                audit_masks[event_frame][event_id-4]=1;audits=audits+1;
            end
            events=events+1;
        end
    end
    task reset_case;
        begin
            @(negedge clk);rst=1;start=0;cancel=0;latched=0;credit=1;allow=1;
            plan_valid=1;row_ready=1;mask=511;frames=2;armed=511;fresh=511;
            kp=1536;kd=512;kv=1536;lim=75;drop_raw=0;drop_sequence=0;bad_audit=0;bad_telemetry=0;stop_done=0;
            repeat(4) @(negedge clk);rst=0;
        end
    endtask
    task launch;
        begin @(negedge clk);start=1;@(negedge clk);start=0;end
    endtask
    task finish_stop;
        input [7:0] expected;
        integer count,saved;
        begin
            count=0;while(!stop && count<6500) begin @(negedge clk);count=count+1;end
            if(!stop || result!=expected || !locked) $fatal(1,"stop result expected %0d got %0d",expected,result);
            if(expected==0 && stop_ticks!=start_ticks+period*frames) $fatal(1,"frame schedule drift");
            saved=packets;repeat(50) @(negedge clk);
            if(packet_valid || packets!=saved || terminal) $fatal(1,"packet after stop or premature terminal");
            stop_done=1;@(negedge clk);stop_done=0;
            if(!terminal || stop_pair_ticks<=stop_ticks) $fatal(1,"missing stop wire completion");
            if(LOGGED && credit) begin
                count=0;
                while(stream_busy && count<250) begin @(negedge clk);count=count+1;end
                if(stream_busy || streamed!=events+2) $fatal(1,"stream omitted start/event/terminal records");
            end
        end
    endtask
    initial begin
        reset_case;launch;
        // Changes to live gain controls after START cannot alter the run.
        kp=1;kd=2;kv=3;lim=4;
        finish_stop(0);
        if(events!=38 || reads!=18 || audits!=18 || controls!=2 || packets!=38
            || read_masks[0]!=511 || read_masks[1]!=511 || audit_masks[0]!=511 || audit_masks[1]!=511) $fatal(1,"incomplete nine-motor run");groups=groups+1;
        reset_case;mask=257;launch;finish_stop(0);
        if(reads!=4 || audits!=4 || controls!=2 || events!=10) $fatal(1,"sparse motor run");groups=groups+1;
        reset_case;bad_audit=1;launch;finish_stop(7);
        if(controls!=1 || audits!=1) $fatal(1,"bad audit did not stop");groups=groups+1;
        reset_case;drop_raw=1;launch;finish_stop(3);
        if(controls!=0 || events!=0) $fatal(1,"supervisor sequence without raw evidence used");groups=groups+1;
        reset_case;drop_sequence=1;launch;finish_stop(3);
        if(controls!=0 || events!=0) $fatal(1,"raw reply without supervisor acceptance used");groups=groups+1;
        reset_case;launch;wait(event_valid);@(negedge clk);credit=0;saved_event=event_data;
        finish_stop(5);
        if(!event_valid || event_data!=saved_event || !locked || controls!=0) $fatal(1,"backpressure lost raw event");
        credit=1;repeat(3) @(negedge clk);if(event_valid || locked) $fatal(1,"event did not drain");groups=groups+1;
        reset_case;allow=0;launch;repeat(15) @(negedge clk);cancel=1;finish_stop(4);
        if(packets!=0) $fatal(1,"cancelled packet escaped");groups=groups+1;
        reset_case;credit=0;launch;finish_stop(3);
        if(packets!=0) $fatal(1,"unreserved logger allowed bus transaction");groups=groups+1;
        reset_case;bad_telemetry=1;launch;finish_stop(8);
        if(controls!=0 || events!=0 || packets!=1) $fatal(1,"invalid raw feedback advanced controller");groups=groups+1;
        $display("PASS experiment transactions: %0d groups, complete/sparse nine-axis runs, raw/supervisor association, final-wire PWM audits, frozen gains, backpressure/cancel",groups);$finish;
    end
    initial begin repeat(65000) @(negedge clk);$fatal(1,"transaction test timeout");end
endmodule
