`timescale 1ns/1ps
module trajectory_store_tb;
    reg clk=0;always #5 clk=~clk;
    reg rst=1,active=0,configure=0,write_row=0,seal=0;
    reg [8:0] input_mask=0,input_frames=0,write_index=0,read_index=0;
    reg [31:0] input_period=0,expected_crc32=0;
    reg [15:0] input_kp=0,input_kd=0,input_kv=0,input_limit=0;
    reg [143:0] input_homes=0;
    reg [287:0] write_data=0;
    wire [8:0] mask,frames,written;
    wire [31:0] period,crc32;
    wire [15:0] kp,kd,kv,duty_limit;
    wire [143:0] homes,row_targets,row_deltas;
    wire valid,fault,busy,row_ready;
    trajectory_store dut(.clk(clk),.rst(rst),.active(active),.invalidate_plan(1'b0),.configure(configure),.write_row(write_row),.seal(seal),
        .input_mask(input_mask),.input_frames(input_frames),.input_period(input_period),
        .input_kp(input_kp),.input_kd(input_kd),.input_kv(input_kv),.input_limit(input_limit),.input_homes(input_homes),
        .write_index(write_index),.read_index(read_index),.write_data(write_data),.expected_crc32(expected_crc32),
        .mask(mask),.frames(frames),.period(period),.kp(kp),.kd(kd),.kv(kv),.duty_limit(duty_limit),.homes(homes),
        .valid(valid),.fault(fault),.busy(busy),.row_ready(row_ready),.row_targets(row_targets),.row_deltas(row_deltas),
        .crc32(crc32),.written(written));
    reg [271:0] vector_header[0:0];
    reg [31:0] vector_crc[0:0];
    reg [287:0] vector_rows[0:255],rows[0:255];
    reg [1023:0] directory;
    reg [271:0] h;
    reg [31:0] c;
    reg [287:0] before_write;
    integer i,j,k,groups=0,saw_fault=0,unused;
    always @(posedge clk) if(fault) saw_fault=saw_fault+1;
    task reset_case;
        begin
            @(negedge clk);rst=1;active=0;configure=0;write_row=0;seal=0;read_index=0;
            repeat(2) @(negedge clk);rst=0;saw_fault=0;
            h=vector_header[0];
            input_mask=h[8:0];input_frames=h[24:16];input_period=h[63:32];
            input_kp=h[79:64];input_kd=h[95:80];input_kv=h[111:96];input_limit=h[127:112];input_homes=h[271:128];
            expected_crc32=vector_crc[0];
            for(i=0;i<256;i=i+1) rows[i]=vector_rows[i];
        end
    endtask
    task apply_config;
        begin @(negedge clk);configure=1;@(negedge clk);configure=0;end
    endtask
    task row;
        input [8:0] index;
        begin @(negedge clk);write_index=index;write_data=rows[index];write_row=1;@(negedge clk);write_row=0;end
    endtask
    task upload;
        begin apply_config;for(j=0;j<input_frames;j=j+1) row(j);end
    endtask
    task sealing;
        begin @(negedge clk);seal=1;@(negedge clk);seal=0;end
    endtask
    task done;
        begin while(busy) @(negedge clk);@(negedge clk);end
    endtask
    task rejected;
        begin done;if(valid || saw_fault==0 || row_ready) $fatal(1,"unsafe plan not rejected group=%0d",groups);end
    endtask
    task accepted;
        begin done;if(!valid || saw_fault!=0 || crc32!=expected_crc32) $fatal(1,"valid plan rejected group=%0d",groups);end
    endtask
    // Independently accumulate CRC using individual byte/bit loops, not DUT function.
    task checksum;
        integer n,b,bit_index;
        reg [7:0] byte_value;
        begin
            h={input_homes,input_limit,input_kv,input_kd,input_kp,input_period,7'd0,input_frames,7'd0,input_mask};
            c=32'hffffffff;
            for(n=0;n<34+36*input_frames;n=n+1) begin
                if(n<34) byte_value=h[n*8 +: 8];
                else begin b=(n-34)%36;byte_value=rows[(n-34)/36][b*8 +: 8];end
                for(bit_index=0;bit_index<8;bit_index=bit_index+1) begin
                    if(c[0]^byte_value[bit_index]) c=(c>>1)^32'hedb88320;else c=c>>1;
                end
            end
            expected_crc32=~c;
        end
    endtask
    initial begin
        directory="tb/trajectory-v1";unused=$value$plusargs("VECTORS=%s",directory);
        $readmemh({directory,"/header.hex"},vector_header);
        $readmemh({directory,"/crc.hex"},vector_crc);
        $readmemh({directory,"/rows.hex"},vector_rows,0,vector_header[0][24:16]-1);
        reset_case;checksum;if(expected_crc32!=vector_crc[0]) $fatal(1,"Rust/RTL canonical CRC mismatch");
        upload;sealing;accepted;
        for(j=0;j<input_frames;j=j+1) begin
            read_index=j;#1;
            if(j!=0 && row_ready) $fatal(1,"stale RAM data advertised ready");
            @(negedge clk);
            if(!row_ready) $fatal(1,"synchronous row not ready");
            for(k=0;k<9;k=k+1)
                if(row_targets[k*16 +: 16]!==rows[j][k*32 +: 16] || row_deltas[k*16 +: 16]!==rows[j][k*32+16 +: 16]) $fatal(1,"row decode mismatch");
        end
        read_index=256;#1;if(row_ready) $fatal(1,"out-of-range RAM alias ready");groups=groups+1;
        reset_case;upload;expected_crc32=expected_crc32^1;sealing;rejected;groups=groups+1;
        reset_case;apply_config;row(0);sealing;rejected;groups=groups+1;
        reset_case;apply_config;row(1);rejected;
        reset_case;apply_config;row(0);row(0);rejected;groups=groups+1;
        reset_case;row(0);rejected;groups=groups+1;
        reset_case;rows[1][31:16]=31;checksum;upload;sealing;rejected;groups=groups+1;
        reset_case;rows[0][15:0]=input_homes[15:0]+1;rows[0][31:16]=1;checksum;upload;sealing;rejected;groups=groups+1;
        reset_case;rows[1][15:0]=input_homes[15:0]+81;rows[1][31:16]=81;checksum;upload;sealing;rejected;
        reset_case;rows[1][15:0]=4096;checksum;upload;sealing;rejected;
        reset_case;rows[1][15:0]=input_homes[15:0]+33;rows[1][31:16]=33;checksum;upload;sealing;rejected;groups=groups+1;
        reset_case;input_kp=4097;apply_config;rejected;
        reset_case;input_kd=4097;apply_config;rejected;
        reset_case;input_kv=4097;apply_config;rejected;
        reset_case;input_limit=101;apply_config;rejected;
        reset_case;input_frames=1;apply_config;rejected;
        reset_case;input_frames=257;apply_config;rejected;
        reset_case;input_frames=256;input_period=7500000;apply_config;rejected;
        reset_case;input_period=1999999;apply_config;rejected;
        reset_case;input_period=7500001;apply_config;rejected;
        reset_case;input_mask=0;apply_config;rejected;groups=groups+1;
        reset_case;input_homes[15:0]=599;checksum;upload;sealing;rejected;
        reset_case;input_homes[15:0]=3496;checksum;upload;sealing;rejected;groups=groups+1;
        reset_case;input_mask=1;checksum;upload;sealing;rejected;
        reset_case;input_mask=1;input_homes[143:16]=0;
        for(j=0;j<input_frames;j=j+1) rows[j][287:32]=0;
        checksum;upload;sealing;accepted;groups=groups+1;
        reset_case;upload;sealing;accepted;input_mask=1;input_kp=0;input_period=0;input_homes=0;
        repeat(3) @(negedge clk);
        if(mask!=vector_header[0][8:0] || kp!=vector_header[0][79:64] || homes!=vector_header[0][271:128] || !valid) $fatal(1,"configuration not frozen");groups=groups+1;
        for(k=0;k<3;k=k+1) begin
            reset_case;upload;sealing;accepted;before_write=dut.memory[0];active=1;
            if(k==0) apply_config;else if(k==1) begin rows[0]=0;row(0);end else sealing;
            rejected;if(dut.memory[0]!==before_write) $fatal(1,"active mutation changed RAM");
        end
        groups=groups+1;
        reset_case;upload;sealing;before_write=dut.memory[0];rows[0]=0;row(0);rejected;
        if(dut.memory[0]!==before_write) $fatal(1,"busy mutation changed RAM");groups=groups+1;
        reset_case;upload;sealing;accepted;apply_config;sealing;rejected;groups=groups+1;
        reset_case;input_frames=256;input_period=2000000;
        for(j=0;j<256;j=j+1) begin rows[j]=0;for(k=0;k<9;k=k+1) rows[j][k*32 +: 16]=input_homes[k*16 +: 16];end
        checksum;upload;if(written!=256) $fatal(1,"256 rows overflow");sealing;accepted;
        read_index=255;@(negedge clk);if(!row_ready || row_targets!=homes || row_deltas!=0) $fatal(1,"last row wrong");groups=groups+1;
        reset_case;apply_config;before_write=dut.memory[0];@(negedge clk);write_row=1;configure=1;write_index=0;write_data=0;
        @(negedge clk);write_row=0;configure=0;rejected;if(dut.memory[0]!==before_write) $fatal(1,"collision wrote RAM");groups=groups+1;
        reset_case;upload;sealing;accepted;before_write=dut.memory[0];rows[0]=0;row(0);rejected;
        if(dut.memory[0]!==before_write) $fatal(1,"sealed plan overwritten");groups=groups+1;
        reset_case;apply_config;before_write=dut.memory[0];@(negedge clk);rst=1;write_row=1;write_index=0;write_data=0;
        @(negedge clk);write_row=0;rst=0;
        if(valid || row_ready || written!=0 || dut.memory[0]!==before_write) $fatal(1,"reset allowed write or retained validity");groups=groups+1;
        $display("PASS trajectory store: %0d groups, Rust vectors and 256-row capacity",groups);$finish;
    end
    initial begin repeat(100000) @(negedge clk);$fatal(1,"test timeout");end
endmodule
