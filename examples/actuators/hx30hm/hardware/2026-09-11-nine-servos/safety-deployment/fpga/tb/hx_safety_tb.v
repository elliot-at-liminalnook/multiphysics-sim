`timescale 1ns/1ps
module hx_safety_tb;
    reg clk=0; always #5 clk=~clk;
    reg rst=1,check_host=0,servo_valid=0,stop_accepted=0,emergency_stop=0,bridge_fault=0;
    reg [511:0] host_packet=0;
    reg [6:0] host_length=0;
    reg [7:0] host_checksum=0,servo_data=0;
    wire allow_forward,local_command,latched,stop_request;
    wire [7:0] reason,fault_id;
    wire [103:0] status;
    hx_safety #(.FEEDBACK_TIMEOUT(1000),.COMMAND_TIMEOUT(1500),.RX_TIMEOUT(100),.STOP_REPEAT(200)) dut (
        .clk(clk),.rst(rst),.check_host(check_host),.host_packet(host_packet),.host_length(host_length),
        .host_checksum(host_checksum),.emergency_stop(emergency_stop),.bridge_fault(bridge_fault),
        .allow_forward(allow_forward),.local_command(local_command),.local_status(status),
        .servo_valid(servo_valid),.servo_data(servo_data),.latched(latched),.reason(reason),
        .fault_id(fault_id),.stop_request(stop_request),.stop_accepted(stop_accepted));
    reg [7:0] p[0:63],r[0:20];
    integer i,j,cases=0;
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
            r[0]=255;r[1]=255;r[2]=id;r[3]=17;r[6]=8;
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
        begin healthy(id);local(1,id);if(latched || !status[32+id-4])$fatal(1,"arm failed ID %0d",id);end
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
    initial begin
        reset;arm(12);torque_off(12);
        if(!status[40])$fatal(1,"torque-off request disabled watchdog before servo readback");
        repeat(1010)@(negedge clk);expect_trip(7,12);
        reset;arm(12);torque_off(12);read_one(12,8'h28);one_reply(12,0,0,0);
        if(status[40])$fatal(1,"verified torque-off did not disarm ID 12");
        repeat(1510)@(negedge clk);if(latched)$fatal(1,"verified torque-off retained watchdog");cases=cases+1;
        reset;arm(12);read_one(12,8'h28);one_reply(12,1,0,0);
        if(!status[40])$fatal(1,"torque-enable=1 disarmed");cases=cases+1;
        reset;arm(12);read_one(12,8'h28);one_reply(12,0,0,1);
        if(!status[40])$fatal(1,"corrupt torque readback disarmed");cases=cases+1;
        reset;arm(12);read_one(12,8'h28);read_one(12,8'h37);one_reply(12,0,0,0);
        if(!status[40])$fatal(1,"unrelated later register reply disarmed");cases=cases+1;
        reset;

        local(1,12);if(!latched)$fatal(1,"armed without fresh telemetry");
        pwm_packet(12,100);if(allow_forward)$fatal(1,"unarmed PWM allowed");
        pwm_packet(254,0);if(!allow_forward)$fatal(1,"broadcast zero blocked");
        arm(12);pwm_packet(12,100);if(!allow_forward)$fatal(1,"armed PWM blocked");
        pwm_packet(4,100);if(allow_forward)$fatal(1,"wrong channel armed");
        cases=cases+1;

        request(12);telemetry(12,120,60,0,100,0);expect_trip(2,12);
        healthy(12);if(!latched)$fatal(1,"fault auto-cleared");
        local(1,12);if(latched)$fatal(1,"explicit healthy rearm failed");
        request(12);telemetry(12,89,40,0,100,0);expect_trip(3,12);
        reset;arm(12);request(12);telemetry(12,127,40,0,100,0);expect_trip(4,12);
        reset;arm(12);request(12);telemetry(12,120,40,0,2000,0);expect_trip(5,12);
        reset;arm(12);request(12);telemetry(12,120,40,1,100,0);expect_trip(6,12);

        reset;arm(12);request(12);telemetry(12,90,59,0,1999,0);
        if(latched)$fatal(1,"inclusive low-voltage boundary rejected");
        request(12);telemetry(12,126,59,0,1999,0);
        if(latched)$fatal(1,"inclusive high-voltage boundary rejected");cases=cases+1;

        reset;arm(12);request(12);telemetry(12,120,90,0,100,1);
        if(latched)$fatal(1,"bad CRC triggered sensor fault");
        repeat(1010)@(negedge clk);expect_trip(7,12);
        reset;telemetry(12,120,40,0,100,0);local(1,12);
        if(!latched)$fatal(1,"unsolicited reply armed servo");cases=cases+1;

        reset;arm(12);
        for(j=0;j<20;j=j+1)begin healthy(12);repeat(60)@(negedge clk);end
        expect_trip(8,12); // telemetry alone must not renew the command lease
        reset;arm(4);arm(5);
        for(j=0;j<20;j=j+1)begin healthy(4);healthy(5);local(3,4);repeat(30)@(negedge clk);end
        expect_trip(8,5); // activity on ID 4 cannot keep ID 5 alive

        reset;arm(12);emergency_stop=1;expect_trip(9,254);
        local(1,12);if(!latched)$fatal(1,"rearmed while emergency stop held");
        reset;arm(12);bridge_fault=1;expect_trip(11,254);
        reset;arm(12);local(0,0);expect_trip(10,254);
        @(negedge clk);stop_accepted=1;@(negedge clk);stop_accepted=0;
        if(stop_request)$fatal(1,"stop repeat not rate limited");
        repeat(205)@(negedge clk);if(!stop_request)$fatal(1,"stop was not retried");cases=cases+1;

        reset;arm(4);
        p[0]=255;p[1]=255;p[2]=254;p[3]=10;p[4]=8'h83;p[5]=8'h2c;p[6]=2;
        p[7]=4;p[8]=100;p[9]=0;p[10]=5;p[11]=100;p[12]=0;
        prepare(14,0);if(allow_forward)$fatal(1,"SYNC write included unarmed servo");
        p[11]=0;prepare(14,0);if(!allow_forward)$fatal(1,"SYNC zero to unarmed channel blocked");
        prepare(14,1);if(allow_forward)$fatal(1,"bad request checksum allowed");cases=cases+1;
        $display("PASS: %0d safety cases, thresholds, malformed/foreign telemetry, independent leases, latching, rearm, stop retries, command gating",cases);
        $finish;
    end
    initial begin #2000000;$fatal(1,"test timeout");end
endmodule
