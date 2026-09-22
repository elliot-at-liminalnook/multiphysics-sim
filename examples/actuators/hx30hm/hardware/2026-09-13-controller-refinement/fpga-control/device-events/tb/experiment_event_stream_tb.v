`timescale 1ns/1ps
module experiment_event_stream_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,start=0,event_valid=0,event_ok=1,terminal=0,ready=0;
    reg [31:0] run_id=17;
    reg [1:0] kind=0;
    reg [7:0] id=4,sequence=1,result=4;
    reg [5:0] width=15;
    reg [143:0] data=0;
    wire start_ready,event_ready,packet_valid,busy,fault;
    wire [511:0] packet;
    wire [6:0] length;
    integer faults=0,groups=0;
    reg [511:0] snapshot;
    experiment_event_stream dut(.clk(clk),.rst(rst),.start(start),.start_ready(start_ready),
        .run_id(run_id),.plan_crc(32'h98c3f9fc),.period(32'd2500000),.mask(9'd511),.frames(9'd3),.start_ticks(64'd100),
        .event_valid(event_valid),.event_ready(event_ready),.event_kind(kind),.event_ok(event_ok),.event_id(id),
        .event_sequence(sequence),.event_error(8'd0),.event_frame(9'd0),.event_request_ticks(64'd200),
        .event_completion_ticks(64'd400),.event_data(data),.event_width(width),.terminal(terminal),.result(result),
        .terminal_frame(9'd0),.stop_ticks(64'd500),.stop_pair_ticks(64'd700),.interrupted_request(1'b0),
        .interrupted_kind(2'd2),.interrupted_id(8'd12),.interrupted_request_ticks(64'd333),
        .packet_valid(packet_valid),.packet_ready(ready),.packet(packet),.packet_length(length),.busy(busy),.fault(fault));
    always @(posedge clk) if(fault) faults=faults+1;
    task reset_case;
        begin @(negedge clk);rst=1;start=0;event_valid=0;terminal=0;ready=0;run_id=17;
            repeat(3) @(negedge clk);rst=0;end
    endtask
    task launch;
        begin if(!start_ready) $fatal(1,"start not ready");@(negedge clk);start=1;@(negedge clk);start=0;end
    endtask
    task consume;
        input [7:0] expected_kind;
        begin
            wait(packet_valid);@(negedge clk);
            if(packet[63:56]!=expected_kind || packet[95:64]!=17) $fatal(1,"record order or run identity lost");
            snapshot=packet;repeat(15) begin @(negedge clk);if(!packet_valid || packet!=snapshot || !busy || start_ready) $fatal(1,"blocked host lost record or unlocked run");end
            ready=1;@(negedge clk);ready=0;
        end
    endtask
    initial begin
        reset_case;launch;run_id=99;
        // A transaction waiting for evidence credit and terminal arrive while
        // START itself is blocked. All three must drain in original order.
        data=144'h1234;event_valid=1;terminal=1;@(negedge clk);terminal=0;
        consume(0);wait(event_ready);@(negedge clk);@(negedge clk);event_valid=0;
        consume(1);wait(packet_valid);@(negedge clk);
        if(length!=64 || packet[511:288]===224'd0) $fatal(1,"terminal missing");
        if(packet[351:288]!=500 || packet[415:352]!=700) $fatal(1,"stop timestamps lost");
        if(packet[503:416]!=0) $fatal(1,"absent interrupted metadata was not canonicalized");
        consume(4);@(negedge clk);if(busy || !start_ready || faults!=0) $fatal(1,"run did not unlock after terminal drain");groups=groups+1;
        reset_case;launch;@(negedge clk);start=1;run_id=88;@(negedge clk);start=0;
        consume(0);if(faults!=1) $fatal(1,"start collision not reported");
        terminal=1;@(negedge clk);terminal=0;consume(4);groups=groups+1;
        reset_case;terminal=1;@(negedge clk);terminal=0;repeat(4) @(negedge clk);
        if(busy || packet_valid || faults!=2) $fatal(1,"orphan terminal not rejected");groups=groups+1;
        $display("PASS event stream: %0d groups; start/event/terminal retention, host backpressure, immutable run identity, terminal drain and ownership rejection",groups);$finish;
    end
    initial begin repeat(2000) @(negedge clk);$fatal(1,"stream timeout");end
endmodule
