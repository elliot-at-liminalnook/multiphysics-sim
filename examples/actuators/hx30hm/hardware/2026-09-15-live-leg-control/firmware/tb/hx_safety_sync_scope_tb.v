`timescale 1ns/1ps
module hx_safety_sync_scope_tb;
    reg [511:0] packet=0;
    reg [6:0] length=0;
    wire allowed;
    integer width,count,i,total;
    hx_safety dut(.clk(1'b0),.rst(1'b1),.host_packet(packet),.host_length(length),
        .host_checksum(8'd255),.allow_forward(allowed));
    initial begin
        for(width=1;width<=2;width=width+1)begin
            for(count=1;count<=(width==1 ? 28 : 18);count=count+1)begin
                total=8+count*(width+1);packet=0;length=total;
                packet[15:0]=16'hffff;packet[23:16]=254;packet[31:24]=total-4;
                packet[39:32]=8'h83;packet[47:40]=width==1 ? 8'h28 : 8'h2c;packet[55:48]=width;
                for(i=0;i<count;i=i+1)packet[(7+i*(width+1))*8 +: 8]=4+i%9;
                #1;
                if(allowed!==(count<=9))$fatal(1,"SYNC scope width %0d count %0d",width,count);
            end
        end
        $display("PASS SYNC scope: all nine supported, every over-nine zero/torque batch rejected including repeated known IDs");
        $finish;
    end
endmodule
