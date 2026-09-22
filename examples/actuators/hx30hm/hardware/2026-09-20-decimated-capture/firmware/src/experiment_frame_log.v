// Bounded, lossless frame capture, independent of host-UART serialization.
// Explicit three-selected-axis profile; any IDs 4..12, never robot-specific.
// Valid raw replies occur once on the wire; compact events reference their ends.
// Unknown/partial/corrupt raw bytes are retained too. Overflow is explicit and
// faults the independent supervisor; already queued evidence still drains.
module experiment_frame_log #(
    parameter QUEUE_DEPTH=4, parameter LOG_STRIDE=1
)(
    input wire clk,rst,
    input wire raw_valid,input wire [7:0] raw_data,
    input wire event_pulse,
    input wire input_valid,output wire input_ready,
    input wire [511:0] input_packet,input wire [6:0] input_length,
    output wire output_valid,input wire output_ready,
    input wire [7:0] read_index,output reg [7:0] read_data,
    output wire [8:0] output_length,
    output wire busy,output wire capture_active,output reg fault=0
);
    reg [7:0] queue[0:QUEUE_DEPTH*256-1];
    localparam BANK_BITS=$clog2(QUEUE_DEPTH);
    reg [8:0] lengths[0:QUEUE_DEPTH-1];
    reg [BANK_BITS-1:0] head=0,tail=0;
    reg [BANK_BITS:0] count=0;
    reg copying_boundary=0;
    reg [511:0] boundary_packet=0;
    reg [6:0] boundary_length=0,boundary_index=0;
    reg opened=0,flush_pending=0,partial=0,diagnostic=0;
    reg copying_event=0,last_audit_copy=0;
    reg [151:0] event_record=0;
    reg [4:0] copy_index=0,copy_length=0;
    reg [7:0] raw[0:127];
    reg [7:0] metadata[0:127];
    reg [7:0] raw_read_data,metadata_read_data;
    reg [7:0] raw_count=0,raw_end_capture=0,metadata_count=0,event_count=0;
    reg [15:0] dropped=0;
    reg [31:0] run=0,period=0;
    reg [63:0] base=0;
    reg [15:0] frame=0;
    reg [8:0] mask=0;
    wire [7:0] kind=input_packet[7*8 +: 8];
    wire [7:0] motor=input_packet[14*8 +: 8];
    wire [15:0] input_frame=input_packet[12*8 +: 16];
    wire [63:0] request_time=input_packet[17*8 +: 64];
    wire [63:0] completion_time=input_packet[25*8 +: 64];
    
    wire [63:0] request_delta=request_time-base,completion_delta=completion_time-base;
    wire pop=output_valid && output_ready;
    wire terminal_flush=input_valid && kind==4 && (raw_count!=0 || event_count!=0 || dropped!=0);
    assign input_ready=!copying_boundary && !copying_event && !flush_pending && !terminal_flush && count<QUEUE_DEPTH;
    assign output_valid=count!=0;
    always @(posedge clk)read_data<=queue[{head,read_index}];
    assign output_length=lengths[head];
    assign capture_active=opened;
    assign busy=opened || copying_boundary || flush_pending || count!=0;
    function [3:0] selected_count;
        input [8:0] value;integer k;
        begin selected_count=0;for(k=0;k<9;k=k+1)selected_count=selected_count+value[k];end
    endfunction
    function [7:0] final_id;
        input [8:0] value;integer k;
        begin final_id=0;for(k=0;k<9;k=k+1)if(value[k])final_id=k+4;end
    endfunction
    reg building=0,build_done=0,build_wait=0;
    reg [8:0] build_index=0;
    wire [8:0] packet_size=30+raw_count+metadata_count;
    reg [7:0] checksum=0,build_byte;
    wire [6:0] raw_read_address=build_index-29;
    wire [6:0] metadata_read_address=build_index-29-raw_count;
    wire [6:0] metadata_write_address=metadata_count+copy_index;
    // Synchronous read/write RAMs. A build wait clock makes the requested byte
    // available before checksum/output commit; capture never waits for this read.
    always @(posedge clk)begin
        if(!rst && raw_valid && opened && raw_count<128)raw[raw_count[6:0]]<=raw_data;
        if(!rst && copying_event)metadata[metadata_write_address]<=event_record[copy_index*8 +: 8];
        raw_read_data<=raw[raw_read_address];
        metadata_read_data<=metadata[metadata_read_address];
    end
    integer index;
    // One byte/checksum update per two clocks. No packet-wide checksum chain,
    // dynamic packet-wide shifts, or frame-index multiplication.
    always @* begin
        build_byte=0;
        case(build_index)
            0,1:build_byte=255;
            2:build_byte=253;3:build_byte=packet_size-4;
            4:build_byte=0;5:build_byte=2;6:build_byte=8'ha4;7:build_byte=1;
            8,9,10,11:build_byte=run[(build_index-8)*8 +: 8];
            12,13:build_byte=frame[(build_index-12)*8 +: 8];
            14,15,16,17,18,19,20,21:build_byte=base[(build_index-14)*8 +: 8];
            22:build_byte=raw_count;23:build_byte=event_count;
            24:build_byte=mask[7:0];25:build_byte={7'd0,mask[8]};
            26:build_byte={4'd0,(diagnostic || fault || raw_count!=33*selected_count(mask)),(LOG_STRIDE==2),(dropped!=0),partial};
            27:build_byte=dropped[7:0];28:build_byte=dropped[15:8];
            default:begin
                if(build_index<29+raw_count)build_byte=raw_read_data;
                else if(build_index<packet_size-1)build_byte=metadata_read_data;
            end
        endcase
        if(build_index==packet_size-1)build_byte=~checksum;
    end
    reg push;
    integer j,at;
    always @(posedge clk) begin
        push=0;
        if(rst) begin
            head<=0;tail<=0;count<=0;opened<=0;flush_pending<=0;fault<=0;building<=0;build_done<=0;build_wait<=0;copying_event<=0;copying_boundary<=0;
            raw_count<=0;metadata_count<=0;event_count<=0;dropped<=0;partial<=0;diagnostic<=0;
        end else begin
            if(event_pulse)raw_end_capture<=raw_count+(raw_valid ? 1 : 0);
            if(raw_valid && opened) begin
                building<=0;build_done<=0;
                if(raw_count<128)raw_count<=raw_count+1;
                else begin fault<=1;if(dropped!=65535)dropped<=dropped+1;end
            end
            if(terminal_flush && !copying_event)begin flush_pending<=1;partial<=1;end
            if(copying_event)begin
                if(copy_index+1==copy_length)begin
                    metadata_count<=metadata_count+copy_length;event_count<=event_count+1;
                    copying_event<=0;if(last_audit_copy)flush_pending<=1;
                end else copy_index<=copy_index+1;
            end
            if(copying_boundary)begin
                queue[{tail,1'b0,boundary_index}]<=boundary_packet[boundary_index*8 +: 8];
                if(boundary_index+1==boundary_length)begin
                    lengths[tail]<=boundary_length;push=1;copying_boundary<=0;
                end else boundary_index<=boundary_index+1;
            end else if(flush_pending && count<QUEUE_DEPTH && !raw_valid)begin
                if(LOG_STRIDE==2 && frame[0] && !partial && !diagnostic && !fault && dropped==0 && raw_count==33*selected_count(mask))begin
                    raw_count<=0;metadata_count<=0;event_count<=0;dropped<=0;flush_pending<=0;
                    frame<=frame+1;base<=base+period;building<=0;build_done<=0;diagnostic<=0;
                end else if(packet_size>255)fault<=1;
                else if(!building)begin
                    checksum<=0;build_index<=0;building<=1;build_done<=0;build_wait<=0;
                end else if(!build_done)begin
                    if(!build_wait)build_wait<=1;
                    else begin
                    queue[{tail,build_index[7:0]}]<=build_byte;
                    if(build_index>=2 && build_index<packet_size-1)checksum<=checksum+build_byte;
                    if(build_index==packet_size-1)build_done<=1;else build_index<=build_index+1;
                    build_wait<=0;
                    end
                end else begin
                    lengths[tail]<=packet_size;push=1;
                    raw_count<=0;metadata_count<=0;event_count<=0;dropped<=0;
                    flush_pending<=0;partial<=0;diagnostic<=0;frame<=frame+1;base<=base+period;
                    building<=0;build_done<=0;
                end
            end else if(input_valid && input_ready)begin
                if(kind==0 || kind==4)begin
                    boundary_packet<=input_packet;boundary_length<=input_length;boundary_index<=0;copying_boundary<=1;
                    if(kind==0)begin
                        run<=input_packet[64 +: 32];base<=request_time;
                        period<=input_packet[44*8 +: 32];mask<=input_packet[40*8 +: 9];
                        frame<=0;opened<=1;fault<=0;diagnostic<=0;raw_count<=0;metadata_count<=0;
                        event_count<=0;dropped<=0;partial<=0;
                        if((LOG_STRIDE!=1 && LOG_STRIDE!=2) || selected_count(input_packet[40*8 +: 9])==0 || selected_count(input_packet[40*8 +: 9])>3)fault<=1;
                    end else opened<=0;
                end else if(kind>=1 && kind<=3 && opened && input_frame==frame
                    && event_count<7 && request_delta<16777216 && completion_delta<16777216
                    && (kind==2 || raw_end_capture<=raw_count))begin
                    if(input_packet[15*8 +: 8]!=0 || input_packet[33*8 +: 8]!=0)diagnostic<=1;
                    event_record<=0;
                    event_record[103:0]<={input_packet[34*8 +: 8],input_packet[33*8 +: 8],
                        input_packet[15*8 +: 8],raw_end_capture,completion_delta[23:0],
                        request_delta[23:0],input_packet[16*8 +: 8],motor,kind};
                    index=0;
                    if(kind==2)for(j=0;j<9;j=j+1)if(mask[j])begin
                        event_record[104+index*8 +: 16]<=input_packet[(36+j*2)*8 +: 16];index=index+2;
                    end
                    copy_length<=13+index;copy_index<=0;copying_event<=1;
                    last_audit_copy<=kind==3 && motor==final_id(mask);
                end else fault<=1;
            end
            if(push)tail<=tail+1==QUEUE_DEPTH ? 0 : tail+1;
            if(pop)head<=head+1==QUEUE_DEPTH ? 0 : head+1;
            case({push,pop})2'b10:count<=count+1;2'b01:count<=count-1;default:;endcase
        end
    end
endmodule
