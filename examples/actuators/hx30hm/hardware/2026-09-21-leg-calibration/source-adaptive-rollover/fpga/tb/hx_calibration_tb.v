`timescale 1ns/1ps
module hx_calibration_tb;
    reg [15:0] test_position=2048;
    reg clk=0; always #5 clk=~clk;
    reg rst=1,check_host=0,servo_valid=0,stop_accepted=0,emergency_stop=0,bridge_fault=0;
    reg [511:0] host_packet=0;
    reg [6:0] host_length=0;
    reg [7:0] host_checksum=0,servo_data=0;
    wire allow_forward,local_command,latched,stop_request;
    wire [7:0] reason,fault_id;
    wire [103:0] status;
    hx_safety #(.FIRST_ID(1),.COUNT(3),.CALIBRATION(1),.FEEDBACK_TIMEOUT(1000),.COMMAND_TIMEOUT(1500),.RX_TIMEOUT(100),.STOP_REPEAT(200)) dut (
        .clk(clk),.rst(rst),.check_host(check_host),.host_packet(host_packet),.host_length(host_length),
        .host_checksum(host_checksum),.emergency_stop(emergency_stop),.bridge_fault(bridge_fault),
        .allow_forward(allow_forward),.local_command(local_command),.local_status(status),
        .servo_valid(servo_valid),.servo_data(servo_data),.latched(latched),.reason(reason),
        .fault_id(fault_id),.stop_request(stop_request),.stop_accepted(stop_accepted));
    reg [7:0] p[0:63],r[0:20];
    integer i,j,cases=0;
    integer length_case;
    initial begin
        for(length_case=0;length_case<128;length_case=length_case+1) begin
            if(dut.sync_word_length(length_case[6:0]) !== (length_case>=11 && length_case<=64 && (length_case-8)%3==0))
                $fatal(1,"word SYNC length predicate changed at %0d",length_case);
            if((length_case>=10 && length_case<=64 && !length_case[0]) !== (length_case>=10 && length_case<=64 && (length_case-8)%2==0))
                $fatal(1,"byte SYNC length predicate changed at %0d",length_case);
        end
        $display("PASS all 128 SYNC word/byte length predicates match previous arithmetic");
    end
    task reset;
        begin
            @(negedge clk); rst=1; check_host=0;servo_valid=0;emergency_stop=0;bridge_fault=0;
            repeat(4) @(negedge clk);rst=0;repeat(4) @(negedge clk);
            if(!latched || !stop_request) $fatal(1,"startup must be latched with stop pending");
        end
    endtask
    task prepare;
        input integer length;
        input integer corrupt;
        reg [7:0] sum;
        integer n;
        begin
            sum=0;for(n=2;n<length-1;n=n+1) sum=sum+p[n];
            p[length-1]=~sum; if(corrupt) p[length-1]=p[length-1]^1;
            host_packet=0;host_checksum=0;host_length=length;
            for(n=0;n<length;n=n+1) begin
                host_packet[8*n +: 8]=p[n];
                if(n>=2)host_checksum=host_checksum+p[n];
            end
            #1;
        end
    endtask
    task send;
        begin @(negedge clk);check_host=1;@(negedge clk);check_host=0;repeat(3)@(negedge clk);end
    endtask
    task local;
        input [7:0] op,id;
        begin
            p[0]=255;p[1]=255;p[2]=254;p[3]=(op==0 || op==2)?3:4;p[4]=8'ha0;p[5]=op;p[6]=id;
            prepare(p[3]+4,0);if(!local_command)$fatal(1,"local decode");send;
        end
    endtask
    task request;
        input [7:0] id;
        begin
            p[0]=255;p[1]=255;p[2]=id;p[3]=4;p[4]=2;p[5]=8'h38;p[6]=15;
            prepare(8,0);if(!allow_forward)$fatal(1,"read blocked");send;
        end
    endtask
    task telemetry;
        input [7:0] id,voltage,temp,flags;
        input [15:0] current;
        input integer corrupt;
        reg [7:0] sum;
        integer n;
        begin
            for(n=0;n<21;n=n+1)r[n]=0;
            r[0]=255;r[1]=255;r[2]=id;r[3]=17;r[5]=test_position[7:0];r[6]=test_position[15:8];
            r[11]=voltage;r[12]=temp;r[14]=flags;r[18]=current[7:0];r[19]=current[15:8];
            sum=0;for(n=2;n<20;n=n+1)sum=sum+r[n];r[20]=~sum;
            if(corrupt)r[20]=r[20]^1;
            for(n=0;n<21;n=n+1)begin
                @(negedge clk);servo_data=r[n];servo_valid=1;
                @(negedge clk);servo_valid=0;
            end
            repeat(5)@(negedge clk);
        end
    endtask
    task healthy;
        input [7:0] id;
        begin request(id);telemetry(id,120,40,0,100,0);end
    endtask
    task arm;
        input [7:0] id;
        begin healthy(id);local(1,id);if(latched || !status[32+id-1])$fatal(1,"arm failed ID %0d",id);end
    endtask
    task expect_trip;
        input [7:0] why,id;
        begin
            repeat(5)@(negedge clk);
            if(!latched || reason!=why || fault_id!=id || !stop_request)
                $fatal(1,"trip expected %0d ID %0d, got latch=%0d reason=%0d ID=%0d",why,id,latched,reason,fault_id);
            cases=cases+1;
        end
    endtask
    task pwm_packet;
        input [7:0] id;
        input [15:0] value;
        begin p[0]=255;p[1]=255;p[2]=id;p[3]=5;p[4]=3;p[5]=8'h2c;p[6]=value[7:0];p[7]=value[15:8];prepare(9,0);end
    endtask
    task torque_off;
        input [7:0] id;
        begin p[0]=255;p[1]=255;p[2]=id;p[3]=4;p[4]=3;p[5]=8'h28;p[6]=0;prepare(8,0);send;end
    endtask
    task read_one;
        input [7:0] id,address;
        begin p[0]=255;p[1]=255;p[2]=id;p[3]=4;p[4]=2;p[5]=address;p[6]=1;prepare(8,0);send;end
    endtask
    task one_reply;
        input [7:0] id,value,error;
        input integer corrupt;
        integer n;reg [7:0] sum;
        begin
            r[0]=255;r[1]=255;r[2]=id;r[3]=3;r[4]=error;r[5]=value;
            sum=id+3+error+value;r[6]=~sum;if(corrupt)r[6]=r[6]^1;
            for(n=0;n<7;n=n+1)begin @(negedge clk);servo_data=r[n];servo_valid=1;@(negedge clk);servo_valid=0;end
            repeat(5)@(negedge clk);
        end
    endtask

    task window;
        input [7:0] id;
        input [15:0] lo,hi;
        begin
            p[0]=255;p[1]=255;p[2]=254;p[3]=8;p[4]=8'ha0;p[5]=6;p[6]=id;
            p[7]=lo[7:0];p[8]=lo[15:8];p[9]=hi[7:0];p[10]=hi[15:8];prepare(12,0);send;
        end
    endtask
    task continuous_window;
        input [7:0] id;input signed [31:0] anchor,lo,hi;
        integer k;
        begin
            p[0]=255;p[1]=255;p[2]=254;p[3]=16;p[4]=8'ha0;p[5]=7;p[6]=id;
            for(k=0;k<4;k=k+1)begin p[7+k]=anchor[8*k +: 8];p[11+k]=lo[8*k +: 8];p[15+k]=hi[8*k +: 8];end
            prepare(20,0);send;
        end
    endtask
    initial begin
        reset;healthy(1);local(1,1);if(!latched)$fatal(1,"arming without window");
        for(j=1;j<=3;j=j+1)begin
            reset;window(j,2040,2056);arm(j);
            pwm_packet(j,1000);if(!allow_forward)$fatal(1,"bounded PWM denied");
            pwm_packet(j,1001);if(allow_forward)$fatal(1,"PWM ceiling bypass");
            pwm_packet(j,2024);if(!allow_forward)$fatal(1,"reverse denied");
            pwm_packet(j,2049);if(allow_forward)$fatal(1,"unknown direction bit allowed");
            pwm_packet(j+3,1);if(allow_forward)$fatal(1,"foreign ID aliases");
            p[5]=8'h2a;prepare(9,0);if(allow_forward)$fatal(1,"position mode bypass");
            if(j!=1)begin healthy(1);local(1,1);if(status[32])$fatal(1,"two simultaneous arms");end
            repeat(1600)@(negedge clk);if(!latched)$fatal(1,"missing host watchdog");
        end
        reset;window(1,2040,2056);arm(1);test_position=2056;healthy(1);
        pwm_packet(1,25);if(allow_forward)$fatal(1,"outward at upper boundary");
        pwm_packet(1,1049);if(!allow_forward)$fatal(1,"recovery from upper boundary");
        test_position=2057;healthy(1);expect_trip(12,1);
        test_position=2048;reset;window(1,500,3500);arm(1);
        reset;window(1,2000,4100);local(1,1);if(!latched)$fatal(1,"invalid window allowed");
        reset;window(1,2040,2056);arm(1);emergency_stop=1;expect_trip(9,254);
        // Crossing raw zero is continuous motion, not a mechanical limit.
        test_position=4;reset;healthy(2);continuous_window(2,4,-50,100);local(1,2);
        if(latched)$fatal(1,"continuous window refused");
        test_position=0;healthy(2);pwm_packet(2,1049);if(!allow_forward)$fatal(1,"zero blocked negative PWM");
        test_position=4095;healthy(2);if(latched || dut.continuous_position[1]!=-1)$fatal(1,"negative rollover not tracked");
        test_position=4045;healthy(2);expect_trip(12,2);
        test_position=4093;reset;healthy(2);continuous_window(2,4093,4000,13000);local(1,2);
        test_position=4095;healthy(2);pwm_packet(2,25);if(!allow_forward)$fatal(1,"4095 blocked positive PWM");
        test_position=1;healthy(2);if(latched || dut.continuous_position[1]!=4097)$fatal(1,"positive rollover not tracked");
        for(j=0;j<2;j=j+1)begin
            test_position=1000;healthy(2);local(3,2);test_position=2000;healthy(2);local(3,2);
            test_position=3000;healthy(2);local(3,2);test_position=4000;healthy(2);local(3,2);test_position=1;healthy(2);local(3,2);
        end
        if(latched || dut.continuous_position[1]!=12289)$fatal(1,"multiple turns not tracked");
        test_position=900;healthy(2);expect_trip(12,2);
        test_position=4;reset;healthy(2);continuous_window(2,5,-50,100);local(1,2);if(!latched)$fatal(1,"false anchor accepted");
        test_position=4;reset;healthy(2);continuous_window(2,4,-4000,4000);local(1,2);
        test_position=2052;healthy(2);expect_trip(13,2);
        $display("PASS continuous encoder: both zero crossings, multiple turns, independent signed limits, anchor validation, ambiguous-jump stop");
        $display("PASS calibration profile: windows, PWM cap, ID isolation, command gating, watchdog, S2");$finish;
    end
endmodule
