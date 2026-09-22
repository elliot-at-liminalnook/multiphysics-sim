// Decoder for a complete validated host packet. The caller supplies a one-cycle
// check pulse and the framing/checksum verdict; never feed partial UART bytes.
// No ARM/START exists. Fault invalidates any formerly sealed trajectory.
module trajectory_upload(
    input wire clk,rst,active,check,packet_ok,transport_fault,
    input wire [511:0] packet,
    input wire [6:0] packet_length,
    input wire [8:0] read_index,
    output wire handled,valid,busy,fault,row_ready,
    output wire [8:0] mask,frames,written,
    output wire [31:0] period,crc32,
    output wire [15:0] kp,kd,kv,duty_limit,
    output wire [143:0] homes,row_targets,row_deltas
);
    wire selected=check && packet[23:16]==254 && packet[39:32]==8'ha2;
    wire [7:0] op=packet[47:40];
    wire framing=packet_ok && packet[15:0]==16'hffff && {1'b0,packet[31:24]}+9'd4==packet_length;
    wire is_config=framing && op==0 && packet_length==42 && packet[55:48]==1
        && packet[71:65]==0 && packet[87:81]==0;
    wire is_row=framing && op==1 && packet_length==45 && packet[63:57]==0;
    wire is_seal=framing && op==2 && packet_length==11;
    assign handled=selected;
    wire invalid=selected && !(is_config || is_row || is_seal);
    trajectory_store memory_bank(
        .clk(clk),.rst(rst),.active(active),.invalidate_plan(invalid || transport_fault),
        .configure(selected && is_config),.write_row(selected && is_row),.seal(selected && is_seal),
        .input_mask(packet[64:56]),.input_frames(packet[80:72]),.input_period(packet[119:88]),
        .input_kp(packet[135:120]),.input_kd(packet[151:136]),.input_kv(packet[167:152]),
        .input_limit(packet[183:168]),.input_homes(packet[327:184]),
        .write_index(packet[56:48]),.write_data(packet[351:64]),.expected_crc32(packet[79:48]),
        .read_index(read_index),.valid(valid),.busy(busy),.fault(fault),.row_ready(row_ready),
        .mask(mask),.frames(frames),.period(period),.kp(kp),.kd(kd),.kv(kv),.duty_limit(duty_limit),
        .homes(homes),.row_targets(row_targets),.row_deltas(row_deltas),.crc32(crc32),.written(written));
endmodule
