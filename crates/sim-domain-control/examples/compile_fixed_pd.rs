//! Emit synthesizable controller logic and deterministic cross-backend vectors.
use sim_domain_control::fixed_pd::{Gains, step, verilog};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).ok_or("output directory")?);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("fixed_pd.v"), verilog())?;
    for (mode,bounded) in [(0,false),(1,true),(2,true)] {
        let mut tb = String::from(
            "module fixed_pd_tb; reg clk=0;always #1 clk=~clk; reg signed [31:0] target,position,previous,delta,kp,kd,kv,limit; wire signed [15:0] duty,piped; fixed_pd dut(target,position,previous,delta,kp,kd,kv,limit,duty);fixed_pd_pipeline pipeline(clk,target,position,previous,delta,kp,kd,kv,limit,piped); initial begin @(negedge clk);\n",
        );
        if bounded {
            tb=tb.replace("module fixed_pd_tb;", "module fixed_pd_bounded_tb;").replace(
            "pipeline(clk,target,position,previous,delta,kp,kd,kv,limit,piped)",
            "pipeline(clk,$signed({20'd0,target[11:0]}),$signed({20'd0,position[11:0]}),$signed({20'd0,previous[11:0]}),{{25{delta[6]}},delta[6:0]},$signed({19'd0,kp[12:0]}),$signed({19'd0,kd[12:0]}),$signed({19'd0,kv[12:0]}),$signed({22'd0,limit[9:0]}),piped)");
        }
        let mut seed = 42u32;
        let mut next = || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            seed
        };
        for i in 0..4096 {
            let mut target = (next() % 4096) as u16;
            let mut position = (next() % 4096) as u16;
            let mut previous = (next() % 4096) as u16;
            let mut delta = (next() % 8191) as i16 - 4095;
            let mut g = Gains {
                kp_q8: (next() % 4097) as u16,
                kd_q8: (next() % 4097) as u16,
                kv_q8: (next() % 4097) as u16,
                limit: (next() % 1001) as u16,
            };
            if bounded {
                delta = (next() % 65) as i16 - 32;
                g.limit = (next() % 1001) as u16;
                if i < 64 {
                    target = if i & 1 == 0 { 0 } else { 4095 };
                    position = if i & 2 == 0 { 0 } else { 4095 };
                    previous = if i & 4 == 0 { 0 } else { 4095 };
                    delta = if i & 8 == 0 { -32 } else { 32 };
                    g.kp_q8 = if i & 16 == 0 { 0 } else { 4096 };
                    g.kd_q8 = if i & 32 == 0 { 0 } else { 4096 };
                    g.kv_q8 = 4096;
                    g.limit = 1000;
                } else if i % 2 == 0 {
                    position = 2048;
                    target = 2045 + (next() % 7) as u16;
                    previous = 2045 + (next() % 7) as u16;
                    delta = (next() % 7) as i16 - 3;
                    g.kp_q8 %= 256;
                    g.kd_q8 %= 256;
                    g.kv_q8 %= 256;
                }
            }
            if mode==2 {
                let choices=[0,64,128,256,512,1024,2048,4096];
                g.kp_q8=choices[next() as usize%8];g.kd_q8=choices[next() as usize%8];g.kv_q8=choices[next() as usize%8];g.validate_power2()?;
            }
            let want = step(g, target, position, previous, delta)?;
            tb.push_str(&format!("target={target};position={position};previous={previous};delta={delta};kp={};kd={};kv={};limit={};repeat(20) @(negedge clk);if(duty !== 16'sh{:04x} || piped !== duty) $fatal(1,\"vector {i}: got %0d piped %0d expected {want}\",duty,piped);\n",g.kp_q8,g.kd_q8,g.kv_q8,g.limit,want as u16));
        }
        tb.push_str("$display(\"PASS 4096 Rust/RTL vectors, signed quantization and saturation\");$finish;end endmodule\n");
        if bounded {
            tb = tb.replace(
                "PASS 4096 Rust/RTL vectors",
                "PASS 4096 bounded-width Rust/RTL vectors",
            );
        }
        if mode==2 {tb=tb.replace("fixed_pd_bounded_tb","fixed_pd_power2_tb").replace("fixed_pd_pipeline pipeline","fixed_pd_power2_pipeline pipeline");}
        std::fs::write(
            dir.join(if mode==2 {"fixed_pd_power2_tb.v"} else if bounded {
                "fixed_pd_bounded_tb.v"
            } else {
                "fixed_pd_tb.v"
            }),
            tb,
        )?;
    }
    Ok(())
}
