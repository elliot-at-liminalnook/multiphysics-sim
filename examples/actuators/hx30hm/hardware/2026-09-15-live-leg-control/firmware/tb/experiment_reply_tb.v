`timescale 1ns/1ps
module experiment_reply_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,cancel=0,accepted=0,valid=0;
    reg [1:0] kind=0;
    reg [7:0] id=4,data=0;
    reg [15:0] pwm=0;
    wire pending,reply_valid,ok,fault;
    wire [1:0] reply_kind;
    wire [7:0] reply_id,error;
    wire [5:0] width;
    wire [119:0] raw;
    experiment_reply #(.RX_TIMEOUT(30)) dut(.clk(clk),.rst(rst),.cancel(cancel),
        .request_accepted(accepted),.request_kind(kind),.request_id(id),.expected_pwm(pwm),
        .servo_valid(valid),.servo_data(data),.pending(pending),.reply_valid(reply_valid),
        .reply_ok(ok),.reply_kind(reply_kind),.reply_id(reply_id),.reply_error(error),
        .reply_width(width),.reply_data(raw),.fault(fault));
    integer replies=0,faults=0,groups=0,i,j,before_replies,before_faults;
    reg [7:0] bytes[0:63];
    reg [7:0] checksum;
    reg [119:0] expected;
    always @(posedge clk) begin
        if(reply_valid) replies=replies+1;
        if(fault) faults=faults+1;
    end
    task byte_in;
        input [7:0] value;
        begin @(negedge clk);data=value;valid=1;@(negedge clk);valid=0;end
    endtask
    task request;
        input [1:0] k;
        input [7:0] motor;
        input [15:0] commanded;
        begin @(negedge clk);kind=k;id=motor;pwm=commanded;accepted=1;
            @(negedge clk);accepted=0;end
    endtask
    task clear;
        begin @(negedge clk);cancel=1;@(negedge clk);cancel=0;end
    endtask
    task frame;
        input [7:0] motor,status;
        input integer count;
        input [119:0] payload;
        input corrupt;
        begin
            bytes[0]=255;bytes[1]=255;bytes[2]=motor;bytes[3]=count+2;bytes[4]=status;
            for(j=0;j<count;j=j+1) bytes[5+j]=j<15 ? payload[j*8 +: 8] : 8'hda;
            checksum=0;for(j=2;j<count+5;j=j+1) checksum=checksum+bytes[j];
            bytes[count+5]=(~checksum)^corrupt;
            for(j=0;j<count+6;j=j+1) byte_in(bytes[j]);
            @(negedge clk);
        end
    endtask
    initial begin
        repeat(3) @(negedge clk);rst=0;
        // Distinct encoder/voltage/current bytes survive unchanged for all IDs.
        for(i=4;i<=12;i=i+1) begin
            expected=0;expected[15:0]=1272+i;expected[55:48]=121;
            expected[63:56]=51;expected[119:104]=26;
            request(0,i,0);frame(i,0,15,expected,0);
            if(pending || !ok || raw!=expected || width!=15 || reply_id!=i || reply_kind!=0 || error!=0) $fatal(1,"telemetry identity/raw bytes lost");
        end
        if(replies!=9 || faults!=0) $fatal(1,"telemetry completion count");groups=groups+1;
        // Exact signed register encoding, including zero and both directions.
        for(i=4;i<=12;i=i+1) begin
            expected=0;expected[7:0]=1;expected[47:32]=(i%2 ? 1024 : 0)+(i==4 ? 0 : 75);
            request(2,i,expected[47:32]);frame(i,0,6,expected,0);
            if(pending || !ok || raw!=expected || width!=6 || reply_kind!=2) $fatal(1,"signed PWM audit mismatch");
        end
        if(replies!=18 || faults!=0) $fatal(1,"audit completion count");groups=groups+1;
        // Foreign, corrupt and unsolicited replies never acknowledge a request.
        before_replies=replies;request(0,4,0);frame(5,0,15,0,0);frame(4,0,15,0,1);
        if(!pending || replies!=before_replies) $fatal(1,"foreign/corrupt reply accepted");
        frame(4,0,15,0,0);frame(4,0,15,0,0);
        if(replies!=before_replies+1) $fatal(1,"duplicate reply accepted");groups=groups+1;
        // Checksum-valid matching failures retain exact width/status/data.
        before_faults=faults;
        request(2,4,75);frame(4,0,6,48'h004a00000001,0);
        if(ok || raw[47:32]!=74) $fatal(1,"wrong PWM accepted or hidden");
        request(2,4,75);frame(4,0,6,48'h004b00000000,0);
        if(ok || raw[7:0]!=0) $fatal(1,"torque off accepted");
        request(2,4,0);frame(4,8,0,0,0);
        if(ok || error!=8 || width!=0 || raw!=0) $fatal(1,"short device error lost");
        request(0,4,0);frame(4,0,6,48'h004b00000001,0);
        if(ok || width!=6) $fatal(1,"configuration reply used as telemetry");
        request(0,4,0);frame(4,0,58,0,0);
        if(ok || width!=58 || raw!=0) $fatal(1,"oversize payload handling");
        if(faults!=before_faults+5) $fatal(1,"matching errors missing");groups=groups+1;
        // A truncated frame cannot swallow the next header after interbyte timeout.
        request(0,4,0);byte_in(255);byte_in(255);byte_in(4);byte_in(17);byte_in(0);byte_in(42);
        repeat(35) @(negedge clk);expected=120'h1234;
        frame(4,0,15,expected,0);
        if(pending || !ok || raw!=expected) $fatal(1,"timeout recovery failed");groups=groups+1;
        // Cancellation and overlapping ownership fail closed.
        before_replies=replies;request(0,4,0);byte_in(255);byte_in(255);clear;frame(4,0,15,0,0);
        if(pending || replies!=before_replies) $fatal(1,"cancelled read acknowledged");
        before_faults=faults;request(0,4,0);request(2,5,0);@(negedge clk);
        if(pending || faults!=before_faults+1) $fatal(1,"overlapping request not rejected");
        request(1,254,0);frame(4,0,0,0,0);
        if(pending || replies!=before_replies) $fatal(1,"control treated as servo ACK");
        request(3,254,0);@(negedge clk);request(0,3,0);@(negedge clk);
        if(pending || faults!=before_faults+3) $fatal(1,"invalid request not rejected");groups=groups+1;
        // Invalid length is discarded without manufacturing a completion.
        request(0,4,0);byte_in(255);byte_in(255);byte_in(4);byte_in(255);
        frame(4,0,15,expected,0);
        if(pending || !ok || raw!=expected) $fatal(1,"bad-length recovery failed");groups=groups+1;
        $display("PASS experiment reply: %0d groups, nine IDs, exact signed PWM/torque audits, raw telemetry, corruption/identity/timeout/cancel",groups);$finish;
    end
    initial begin repeat(10000) @(negedge clk);$fatal(1,"reply test timeout");end
endmodule
