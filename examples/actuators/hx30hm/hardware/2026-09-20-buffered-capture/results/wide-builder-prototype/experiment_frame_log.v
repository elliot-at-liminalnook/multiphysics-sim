// Bounded, lossless frame capture, independent of host-UART serialization.
// Explicit three-selected-axis profile; any IDs 4..12, never robot-specific.
// Valid raw replies occur once on the wire; compact events reference their ends.
// Unknown/partial/corrupt raw bytes are retained too. Overflow is explicit and
// faults the independent supervisor; already queued evidence still drains.
module experiment_frame_log #(
    parameter QUEUE_DEPTH=3
)(
    input wire clk,rst,
    input wire raw_valid,input wire [7:0] raw_data,
    input wire event_pulse,
    input wire input_valid,output wire input_ready,
    input wire [511:0] input_packet,input wire [6:0] input_length,
    output wire output_valid,input wire output_ready,
    output wire [2047:0] output_packet,output wire [8:0] output_length,
    output wire busy,output wire capture_active,output reg fault=0
);
    reg [2047:0] queue[0:QUEUE_DEPTH-1];
    reg [8:0] lengths[0:QUEUE_DEPTH-1];
    integer head=0,tail=0,count=0;
    reg opened=0,flush_pending=0,partial=0;
    reg [1023:0] raw=0;
    reg [871:0] metadata=0; // 7*13 bytes plus at most 18 control bytes.
    reg [7:0] raw_count=0,raw_end_capture=0,metadata_count=0,event_count=0;
    reg [15:0] dropped=0;
    reg [31:0] run=0,period=0;
    reg [63:0] origin=0;
    reg [15:0] frame=0;
    reg [8:0] mask=0;
    wire [7:0] kind=input_packet[7*8 +: 8];
    wire [7:0] motor=input_packet[14*8 +: 8];
    wire [15:0] input_frame=input_packet[12*8 +: 16];
    wire [63:0] request_time=input_packet[17*8 +: 64];
    wire [63:0] completion_time=input_packet[25*8 +: 64];
    wire [63:0] base=origin+frame*{32'd0,period};
    wire [63:0] request_delta=request_time-base,completion_delta=completion_time-base;
    wire pop=output_valid && output_ready;
    wire terminal_flush=input_valid && kind==4 && (raw_count!=0 || event_count!=0 || dropped!=0);
    assign input_ready=!flush_pending && !terminal_flush && count<QUEUE_DEPTH;
    assign output_valid=count!=0;
    assign output_packet=queue[head];
    assign output_length=lengths[head];
    assign capture_active=opened;
    assign busy=opened || flush_pending || count!=0;
    function [3:0] selected_count;
        input [8:0] value;integer k;
        begin selected_count=0;for(k=0;k<9;k=k+1)selected_count=selected_count+value[k];end
    endfunction
    function [7:0] final_id;
        input [8:0] value;integer k;
        begin final_id=0;for(k=0;k<9;k=k+1)if(value[k])final_id=k+4;end
    endfunction
    reg [2047:0] assembled;
    integer i,n,index;
    reg [7:0] checksum;
    // Assembly is off the control path. The complete packet is retained until
    // the bridge has reserved space and copied its final byte.
    always @* begin
        assembled=0;n=30+raw_count+metadata_count;
        assembled[0 +: 16]=16'hffff;assembled[16 +: 8]=253;
        assembled[24 +: 8]=n-4;assembled[40 +: 8]=2;
        assembled[48 +: 8]=8'ha4;assembled[56 +: 8]=1;
        assembled[64 +: 32]=run;assembled[96 +: 16]=frame;
        assembled[112 +: 64]=base;
        assembled[176 +: 8]=raw_count;assembled[184 +: 8]=event_count;
        assembled[192 +: 16]={7'd0,mask};
        assembled[208 +: 8]={6'd0,(dropped!=0),partial};
        assembled[216 +: 16]=dropped;
        for(i=0;i<128;i=i+1)if(i<raw_count)assembled[(29+i)*8 +: 8]=raw[i*8 +: 8];
        for(i=0;i<109;i=i+1)if(i<metadata_count)assembled[(29+raw_count+i)*8 +: 8]=metadata[i*8 +: 8];
        checksum=0;for(i=2;i<255;i=i+1)if(i<n-1)checksum=checksum+assembled[i*8 +: 8];
        if(n<=256)assembled[(n-1)*8 +: 8]=~checksum;
    end
    reg push;
    integer j,at;
    always @(posedge clk) begin
        push=0;
        if(rst) begin
            head<=0;tail<=0;count<=0;opened<=0;flush_pending<=0;fault<=0;
            raw_count<=0;metadata_count<=0;event_count<=0;dropped<=0;partial<=0;
        end else begin
            if(event_pulse)raw_end_capture<=raw_count+(raw_valid ? 1 : 0);
            if(raw_valid && opened) begin
                if(raw_count<128)begin raw[raw_count*8 +: 8]<=raw_data;raw_count<=raw_count+1;end
                else begin fault<=1;if(dropped!=65535)dropped<=dropped+1;end
            end
            if(terminal_flush)begin flush_pending<=1;partial<=1;end
            if(flush_pending && count<QUEUE_DEPTH && !raw_valid)begin
                if(n>256)fault<=1;
                else begin
                    queue[tail]<=assembled;lengths[tail]<=n;push=1;
                    raw_count<=0;metadata_count<=0;event_count<=0;dropped<=0;
                    flush_pending<=0;partial<=0;frame<=frame+1;
                end
            end else if(input_valid && input_ready)begin
                if(kind==0 || kind==4)begin
                    queue[tail]<={1536'd0,input_packet};lengths[tail]<=input_length;push=1;
                    if(kind==0)begin
                        run<=input_packet[64 +: 32];origin<=request_time;
                        period<=input_packet[44*8 +: 32];mask<=input_packet[40*8 +: 9];
                        frame<=0;opened<=1;fault<=0;raw_count<=0;metadata_count<=0;
                        event_count<=0;dropped<=0;partial<=0;
                        if(selected_count(input_packet[40*8 +: 9])==0 || selected_count(input_packet[40*8 +: 9])>3)fault<=1;
                    end else opened<=0;
                end else if(kind>=1 && kind<=3 && opened && input_frame==frame
                    && event_count<7 && request_delta<16777216 && completion_delta<16777216
                    && (kind==2 || raw_end_capture<=raw_count))begin
                    at=metadata_count;
                    metadata[(at+0)*8 +: 8]<=kind;
                    metadata[(at+1)*8 +: 8]<=motor;
                    metadata[(at+2)*8 +: 8]<=input_packet[16*8 +: 8];
                    metadata[(at+3)*8 +: 24]<=request_delta[23:0];
                    metadata[(at+6)*8 +: 24]<=completion_delta[23:0];
                    metadata[(at+9)*8 +: 8]<=raw_end_capture;
                    metadata[(at+10)*8 +: 8]<=input_packet[15*8 +: 8];
                    metadata[(at+11)*8 +: 8]<=input_packet[33*8 +: 8];
                    metadata[(at+12)*8 +: 8]<=input_packet[34*8 +: 8];
                    index=13;
                    if(kind==2)for(j=0;j<9;j=j+1)if(mask[j])begin
                        metadata[(at+index)*8 +: 16]<=input_packet[(36+j*2)*8 +: 16];index=index+2;
                    end
                    metadata_count<=metadata_count+index;event_count<=event_count+1;
                    if(kind==3 && motor==final_id(mask))flush_pending<=1;
                end else fault<=1;
            end
            if(push)tail<=tail+1==QUEUE_DEPTH ? 0 : tail+1;
            if(pop)head<=head+1==QUEUE_DEPTH ? 0 : head+1;
            case({push,pop})2'b10:count<=count+1;2'b01:count<=count-1;default:;endcase
        end
    end
endmodule
