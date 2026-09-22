`timescale 1ns/1ps
module experiment_packet_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,valid=0,ready=0;
    reg [1:0] kind=0;
    reg [7:0] id=4;
    reg [8:0] mask=511;
    reg [15:0] kp=1536,kd=512,kv=1536,lim=75;
    reg [143:0] targets=0,deltas=0;
    wire accepted,packet_valid,fault;
    wire [511:0] packet;
    wire [6:0] length;
    experiment_packet dut(.clk(clk),.rst(rst),.request_valid(valid),.request_kind(kind),.request_id(id),
        .mask(mask),.kp(kp),.kd(kd),.kv(kv),.duty_limit(lim),.targets(targets),.deltas(deltas),
        .request_ready(accepted),.packet_valid(packet_valid),.packet_ready(ready),.packet(packet),.packet_length(length),.fault(fault));
    reg [287:0] rows[0:2];
    reg [511:0] before_stall;
    reg [7:0] sum;
    integer i,j,groups=0,faults=0;
    always @(posedge clk) if(fault) faults=faults+1;
    task drop;
        begin @(negedge clk);valid=0;ready=0;@(negedge clk);end
    endtask
    task check_packet;
        input integer size;
        begin
            wait(packet_valid);@(negedge clk);
            if(length!=size || packet[15:0]!=16'hffff || packet[23:16]!=id || packet[31:24]!=size-4) $fatal(1,"incorrect scheduler packet");
            sum=0;for(j=2;j<size;j=j+1) sum=sum+packet[j*8 +: 8];
            if(sum!=255) $fatal(1,"incorrect scheduler checksum");
            for(j=size;j<64;j=j+1) if(packet[j*8 +: 8]!=0) $fatal(1,"stale trailing packet data");
        end
    endtask
    initial begin
        $readmemh("tb/trajectory-v1/rows.hex",rows);
        repeat(3) @(negedge clk);rst=0;
        kind=1;id=254;
        for(i=0;i<9;i=i+1) begin targets[i*16 +: 16]=rows[2][i*32 +: 16];deltas[i*16 +: 16]=rows[2][i*32+16 +: 16];end
        valid=1;check_packet(52);
        if(packet[39:32]!=8'ha1 || packet[55:40]!=511 || packet[71:56]!=1536 || packet[87:72]!=512 || packet[103:88]!=1536 || packet[119:104]!=75) $fatal(1,"A1 settings differ from Rust");
        if(packet[407:120]!=rows[2]) $fatal(1,"A1 targets/deltas differ from Rust");
        before_stall=packet;repeat(20) begin @(negedge clk);if(packet!=before_stall || !packet_valid || accepted) $fatal(1,"unstable stalled packet");end
        ready=1;#1;if(!accepted) $fatal(1,"missing acceptance");drop;groups=groups+1;
        for(i=4;i<=12;i=i+1) begin
            kind=0;id=i;valid=1;check_packet(8);
            if(packet[55:32]!=24'h0f3802) $fatal(1,"telemetry request wrong");drop;
            kind=2;valid=1;check_packet(8);
            if(packet[55:32]!=24'h062802) $fatal(1,"audit request wrong");drop;
        end
        groups=groups+1;
        kind=1;id=254;valid=1;repeat(10) @(negedge clk);drop;repeat(55) @(negedge clk);
        if(packet_valid || accepted) $fatal(1,"cancelled assembly became available");groups=groups+1;
        kind=0;id=3;valid=1;repeat(10) @(negedge clk);if(faults!=1 || packet_valid) $fatal(1,"foreign motor not rejected");drop;
        mask=1;id=12;valid=1;repeat(10) @(negedge clk);if(faults!=2 || packet_valid) $fatal(1,"unselected motor not rejected");drop;
        kind=3;id=254;valid=1;repeat(10) @(negedge clk);if(faults!=3 || packet_valid) $fatal(1,"unknown transaction not rejected");drop;groups=groups+1;
        $display("PASS experiment packet: %0d groups, Rust A1 parity, nine telemetry/audit addresses, stalls/cancel/rejection",groups);$finish;
    end
    initial begin repeat(5000) @(negedge clk);$fatal(1,"packet builder timeout");end
endmodule
