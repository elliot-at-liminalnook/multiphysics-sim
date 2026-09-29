//! SI dimensions for checking authored equations: exponents of m, kg, s, A,
//! K, mol, cd. Units are written the way the registry writes them
//! (`N·m/A`, `V·s/rad`, `kg·m²`, `1/K`). Only coherent SI units are accepted,
//! so a value's number never needs scaling (write 0.0005 H, not 0.5 mH).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Dim(pub [i8; 7]);

impl Dim {
    pub const NONE: Dim = Dim([0; 7]);
    pub const fn si(p: [i8; 7]) -> Self {
        Dim(p)
    }
    pub fn mul(self, o: Dim) -> Dim {
        let mut r = self.0;
        for i in 0..7 {
            r[i] += o.0[i];
        }
        Dim(r)
    }
    pub fn div(self, o: Dim) -> Dim {
        let mut r = self.0;
        for i in 0..7 {
            r[i] -= o.0[i];
        }
        Dim(r)
    }
    pub fn pow(self, n: i8) -> Dim {
        Dim(self.0.map(|p| p * n))
    }
    pub fn sqrt(self) -> Option<Dim> {
        self.0.iter().all(|p| p % 2 == 0).then(|| Dim(self.0.map(|p| p / 2)))
    }
    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }
}

impl std::fmt::Display for Dim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_none() {
            return write!(f, "1 (dimensionless)");
        }
        let names = ["m", "kg", "s", "A", "K", "mol", "cd"];
        let parts: Vec<String> = self.0.iter().zip(names).filter(|(p, _)| **p != 0).map(|(p, n)| if *p == 1 { n.to_string() } else { format!("{n}^{p}") }).collect();
        write!(f, "{}", parts.join("·"))
    }
}

pub const M: Dim = Dim::si([1, 0, 0, 0, 0, 0, 0]);
pub const KG: Dim = Dim::si([0, 1, 0, 0, 0, 0, 0]);
pub const S: Dim = Dim::si([0, 0, 1, 0, 0, 0, 0]);
pub const A: Dim = Dim::si([0, 0, 0, 1, 0, 0, 0]);
pub const K: Dim = Dim::si([0, 0, 0, 0, 1, 0, 0]);

fn symbol(s: &str) -> Option<Dim> {
    let n = M.mul(KG).div(S.pow(2));
    let j = n.mul(M);
    let w = j.div(S);
    let v = w.div(A);
    Some(match s {
        "1" | "rad" | "sr" | "turn" => Dim::NONE,
        "m" => M,
        "kg" => KG,
        "s" => S,
        "A" => A,
        "K" => K,
        "mol" => Dim::si([0, 0, 0, 0, 0, 1, 0]),
        "cd" => Dim::si([0, 0, 0, 0, 0, 0, 1]),
        "Hz" => S.pow(-1),
        "N" => n,
        "J" => j,
        "W" => w,
        "V" => v,
        "C" => A.mul(S),
        "Ω" | "ohm" => v.div(A),
        "S" => A.div(v),
        "H" => v.mul(S).div(A),
        "F" => A.mul(S).div(v),
        "Wb" => v.mul(S),
        "T" => v.mul(S).div(M.pow(2)),
        "Pa" => n.div(M.pow(2)),
        _ => return None,
    })
}

/// Parse a unit string into a dimension.
pub fn parse(unit: &str) -> Result<Dim, String> {
    let unit = unit.trim();
    if unit.is_empty() {
        return Err("empty unit".into());
    }
    let mut dim = Dim::NONE;
    let mut divide = false;
    let mut token = String::new();
    let flush = |token: &mut String, dim: &mut Dim, divide: bool| -> Result<(), String> {
        if token.is_empty() {
            return Ok(());
        }
        // Split a trailing exponent: m², s^-2, m3.
        let (base, exp) = split_exponent(token)?;
        let d = symbol(&base).ok_or_else(|| {
            let hint = if base.len() > 1 && symbol(&base[base.char_indices().nth(1).unwrap().0..]).is_some() { " (use coherent SI units: no prefixes such as m, k or µ)" } else { "" };
            format!("unknown unit `{base}`{hint}")
        })?;
        let d = d.pow(exp);
        *dim = if divide { dim.div(d) } else { dim.mul(d) };
        token.clear();
        Ok(())
    };
    for c in unit.chars() {
        match c {
            '·' | '*' | '⋅' | ' ' => flush(&mut token, &mut dim, divide)?,
            '/' => {
                flush(&mut token, &mut dim, divide)?;
                divide = true;
            }
            _ => token.push(c),
        }
    }
    flush(&mut token, &mut dim, divide)?;
    Ok(dim)
}

fn split_exponent(token: &str) -> Result<(String, i8), String> {
    let supers = [('⁻', '-'), ('⁰', '0'), ('¹', '1'), ('²', '2'), ('³', '3'), ('⁴', '4')];
    let mut base = String::new();
    let mut exp = String::new();
    let mut in_exp = false;
    for c in token.chars() {
        if let Some((_, plain)) = supers.iter().find(|(s, _)| *s == c) {
            in_exp = true;
            exp.push(*plain);
        } else if c == '^' {
            in_exp = true;
        } else if in_exp {
            exp.push(c);
        } else {
            base.push(c);
        }
    }
    if exp.is_empty() {
        return Ok((base, 1));
    }
    let e: i8 = exp.parse().map_err(|_| format!("bad exponent in `{token}`"))?;
    Ok((base, e))
}

/// Dimension of a registry quantity by its short name (`Current`, `Torque`…).
pub fn quantity(name: &str) -> Option<(Dim, sim_core::QuantityKind)> {
    use sim_core::QuantityKind as Q;
    let q = match name {
        "Dimensionless" => Q::Dimensionless,
        "Time" => Q::Time,
        "Voltage" => Q::Voltage,
        "Current" => Q::Current,
        "Angle" => Q::Angle,
        "AngularVelocity" => Q::AngularVelocity,
        "Torque" => Q::Torque,
        "Length" => Q::Length,
        "LinearVelocity" => Q::LinearVelocity,
        "Force" => Q::Force,
        "LinearAcceleration" => Q::LinearAcceleration,
        "AngularAcceleration" => Q::AngularAcceleration,
        "Energy" => Q::Energy,
        "Power" => Q::Power,
        "Temperature" => Q::Temperature,
        "HeatFlow" => Q::HeatFlow,
        "Pressure" => Q::Pressure,
        "Frequency" => Q::Frequency,
        "Mass" => Q::Mass,
        "MagneticFlux" => Q::MagneticFlux,
        _ => return None,
    };
    Some((parse(q.unit()).ok()?, q))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn units_parse_to_si_dimensions() {
        assert_eq!(parse("N·m/A").unwrap(), parse("V·s/rad").unwrap(), "k_t and k_e share a dimension");
        assert_eq!(parse("Ω").unwrap(), parse("V/A").unwrap());
        assert_eq!(parse("kg·m²").unwrap(), parse("N·m·s^2").unwrap());
        assert_eq!(parse("1/K").unwrap(), K.pow(-1));
        assert!(parse("mH").unwrap_err().contains("prefixes"));
        assert!(parse("furlong").is_err());
    }
}
