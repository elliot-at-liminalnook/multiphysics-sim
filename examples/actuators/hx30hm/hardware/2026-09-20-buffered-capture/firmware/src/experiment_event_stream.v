// Keeps start/transaction/terminal records ordered through host backpressure.
// start is the SAME accepted pulse delivered to experiment_transactions; its
// start_ticks input is the device clock at that edge. No command or lease logic.
module experiment_event_stream(
    input wire clk,rst,start,
    output wire start_ready,
    input wire [31:0] run_id,plan_crc,period,
    input wire [8:0] mask,frames,
    input wire [63:0] start_ticks,
    input wire event_valid,
    output wire event_ready,
    input wire [1:0] event_kind,
    input wire event_ok,
    input wire [7:0] event_id,event_sequence,event_error,
    input wire [8:0] event_frame,
    input wire [63:0] event_request_ticks,event_completion_ticks,
    input wire [143:0] event_data,
    input wire [5:0] event_width,
    input wire terminal,
    input wire [7:0] result,
    input wire [8:0] terminal_frame,
    input wire [63:0] stop_ticks,stop_pair_ticks,
    input wire interrupted_request,
    input wire [1:0] interrupted_kind,
    input wire [7:0] interrupted_id,
    input wire [63:0] interrupted_request_ticks,
    output wire packet_valid,
    input wire packet_ready,
    output wire [511:0] packet,
    output wire [6:0] packet_length,
    output wire busy,
    output wire fault
);
    reg open=0,start_pending=0,terminal_pending=0,sending_terminal=0;
    reg ownership_fault=0;
    reg [31:0] saved_run=0;
    reg [63:0] saved_start=0,saved_done=0;
    reg [127:0] start_data=0;
    reg [215:0] terminal_data=0;
    reg [8:0] saved_frame=0;
    reg [7:0] saved_result=0;
    wire encoder_ready,encoder_fault;
    wire select_event=!start_pending && event_valid && !sending_terminal;
    wire select_terminal=!start_pending && !event_valid && terminal_pending;
    wire record_valid=open && (start_pending || select_event || select_terminal);
    wire [2:0] kind=start_pending ? 0 : select_event ? {1'b0,event_kind}+3'd1 : 4;
    wire [8:0] frame=start_pending ? 0 : select_event ? event_frame : saved_frame;
    wire [7:0] id=select_event ? event_id : 254;
    wire [7:0] outcome=start_pending ? 0 : select_event ? (event_ok ? 0 : 1) : saved_result;
    wire [7:0] sequence=select_event ? event_sequence : 0;
    wire [63:0] request=select_event ? event_request_ticks : saved_start;
    wire [63:0] completion=start_pending ? saved_start : select_event ? event_completion_ticks : saved_done;
    wire [5:0] reported=start_pending ? 16 : select_event ? event_width : 27;
    wire [4:0] stored=start_pending ? 16 : select_event ? (event_kind==2 && event_width>15 ? 15 : event_width[4:0]) : 27;
    wire [215:0] data=start_pending ? {88'd0,start_data} : select_event ? {72'd0,event_data} : terminal_data;
    assign start_ready=!open && encoder_ready;
    assign busy=open;
    assign event_ready=open && !start_pending && !sending_terminal && encoder_ready;
    assign fault=ownership_fault || encoder_fault;
    always @(posedge clk) begin
        ownership_fault<=0;
        if(rst) begin open<=0;start_pending<=0;terminal_pending<=0;sending_terminal<=0;end
        else begin
            if(start) begin
                if(!start_ready) ownership_fault<=1;
                else begin
                    open<=1;start_pending<=1;saved_run<=run_id;saved_start<=start_ticks;
                    start_data<={32'd50000000,period,7'd0,frames,7'd0,mask,plan_crc};
                end
            end
            if(terminal) begin
                if(!open || terminal_pending || sending_terminal) ownership_fault<=1;
                else begin
                    terminal_pending<=1;saved_frame<=terminal_frame;saved_result<=result;saved_done<=stop_pair_ticks;
                    terminal_data<={7'd0,interrupted_request,
                        (interrupted_request ? interrupted_id : 8'd0),
                        (interrupted_request ? {6'd0,interrupted_kind} : 8'd0),
                        (interrupted_request ? interrupted_request_ticks : 64'd0),stop_pair_ticks,stop_ticks};
                end
            end
            if(event_valid && (!open || sending_terminal || event_kind==3)) ownership_fault<=1;
            if(record_valid && encoder_ready) begin
                if(start_pending) start_pending<=0;
                else if(select_terminal) begin terminal_pending<=0;sending_terminal<=1;end
            end
            if(packet_valid && packet_ready && sending_terminal) begin sending_terminal<=0;open<=0;end
        end
    end
    experiment_event_packet encoder(.clk(clk),.rst(rst),.record_valid(record_valid),.record_ready(encoder_ready),
        .kind(kind),.run_id(saved_run),.frame(frame),.motor_id(id),.outcome(outcome),.sequence(sequence),
        .device_error(select_event ? event_error : 8'd0),.request_ticks(request),.completion_ticks(completion),
        .reported_width(reported),.stored_width(stored),.data(data),.packet_valid(packet_valid),
        .packet_ready(packet_ready),.packet(packet),.packet_length(packet_length),.fault(encoder_fault));
endmodule
