`timescale 1ns/1ps
// Exhaust every on-wire ID to falsify aliasing after narrowing array addresses.
module hx_safety_index_tb;
    hx_safety dut(.clk(1'b0),.rst(1'b1));
    integer id,axis;
    reg [8:0] mask;
    initial begin
        for(axis=0;axis<9;axis=axis+1)begin
            mask=1<<axis;
            for(id=0;id<256;id=id+1)begin
                if(dut.write_ok(0,mask,id,8'h2c,2,0,0)!==(id==axis+4))
                    $fatal(1,"positive PWM ID alias: ID %0d armed %0d",id,axis+4);
                if(dut.write_ok(1,mask,id,8'h2c,2,0,0)!==0)
                    $fatal(1,"latched positive PWM accepted");
                if(dut.write_ok(0,mask,id,8'h28,1,0,1)!==(id==axis+4))
                    $fatal(1,"torque enable ID alias");
                if(dut.write_ok(1,mask,id,8'h2c,2,1,0)!==((id>=4 && id<=12)||id==254))
                    $fatal(1,"zero command ID scope changed");
            end
        end
        $display("PASS narrowed address isolation: all 256 IDs, each armed axis, positive/zero PWM, torque enable and latched state");
        $finish;
    end
endmodule
