`timescale 1ns/1ps
module capture_tb;
    reg clk=0; always #10 clk=~clk;
    reg rst=1, request=0, good=0, bad=0, timeout=0;
    reg [7:0] id=2,instruction=2;
    reg [63:0] req=64'h0238;
    reg [3:0] req_len=2, reply_len=2;
    wire tx;
    capture_stream #(.HOST_CLKS_PER_BIT(5),.FIFO_BITS(2)) dut(
        .clk(clk),.rst(rst),.request(request),.id(id),.instruction(instruction),
        .request_length(req_len),.request_params(req),.control_flags(8'h12),
        .reply_valid(good),.reply_bad(bad),.reply_timeout(timeout),
        .error_byte(8'd0),.reply_length(reply_len),.reply_params(64'hxxxxxxxxxxxx1234),.tx(tx));
    wire [7:0] byte_in; wire byte_valid;
    uart_rx #(.CLKS_PER_BIT(5)) rx(.clk(clk),.rst(rst),.rx(tx),.enable(1'b1),.data(byte_in),.valid(byte_valid));
    wire valid; wire [7:0] seq,op,len,data; wire [5:0] addr; wire we;
    wire [15:0] crc_errors,frame_timeouts;
    link_rx #(.IDLE_TIMEOUT(5000)) parser(.clk(clk),.rst(rst),.rx_data(byte_in),.rx_valid(byte_valid),
        .frame_valid(valid),.seq(seq),.op(op),.plen(len),.pay_data(data),.pay_addr(addr),.pay_we(we),
        .crc_errors(crc_errors),.timeouts(frame_timeouts));
    reg [7:0] bytes [0:63];
    integer fd, frames=0, errors=0, i;
    reg saw_bad=0,saw_timeout=0,saw_drop=0;
    reg [31:0] last_id=0, last_dropped=0;
    always @(posedge clk) begin
        if(byte_valid) $fwrite(fd,"%c",byte_in);
        if(we) bytes[addr]<=data;
        if(valid) begin
            frames=frames+1;
            if(op!==8'h83 || len!==56 || bytes[0]!==1 || bytes[2]!==2 ||
                bytes[6]!==8'h12)
                $fatal(1,"capture header/request mismatch");
            if(bytes[3]==2 && (bytes[32]!==8'h38 || bytes[33]!==2 || bytes[4]!==2))
                $fatal(1,"read request mismatch");
            if(bytes[3]==3 && (bytes[32]!==8'h2e || {bytes[34],bytes[33]}!==16'h8001 || bytes[4]!==3 || bytes[5]!==0))
                $fatal(1,"write request/ack mismatch");
            if({bytes[52],bytes[51],bytes[50],bytes[49]}!==32'd50_000_000)
                $fatal(1,"capture clock units mismatch");
            if({bytes[31],bytes[30],bytes[29],bytes[28],bytes[27],bytes[26],bytes[25],bytes[24]}
                <= {bytes[23],bytes[22],bytes[21],bytes[20],bytes[19],bytes[18],bytes[17],bytes[16]})
                $fatal(1,"acquisition window is not ordered");
            if(bytes[1]==0 && bytes[3]==2 && ({bytes[41],bytes[40]}!==16'h1234 || bytes[5]!==2))
                $fatal(1,"valid reply payload mismatch");
            if(bytes[1]!=0 && (bytes[5]!==0 || bytes[40]!==0))
                $fatal(1,"failed transaction published stale values");
            if(bytes[1]==1)saw_bad=1;
            if(bytes[1]==2)saw_timeout=1;
            last_id={bytes[11],bytes[10],bytes[9],bytes[8]};
            last_dropped={bytes[15],bytes[14],bytes[13],bytes[12]};
            if(last_dropped>0)saw_drop=1;
        end
    end
    task transact;
        input integer result;
        begin
            @(negedge clk);request=1;
            @(negedge clk);request=0;
            repeat(20)@(negedge clk);
            case(result) 0:good=1;1:bad=1;2:timeout=1;endcase
            @(negedge clk);good=0;bad=0;timeout=0;
        end
    endtask
    initial begin
        fd=$fopen("impl/capture-fixture.bin","wb");
        repeat(5)@(negedge clk);rst=0;
        transact(0); #100000;
        instruction=3;req=64'h80012e;req_len=3;reply_len=0;
        transact(0); #100000;
        instruction=2;req=64'h0238;req_len=2;reply_len=2;
        transact(1); #100000;
        transact(2); #100000;
        for(i=0;i<30;i=i+1)transact(0);
        #1000000;transact(0);#200000;
        $fclose(fd);
        if(!saw_bad || !saw_timeout || !saw_drop || crc_errors!=0 || frame_timeouts!=0)
            $fatal(1,"missing errors/overflow evidence or bad framing");
        if(frames + last_dropped != last_id+1)
            $fatal(1,"drop count does not explain missing transactions");
        $display("PASS: timestamped UART capture, errors and overflow (%0d frames, %0d dropped)",frames,last_dropped);
        $finish;
    end
    initial begin #3000000;$fatal(1,"capture test timeout");end
endmodule
