// FPGA safety policy for an explicitly armed HX bench chain.
// Times are FPGA clocks. Voltage is 0.1 V/count. Current is the uncalibrated
// servo current register, NOT an external supply-current measurement.
module hx_safety #(
    parameter FIRST_ID=4, COUNT=9,
    parameter CALIBRATION=0, PWM_LIMIT=1000,
    parameter TEMP_MAX=60, VOLT_MIN=90, VOLT_MAX=126, CURRENT_MAX=2000,
    parameter FEEDBACK_TIMEOUT=10000000, // 200 ms at 50 MHz
    parameter COMMAND_TIMEOUT=15000000,  // 300 ms at 50 MHz
    parameter RX_TIMEOUT=50000,
    parameter STOP_REPEAT=2500000
)(
    input wire clk, rst,
    input wire check_host,
    input wire [511:0] host_packet,
    input wire [6:0] host_length,
    input wire [7:0] host_checksum,
    input wire emergency_stop,
    input wire bridge_fault,
    output reg allow_forward,
    output wire local_command,
    output wire [103:0] local_status,
    output reg [COUNT*16-1:0] positions=0,
    output reg [COUNT*8-1:0] sample_sequences=0,
    input wire servo_valid,
    input wire [7:0] servo_data,
    output reg latched=1,
    output reg [7:0] reason=1,
    output reg [7:0] fault_id=254,
    output wire stop_request,
    input wire stop_accepted
);
    reg [COUNT-1:0] armed=0, seen=0, healthy=0, expected=0, expected_off=0;
    reg [$clog2(FEEDBACK_TIMEOUT+1)-1:0] feedback_age[0:COUNT-1];
    reg [$clog2(COMMAND_TIMEOUT+1)-1:0] command_age[0:COUNT-1];
    reg [7:0] sample_reason[0:COUNT-1];
    reg [$clog2(STOP_REPEAT+1)-1:0] stop_age=0;
    // All index uses below are gated by known(id). Narrow the address, not
    // the full ID validation; unknown IDs must never alias an armed motor.
    localparam AXIS_BITS=COUNT>1 ? $clog2(COUNT) : 1;
    wire [7:0] hid=host_packet[23:16];
    wire [AXIS_BITS-1:0] host_axis=hid-FIRST_ID;
    wire [AXIS_BITS-1:0] command_axis=host_packet[55:48]-FIRST_ID;
    wire [7:0] inst=host_packet[39:32];
    wire [7:0] addr=host_packet[47:40];
    function known;
        input [7:0] id;
        begin known=(id>=FIRST_ID && id<FIRST_ID+COUNT); end
    endfunction
    wire frame_ok=host_length>=6 && host_length<=64 && host_packet[8*(0) +: 8]==255 && host_packet[8*(1) +: 8]==255
        && host_packet[8*(3) +: 8]+8'd4==host_length && host_checksum==255;
    assign local_command=frame_ok && hid==254 && inst==8'ha0;

    reg [COUNT-1:0] window_valid=0;
    reg [15:0] lower [0:COUNT-1];
    reg [15:0] upper [0:COUNT-1];
    function write_ok;
        input blocked;
        input [COUNT-1:0] arm_mask;
        input [7:0] id,address;
        input integer width;
        input zero,one;
        reg [AXIS_BITS-1:0] motor_axis;
        begin
            motor_axis=id-FIRST_ID;
            write_ok=0;
            if(known(id) || id==254) begin
                if(address==8'h28 && width==1 && zero) write_ok=1;
                else if(address==8'h2c && (width==2 || width==4) && zero) write_ok=1;
                else if(address==8'h37 && width==1 && one) write_ok=1;
                else if(known(id) && !blocked && arm_mask[motor_axis]) write_ok=1;
            end
        end
    endfunction
    wire ordinary_zero=(host_length==8 && host_packet[55:48]==0)
        || (host_length==9 && host_packet[63:48]==0)
        || (host_length==11 && host_packet[79:48]==0);
    // Equivalent to length>=11 && length<=64 && (length-8)%3==0.
    // The unsized subtraction otherwise infers a 32-bit remainder circuit.
    function sync_word_length;
        input [6:0] length;
        begin case(length)
            11,14,17,20,23,26,29,32,35,38,41,44,47,50,53,56,59,62: sync_word_length=1;
            default: sync_word_length=0;
        endcase end
    endfunction
    // A batch cannot contain more entries than this profile has motors.
    // Bound the complete frame before checking every included entry.
    integer q;
    always @* begin
        allow_forward=0;
        if(frame_ok && !local_command) begin
            if(inst==1 && host_length==6) allow_forward=1;
            else if(inst==2 && host_length==8) allow_forward=1;
            else if(inst==3 && host_length>=8)
                allow_forward=write_ok(latched,armed,hid,addr,host_length-7,ordinary_zero,host_packet[55:48]==1);
            else if(inst==8'h83 && hid==254 && host_packet[55:48]==2
                && (addr==8'h2a || addr==8'h2c) && sync_word_length(host_length)
                && host_length<=8+3*COUNT) begin
                allow_forward=1;
                for(q=7;q<61 && q<7+3*COUNT;q=q+3)
                    if(host_length>q+1 && !write_ok(latched,armed,host_packet[8*q +: 8],addr,2,
                        host_packet[8*(q+1) +: 16]==0,1'b0)) allow_forward=0;
            end
            else if(inst==8'h83 && hid==254 && host_packet[55:48]==1 && addr==8'h28
                && host_length>=10 && host_length<=8+2*COUNT && !host_length[0]) begin
                allow_forward=1;
                for(q=7;q<63 && q<7+2*COUNT;q=q+2)
                    if(host_length>q+1 && !write_ok(latched,armed,host_packet[8*q +: 8],addr,1,
                        host_packet[8*(q+1) +: 8]==0,1'b0)) allow_forward=0;
            end
        end
        // Calibration accepts only addressed, bounded PWM and explicit mode setup.
        // No position commands, sync commands, or unrelated NVS writes can bypass it.
        if(CALIBRATION && inst!=1 && inst!=2 && !local_command) begin
            if(inst!=3 || !known(hid)) allow_forward=0;
            else if(addr==8'h2c) begin
                if(host_length!=9 || host_packet[63:59]!=0 || host_packet[57:48]>PWM_LIMIT)
                    allow_forward=0;
                if(host_packet[57:48]!=0 && (!window_valid[host_axis]
                    || (host_packet[58] && positions[16*host_axis +: 16]<=lower[host_axis])
                    || (!host_packet[58] && positions[16*host_axis +: 16]>=upper[host_axis])))
                    allow_forward=0;
            end else if(addr==8'h28) begin
                if(host_length!=8 || host_packet[55:48]>1) allow_forward=0;
            end else if(addr==8'h37) begin
                if(host_length!=8 || host_packet[55:48]>1) allow_forward=0;
            end else if(addr==8'h21) begin
                if(host_length!=8 || host_packet[55:48]!=2) allow_forward=0;
            end else allow_forward=0;
        end
    end

    // A telemetry sample is accepted only after a forwarded 0x38/15 read
    // for that ID, with complete framing and checksum. ACK/config packets do
    // not refresh feedback age. Bad, foreign, or missing replies age out.
    reg [1:0] rx_state=0;
    reg [6:0] rx_count=0, rx_total=0;
    reg [7:0] rx_sum=0;
    reg [7:0] rb[0:20];
    reg [$clog2(RX_TIMEOUT+1)-1:0] rx_age=0;
    reg sample_valid=0, packet_valid=0, off_valid=0;
    reg [7:0] sample_id=0, decoded_reason=0;
    wire [AXIS_BITS-1:0] sample_axis=sample_id-FIRST_ID;
    integer i;
    always @(posedge clk) begin
        sample_valid<=0; packet_valid<=0; off_valid<=0;
        if(rst) begin rx_state<=0; rx_age<=0; end
        else begin
            if(servo_valid) rx_age<=0;
            else if(rx_age<RX_TIMEOUT) rx_age<=rx_age+1;
            if(rx_age>=RX_TIMEOUT) rx_state<=0;
            if(servo_valid) case(rx_state)
                0: if(servo_data==255) rx_state<=1;
                1: if(servo_data==255) begin rx_state<=2; rx_count<=2; rx_sum<=0; end
                   else rx_state<=0;
                2: begin
                    if(rx_count<21) rb[rx_count]<=servo_data;
                    rx_sum<=rx_sum+servo_data;
                    if(rx_count==3) begin
                        if(servo_data<2 || servo_data>60) rx_state<=0;
                        else begin rx_total<=servo_data+4; rx_count<=4; end
                    end else if(rx_count>3 && rx_count+1==rx_total) begin
                        rx_state<=0;
                        if(rx_sum+servo_data==8'hff && known(rb[2])) begin
                            packet_valid<=1; sample_id<=rb[2];
                            if(rx_total==7 && rb[4]==0 && rb[5]==0) off_valid<=1;
                        end
                        if(rx_total==21 && rx_sum+servo_data==8'hff && known(rb[2])) begin
                            sample_valid<=1;
                            if(rb[12]>=TEMP_MAX) decoded_reason<=2;
                            else if(rb[11]<VOLT_MIN) decoded_reason<=3;
                            else if(rb[11]>VOLT_MAX) decoded_reason<=4;
                            else if({rb[19],rb[18]}>=CURRENT_MAX) decoded_reason<=5;
                            else if(rb[4]!=0 || rb[14]!=0) decoded_reason<=6;
                            else decoded_reason<=0;
                        end
                    end else rx_count<=rx_count+1;
                end
                default: rx_state<=0;
            endcase
        end
    end

    reg [7:0] trip_reason, trip_id;
    reg [COUNT-1:0] fresh;
    integer n;
    always @* begin
        trip_reason=0; trip_id=254; fresh=0;
        for(n=0;n<COUNT;n=n+1) begin
            fresh[n]=seen[n] && healthy[n] && feedback_age[n]<FEEDBACK_TIMEOUT;
            if(armed[n] && trip_reason==0) begin
                if(!healthy[n]) begin trip_reason=sample_reason[n]; trip_id=FIRST_ID+n; end
                else if(feedback_age[n]>=FEEDBACK_TIMEOUT) begin trip_reason=7; trip_id=FIRST_ID+n; end
                else if(CALIBRATION && (!window_valid[n] || positions[16*n +: 16]<lower[n]
                    || positions[16*n +: 16]>upper[n])) begin trip_reason=12;trip_id=FIRST_ID+n;end
                else if(command_age[n]>=COMMAND_TIMEOUT) begin trip_reason=8; trip_id=FIRST_ID+n; end
            end
        end
    end
    wire [15:0] armed16=armed;
    wire [15:0] fresh16=fresh;
    // Little-endian byte fields: version, latch, reason, ID, armed mask,
    // fresh mask, temperature/voltage/current thresholds.
    assign local_status={CURRENT_MAX[15:8],CURRENT_MAX[7:0],VOLT_MAX[7:0],VOLT_MIN[7:0],
        TEMP_MAX[7:0],fresh16[15:8],fresh16[7:0],armed16[15:8],armed16[7:0],fault_id,reason,{7'b0,latched},(CALIBRATION ? 8'd4 : 8'd1)};
    assign stop_request=latched && stop_age==0;
    always @(posedge clk) begin
        if(rst) begin
            latched<=1; reason<=1; fault_id<=254;
            window_valid<=0; armed<=0; seen<=0; healthy<=0; expected<=0; expected_off<=0; stop_age<=0;
            for(i=0;i<COUNT;i=i+1) begin
                feedback_age[i]<=FEEDBACK_TIMEOUT; command_age[i]<=COMMAND_TIMEOUT; sample_reason[i]<=7;
            end
        end else begin
            if(stop_age!=0) stop_age<=stop_age-1;
            if(stop_accepted) stop_age<=STOP_REPEAT;
            for(i=0;i<COUNT;i=i+1) begin
                if(feedback_age[i]<FEEDBACK_TIMEOUT) feedback_age[i]<=feedback_age[i]+1;
                if(command_age[i]<COMMAND_TIMEOUT) command_age[i]<=command_age[i]+1;
            end
            // Correlate only with the most recently forwarded request for an
            // ID. A later config read cannot satisfy an older torque read.
            if(check_host && allow_forward) begin
                if(known(hid)) begin
                    expected[host_axis]<=(inst==2 && addr==8'h38 && host_packet[55:48]==15);
                    expected_off[host_axis]<=(inst==2 && addr==8'h28 && host_packet[55:48]==1);
                end else if(hid==254) begin expected<=0; expected_off<=0; end
            end
            if(packet_valid) begin
                expected[sample_axis]<=0;
                expected_off[sample_axis]<=0;
            end
            if(sample_valid && expected[sample_axis]) begin
                positions[16*sample_axis +: 16]<={rb[6],rb[5]};
                sample_sequences[8*sample_axis +: 8]<=sample_sequences[8*sample_axis +: 8]+1;
                seen[sample_axis]<=1;
                healthy[sample_axis]<=(decoded_reason==0);
                sample_reason[sample_axis]<=decoded_reason;
                feedback_age[sample_axis]<=0;
            end
            // Merely transmitting torque-off does NOT remove the watchdog.
            // Keep it armed until a solicited, checksum-valid 0x28 read reports
            // torque disabled. Missing/corrupt replies therefore still trip.
            if(off_valid && expected_off[sample_axis]) armed[sample_axis]<=0;
            if(emergency_stop || bridge_fault) begin
                latched<=1; reason<=emergency_stop ? 9 : 11; fault_id<=254; armed<=0;
                if(!latched) stop_age<=0;
            end else if(trip_reason!=0 && !latched) begin
                latched<=1; reason<=trip_reason; fault_id<=trip_id; armed<=0; stop_age<=0;
            end else if(check_host && local_command) begin
                if(CALIBRATION && addr==6 && host_length==12 && known(host_packet[55:48]) && armed==0) begin
                    // [6,id,lower LE16,upper LE16], only while every axis is disarmed.
                    if(host_packet[71:56]<host_packet[87:72] && host_packet[87:72]<=4095) begin
                        lower[command_axis]<=host_packet[71:56];upper[command_axis]<=host_packet[87:72];
                        window_valid[command_axis]<=1;
                    end else window_valid[command_axis]<=0;
                end else if(addr==0 && host_length==7) begin
                    latched<=1; reason<=10; fault_id<=254; armed<=0; stop_age<=0;
                end else if(addr==1 && host_length==8 && known(host_packet[8*(6) +: 8])) begin
                    if(fresh[command_axis] && (!CALIBRATION || (armed==0 && window_valid[command_axis]
                        && positions[16*command_axis +: 16]>=lower[command_axis]
                        && positions[16*command_axis +: 16]<=upper[command_axis]))) begin
                        latched<=0; reason<=0; fault_id<=254;
                        armed[command_axis]<=1; command_age[command_axis]<=0;
                    end
                end else if(addr==3 && host_length==8 && known(host_packet[8*(6) +: 8]) && !latched) begin
                    if(armed[command_axis]) command_age[command_axis]<=0;
                end else if(addr==5 && host_length==9 && !latched && host_packet[63:48]!=0
                    && host_packet[63:48]<(1<<COUNT) && (host_packet[63:48] & armed16)==host_packet[63:48]) begin
                    // Explicit bounded batch lease renewal. Never arms or clears faults.
                    for(i=0;i<COUNT;i=i+1) if(host_packet[48+i]) command_age[i]<=0;
                end else if(addr==4 && host_length==8 && known(host_packet[8*(6) +: 8])) begin
                    armed[command_axis]<=0;
                    // Explicit disarm also requests a global stop, keeping a
                    // lost host from leaving a remembered nonzero drive.
                    latched<=1; reason<=10; fault_id<=host_packet[8*(6) +: 8]; armed<=0; stop_age<=0;
                end
            end
        end
    end
endmodule
