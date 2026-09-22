`timescale 1ns/1ps
module experiment_scheduler_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,start=0,cancel=0,plan_valid=1,latched=0,row_ready=1,event_ready=1,stop_done=0,adapter_fault=0;
    reg [8:0] mask=511,frames=3,armed=511,fresh=511;
    reg [31:0] period=400;
    reg [71:0] sequences=0;
    wire [8:0] frame;
    wire request,control,active,stop,terminal,event_valid,event_control,event_ok;
    wire [1:0] kind,event_kind;
    wire [7:0] id,event_id,event_sequence,result;
    wire [8:0] event_frame;
    wire interrupted;
    wire [1:0] interrupted_kind;
    wire [7:0] interrupted_id;
    wire [63:0] interrupted_ticks,stop_ticks,stop_pair_ticks;
    wire [63:0] req_time,done_time,ticks,start_ticks;
    reg wire_done=0,audit_valid=0,audit_ok=1;
    reg [7:0] audit_id=0;
    reg allow=1,busy=0,missing_feedback=0,bad_audit=0,wrong_audit_id=0;
    reg [1:0] pending_kind=0;
    reg [7:0] pending_id=0;
    integer delay_count=0,requests=0,controls=0,events=0,audits=0,read_events=0,control_events=0;
    reg [8:0] reads_seen[0:2],audits_seen[0:2];
    reg [2:0] controls_seen=0;
    integer j;
    wire ready=allow&&!busy;
    experiment_scheduler #(.MIN_PERIOD(180),.MAX_PERIOD(600),.MAX_DURATION(4000)) dut(
        .clk(clk),.rst(rst),.start(start),.cancel(cancel),.configured_mask(mask),
        .configured_frames(frames),.configured_period(period),.plan_valid(plan_valid),
        .supervisor_latched(latched),.adapter_fault(adapter_fault),.armed_mask(armed),.fresh_mask(fresh),.sample_sequences(sequences),
        .frame_index(frame),.row_ready(row_ready),.request_valid(request),.request_control(control),
        .request_kind(kind),.request_id(id),.request_ready(ready),.control_wire_done(wire_done),
        .audit_reply_valid(audit_valid),.audit_reply_ok(audit_ok),.audit_reply_id(audit_id),
        .event_ready(event_ready),.event_valid(event_valid),.event_control(event_control),
        .event_kind(event_kind),.event_ok(event_ok),.event_id(event_id),.event_frame(event_frame),
        .event_request_ticks(req_time),.event_completion_ticks(done_time),.event_sample_sequence(event_sequence),
        .stop_required(stop),.stop_pair_wire_done(stop_done),.active(active),.terminal(terminal),
        .result(result),.start_ticks(start_ticks),.ticks(ticks),.stop_ticks(stop_ticks),.stop_pair_ticks(stop_pair_ticks),
        .interrupted_request(interrupted),.interrupted_kind(interrupted_kind),.interrupted_id(interrupted_id),.interrupted_request_ticks(interrupted_ticks));
    always @(posedge clk) begin
        wire_done<=0;audit_valid<=0;
        if(rst) begin busy<=0;sequences<=0;requests<=0;controls<=0;delay_count<=0;end
        else begin
            if(request && ready) begin
                busy<=1;pending_kind<=kind;pending_id<=id;delay_count<=2+(id%3);
                requests<=requests+1;if(control) controls<=controls+1;
            end else if(busy) begin
                if(delay_count!=0) delay_count<=delay_count-1;
                else begin
                    busy<=0;
                    if(pending_kind==0 && !missing_feedback)
                        sequences[(pending_id-4)*8 +: 8]<=sequences[(pending_id-4)*8 +: 8]+1;
                    if(pending_kind==1) wire_done<=1;
                    if(pending_kind==2) begin audit_valid<=1;audit_ok<=!bad_audit;
                        audit_id<=wrong_audit_id ? 99 : pending_id;end
                end
            end
        end
    end
    always @(negedge clk) begin
        if(rst) begin events=0;audits=0;read_events=0;control_events=0;controls_seen=0;
            for(j=0;j<3;j=j+1) begin reads_seen[j]=0;audits_seen[j]=0;end
        end
        else if(event_valid && event_ready) begin
            if(done_time<req_time || done_time>=start_ticks+(event_frame+1)*period)
                $fatal(1,"event outside its device-clock frame");
            if(req_time<start_ticks+event_frame*period) $fatal(1,"early/catch-up event");
            if(event_frame>=frames || event_frame>=3) $fatal(1,"wrong event frame identity");
            if(event_kind==0) begin
                if(event_id<4 || event_id>12 || !mask[event_id-4] || reads_seen[event_frame][event_id-4] || controls_seen[event_frame]) $fatal(1,"duplicate, wrong or late telemetry");
                reads_seen[event_frame][event_id-4]=1;
            end
            if(event_kind==1) begin
                if(reads_seen[event_frame]!=mask || controls_seen[event_frame]) $fatal(1,"control before all fresh telemetry or duplicate control");
                controls_seen[event_frame]=1;
            end
            if(event_kind==2) begin
                if(!controls_seen[event_frame] || event_id<4 || event_id>12 || !mask[event_id-4] || audits_seen[event_frame][event_id-4]) $fatal(1,"wrong, duplicate or premature audit");
                audits_seen[event_frame][event_id-4]=1;
            end
            events=events+1;
            if(event_kind==0) read_events=read_events+1;
            if(event_kind==1) control_events=control_events+1;
            if(event_kind==2) audits=audits+1;
        end
    end
    task reset_case;
        begin
            @(negedge clk);rst=1;start=0;cancel=0;latched=0;armed=511;fresh=511;adapter_fault=0;
            mask=511;frames=3;period=400;row_ready=1;plan_valid=1;event_ready=1;
            stop_done=0;allow=1;missing_feedback=0;bad_audit=0;wrong_audit_id=0;
            repeat(4) @(negedge clk);rst=0;
        end
    endtask
    task launch;
        begin @(negedge clk);start=1;@(negedge clk);start=0;end
    endtask
    task expect_stop;
        input [7:0] expected;
        integer n,saved,f;
        begin
            n=0;while(!stop && n<5000) begin @(negedge clk);n=n+1;end
            if(!stop || result!=expected) $fatal(1,"expected stop %0d, got %0d",expected,result);
            if(expected==3 && stop_ticks!=start_ticks+(frame+1)*period) $fatal(1,"deadline moved with transport latency");
            if(expected==0 && stop_ticks!=start_ticks+frames*period) $fatal(1,"completed duration drifted from device clock");
            if(expected==0) for(f=0;f<frames;f=f+1)
                if(reads_seen[f]!=mask || audits_seen[f]!=mask || !controls_seen[f]) $fatal(1,"terminal omitted a selected motor or frame");
            saved=requests;repeat(30) @(negedge clk);
            if(requests!=saved || request || active || terminal) $fatal(1,"motion request or premature terminal while stop unconfirmed");
            stop_done=1;@(negedge clk);stop_done=0;
            if(stop_pair_ticks<=stop_ticks) $fatal(1,"stop transmission was not separately timestamped");
            if(!terminal || stop) $fatal(1,"terminal requires completed stop-pair transmission");
        end
    endtask
    initial begin
        reset_case;launch;expect_stop(0);
        if(controls!=3 || read_events!=27 || control_events!=3 || audits!=27 || events!=57)
            $fatal(1,"nine-axis frame incomplete: controls %0d, reads %0d, audits %0d",controls,read_events,audits);
        $display("PASS 1: three exact-period frames, nine fresh reads and nine torque/PWM audits each");
        reset_case;mask=9'b100000001;frames=2;launch;expect_stop(0);
        if(controls!=2 || read_events!=4 || audits!=4) $fatal(1,"sparse mask");
        $display("PASS 2: sparse IDs 4 and 12 only");
        reset_case;allow=0;launch;expect_stop(3);if(requests!=0) $fatal(1,"late requests");
        $display("PASS 3: arbitration stall reaches fixed deadline without catch-up");
        reset_case;missing_feedback=1;launch;expect_stop(3);if(controls!=0) $fatal(1,"stale feedback drove controller");
        $display("PASS 4: unchanged sample sequence cannot drive controller");
        reset_case;event_ready=0;launch;expect_stop(5);if(controls!=0) $fatal(1,"lost evidence before drive");
        $display("PASS 5: evidence overflow aborts");
        reset_case;bad_audit=1;launch;expect_stop(7);if(controls!=1) $fatal(1,"audit failure continued drive");
        $display("PASS 6: torque/PWM mismatch stops before another frame");
        reset_case;wrong_audit_id=1;launch;expect_stop(3);
        $display("PASS 7: unrelated audit reply cannot acknowledge a motor");
        reset_case;launch;repeat(20) @(negedge clk);latched=1;expect_stop(2);
        $display("PASS 8: independent supervisor trip preempts sequencer");
        reset_case;launch;repeat(20) @(negedge clk);armed=0;expect_stop(2);
        $display("PASS 9: lost host lease/arm cannot be renewed by sequencer");
        reset_case;launch;wait(request&&ready);@(negedge clk);@(negedge clk);cancel=1;expect_stop(4);
        if(!interrupted || interrupted_kind!=0 || interrupted_id!=4 || interrupted_ticks>=stop_ticks)
            $fatal(1,"cancel lost its in-flight request evidence");
        $display("PASS 10: cancellation withdraws pending packets");
        reset_case;row_ready=0;launch;expect_stop(3);
        $display("PASS 11: missing trajectory row cannot extend deadline");
        reset_case;period=179;launch;expect_stop(1);
        reset_case;frames=0;launch;expect_stop(1);
        reset_case;frames=20;launch;expect_stop(1);
        reset_case;fresh=0;launch;expect_stop(1);
        $display("PASS 12: invalid cadence/count/duration/freshness rejected");
        reset_case;launch;repeat(20) @(negedge clk);start=1;expect_stop(6);
        repeat(30) @(negedge clk);if(active||request) $fatal(1,"held start rearmed after stop");
        $display("PASS 13: start collision stops; held start cannot restart");
        reset_case;launch;repeat(20) @(negedge clk);plan_valid=0;expect_stop(6);
        $display("PASS 14: revoked trajectory validity stops");
        reset_case;launch;wait(event_valid);@(negedge clk);event_ready=0;expect_stop(5);
        if(!event_valid || controls!=0) $fatal(1,"withdrawn event credit lost evidence or sent drive");
        event_ready=1;repeat(2) @(negedge clk);if(event_valid) $fatal(1,"event could not drain after stop");
        $display("PASS 15: event held through backpressure; next request withheld until consumption");
        reset_case;launch;wait(request&&ready);@(negedge clk);adapter_fault=1;expect_stop(8);
        if(controls!=0) $fatal(1,"adapter error drove control");
        $display("PASS 16: transaction adapter failure stops without another request");
        $display("PASS: experiment scheduler contract");$finish;
    end
    initial begin #2000000;$fatal(1,"test timeout");end
endmodule
