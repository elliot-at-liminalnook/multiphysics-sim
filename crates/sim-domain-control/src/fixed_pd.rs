//! Integer PD plus velocity feedforward, shared by Rust and synthesized RTL.
//! Positions and per-tick displacements are encoder counts; output is PWM counts.
//! This is a discrete-time controller: acquisition and command times belong in the recording.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gains {
    pub kp_q8: u16,
    pub kd_q8: u16,
    pub kv_q8: u16,
    pub limit: u16,
}
impl Gains {
    /// Discrete tunable FPGA profile; same integer controller law and rounding.
    pub fn validate_power2(self)->Result<(),String>{
        self.validate()?;
        if [self.kp_q8,self.kd_q8,self.kv_q8].iter().any(|x|![0,64,128,256,512,1024,2048,4096].contains(x)){return Err("FPGA gain must be zero or a power of two from 64..4096".into());}
        Ok(())
    }

    pub fn validate(self) -> Result<(), String> {
        if self.kp_q8 > 4096 || self.kd_q8 > 4096 || self.kv_q8 > 4096 || self.limit > 1000 {
            return Err("Fixed PD gains exceed bounded integer arithmetic contract".into());
        }
        Ok(())
    }
}

// A small signed integer expression graph is the single source for both execution
// and RTL generation. Every intermediate fits signed 32 bits over validated inputs.
enum Expr {
    Input(&'static str, usize),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}
impl Expr {
    fn pipeline(&self, statements: &mut Vec<String>) -> String {self.pipeline_mode(statements,false)}
    fn pipeline_mode(&self, statements: &mut Vec<String>,power2:bool) -> String {
        if let Self::Input(name, _) = self {
            return name.to_string();
        }
        let (a, b, op) = match self {
            Self::Add(a, b) => (a, b, "+"),
            Self::Sub(a, b) => (a, b, "-"),
            Self::Mul(a, b) => (a, b, "*"),
            _ => unreachable!(),
        };
        let a = a.pipeline_mode(statements,power2);
        let b = b.pipeline_mode(statements,power2);
        let n = format!("node_{}", statements.len());
        if op=="*" && power2 {
            let cases=(6..=12).map(|shift|format!("32'sd{}: {n} <= {b} <<< {shift};",1<<shift)).collect::<Vec<_>>().join("\n");
            statements.push(format!("reg signed [31:0] {n}; always @(posedge clk) begin case({a})\n{cases}\ndefault: {n} <= 0; endcase end"));
        } else if op == "*" {
            // Four short multipliers and a registered adder tree avoid a full-width
            // multiplier critical path. Truncation modulo 2^32 preserves signed products.
            statements.push(format!("wire signed [31:0] {n}_a={a}, {n}_b={b};\nreg signed [31:0] {n}_p0,{n}_p1,{n}_p2,{n}_p3,{n}_s0,{n}_s1,{n};\nalways @(posedge clk) begin\n{n}_p0 <= $signed({{1'b0,{n}_a[7:0]}})*{n}_b;\n{n}_p1 <= ($signed({{1'b0,{n}_a[15:8]}})*{n}_b) <<< 8;\n{n}_p2 <= ($signed({{1'b0,{n}_a[23:16]}})*{n}_b) <<< 16;\n{n}_p3 <= ($signed({n}_a[31:24])*{n}_b) <<< 24;\n{n}_s0 <= {n}_p0+{n}_p1; {n}_s1 <= {n}_p2+{n}_p3; {n} <= {n}_s0+{n}_s1;\nend"));
        } else {
            statements.push(format!(
                "reg signed [31:0] {n}; always @(posedge clk) {n} <= {a} {op} {b};"
            ));
        }
        n
    }
    fn eval(&self, x: &[i32; 7]) -> i32 {
        match self {
            Self::Input(_, i) => x[*i],
            Self::Add(a, b) => a.eval(x) + b.eval(x),
            Self::Sub(a, b) => a.eval(x) - b.eval(x),
            Self::Mul(a, b) => a.eval(x) * b.eval(x),
        }
    }
    fn rtl(&self) -> String {
        match self {
            Self::Input(n, _) => n.to_string(),
            Self::Add(a, b) => format!("({} + {})", a.rtl(), b.rtl()),
            Self::Sub(a, b) => format!("({} - {})", a.rtl(), b.rtl()),
            Self::Mul(a, b) => format!("({} * {})", a.rtl(), b.rtl()),
        }
    }
}
fn law() -> Expr {
    use Expr::*;
    let x = |n, i| Box::new(Input(n, i));
    Add(
        Box::new(Add(
            Box::new(Mul(
                x("kp", 4),
                Box::new(Sub(x("target", 0), x("position", 1))),
            )),
            Box::new(Mul(
                x("kd", 5),
                Box::new(Sub(
                    x("delta", 3),
                    Box::new(Sub(x("position", 1), x("previous", 2))),
                )),
            )),
        )),
        Box::new(Mul(x("kv", 6), x("delta", 3))),
    )
}
pub fn step(
    g: Gains,
    target: u16,
    position: u16,
    previous: u16,
    delta: i16,
) -> Result<i16, String> {
    g.validate()?;
    if [target, position, previous].iter().any(|x| *x > 4095) || delta.unsigned_abs() > 4095 {
        return Err("Fixed PD inputs must be non-wrapping encoder counts".into());
    }
    let raw = law().eval(&[
        target.into(),
        position.into(),
        previous.into(),
        delta.into(),
        g.kp_q8.into(),
        g.kd_q8.into(),
        g.kv_q8.into(),
    ]);
    // Division truncates toward zero on both backends, including negative commands.
    Ok((raw / 256).clamp(-(g.limit as i32), g.limit as i32) as i16)
}
/// The same law and rounding on count differences, for a multi-turn encoder
/// whose absolute count no longer fits 12 bits. `error` = target − position,
/// `moved` = position − previous, `delta` = target step; each is bounded like
/// the single-turn inputs, so every intermediate still fits signed 32 bits.
pub fn step_differences(g: Gains, error: i32, moved: i32, delta: i32) -> Result<i16, String> {
    g.validate()?;
    if [error, moved, delta].iter().any(|x| x.unsigned_abs() > 4095) {
        return Err("Fixed PD differences must stay within 4095 encoder counts".into());
    }
    let raw = law().eval(&[
        error,
        0,
        -moved,
        delta,
        g.kp_q8.into(),
        g.kd_q8.into(),
        g.kv_q8.into(),
    ]);
    Ok((raw / 256).clamp(-(g.limit as i32), g.limit as i32) as i16)
}
pub fn verilog() -> String {
    let mut statements = vec![];
    let numerator = law().pipeline(&mut statements);
    let mut power_statements=vec![];
    let power_numerator=law().pipeline_mode(&mut power_statements,true);
    let power_module=format!("// Requires validated power-of-two gains; same shared expression graph.\nmodule fixed_pd_power2_pipeline(input clk,input signed [31:0] target,position,previous,delta,kp,kd,kv,limit,output reg signed [15:0] duty);\n{}\nreg signed [31:0] raw;always @(posedge clk) begin raw <= {power_numerator}/32'sd256;duty <= raw>limit ? limit : (raw< -limit ? -limit : raw);end\nendmodule\n",power_statements.join("\n"));
    format!(
        "// GENERATED by sim-domain-control::fixed_pd::verilog. Do not edit.\nmodule fixed_pd(input signed [31:0] target,position,previous,delta,kp,kd,kv,limit, output signed [15:0] duty);\nwire signed [31:0] numerator = {};\nwire signed [31:0] raw = numerator / 32'sd256;\nassign duty = raw > limit ? limit : (raw < -limit ? -limit : raw);\nendmodule\n// Inputs must remain stable for 20 rising edges before consuming duty.\nmodule fixed_pd_pipeline(input clk,input signed [31:0] target,position,previous,delta,kp,kd,kv,limit,output reg signed [15:0] duty);\n{}\nreg signed [31:0] raw;always @(posedge clk) begin raw <= {numerator}/32'sd256;duty <= raw>limit ? limit : (raw< -limit ? -limit : raw);end\nendmodule\n",
        law().rtl(),
        statements.join("\n")
    ) + &power_module
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_quantization_and_saturation() {
        let g = Gains {
            kp_q8: 256,
            kd_q8: 0,
            kv_q8: 0,
            limit: 50,
        };
        assert_eq!(step(g, 2001, 2000, 2000, 0).unwrap(), 1);
        assert_eq!(step(g, 1900, 2000, 2000, 0).unwrap(), -50);
        assert_eq!(
            step(Gains { kp_q8: 1, ..g }, 1999, 2000, 2000, 0).unwrap(),
            0
        );
        assert!(step(g, 4096, 2000, 2000, 0).is_err());
    }
}
