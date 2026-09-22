// Observation only. Included inside the original complete UART fixture.
// Exclusive classification follows downstream admission gate priority. These
// counters describe a synthetic run, not internal signals from physical captures.
`define SCH dut.experiment.session.transactions.scheduler
integer gap_fd, gap_guard=0, gap_copy=0, gap_local=0, gap_encode=0;
integer gap_fifo=0, gap_host=0, gap_other=0;
reg gap_tracking=0, gap_last_event=0;
reg [63:0] gap_end=0;
initial begin
    gap_fd=$fopen("gap-trace.csv","w");
    $fdisplay(gap_fd,"frame,next_kind,next_id,gap_ticks,guard_ticks,log_copy_ticks,local_ticks,encode_ticks,fifo_ticks,host_ticks,other_ticks,boundary_ticks");
end
always @(posedge clk) begin
    if (`SCH.event_valid && !gap_last_event) begin
        $display("EVENT %0d %0d %0d %0d %0d", `SCH.event_frame,
            `SCH.event_kind, `SCH.event_id, `SCH.event_request_ticks,
            `SCH.event_completion_ticks);
        gap_tracking= !(`SCH.event_kind==2 && `SCH.event_id==12);
        gap_end=`SCH.event_completion_ticks;
        gap_guard=0;gap_copy=0;gap_local=0;gap_encode=0;
        gap_fifo=0;gap_host=0;gap_other=0;
    end
    gap_last_event=`SCH.event_valid;
    if(gap_tracking) begin
        if (`SCH.request_valid && `SCH.request_ready) begin
            $fdisplay(gap_fd,"%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d,%0d",
                `SCH.frame_index,`SCH.request_kind,`SCH.request_id,
                `SCH.ticks-gap_end,gap_guard,gap_copy,gap_local,gap_encode,
                gap_fifo,gap_host,gap_other,
                `SCH.ticks-gap_end-gap_guard-gap_copy-gap_local-gap_encode-gap_fifo-gap_host-gap_other);
            $fflush(gap_fd);gap_tracking=0;
        end else if(dut.cstate!=0 || dut.stop_active || dut.begin_stop || dut.s_tx_busy
            || dut.stop_gap!=0 || dut.reply_wait!=0 || dut.rx_quiet<1000 || dut.experiment_owner)
            gap_guard=gap_guard+1;
        else if(dut.local_pending) gap_local=gap_local+1;
        else if(dut.experiment_log_sending) gap_copy=gap_copy+1;
        else if(dut.experiment_log_valid || !dut.experiment.session.event_ready)
            gap_encode=gap_encode+1;
        else if(dut.queued_valid) gap_host=gap_host+1;
        else if(dut.fifo_free<128) gap_fifo=gap_fifo+1;
        else gap_other=gap_other+1;
    end
end
`undef SCH
