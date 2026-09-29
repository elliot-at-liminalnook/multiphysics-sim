//! Quantities with units in answers and prose: "24 mN·m", "4300 rpm",
//! "3.3 A", "0.012 N·m/A". Units are a factor to SI and a dimension vector
//! (m, kg, s, A, K); angles are dimensionless in radians, so rpm converts to
//! rad/s. Used to accept an answer in any sensible unit and to explain the
//! common slips (a factor of 1000, rpm for rad/s, turns for radians).
use std::f64::consts::PI;

/// Exponents of metre, kilogram, second, ampere, kelvin.
pub type Dims = [i8; 5];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unit {
    /// Multiply a value in this unit by `factor` to get SI.
    pub factor: f64,
    pub dims: Dims,
}
impl Unit {
    pub const ONE: Unit = Unit { factor: 1.0, dims: [0; 5] };
    fn mul(self, o: Unit, sign: i8) -> Unit {
        let mut dims = self.dims;
        for (d, e) in dims.iter_mut().zip(o.dims) {
            *d += sign * e;
        }
        Unit { factor: if sign > 0 { self.factor * o.factor } else { self.factor / o.factor }, dims }
    }
    fn pow(self, n: i8) -> Unit {
        Unit { factor: self.factor.powi(n as i32), dims: self.dims.map(|d| d * n) }
    }
    /// A name for the kind of quantity, for feedback ("a torque").
    pub fn kind(&self) -> &'static str {
        match self.dims {
            [0, 0, 0, 0, 0] => "a pure number or an angle",
            [1, 0, 0, 0, 0] => "a length",
            [0, 1, 0, 0, 0] => "a mass",
            [0, 0, 1, 0, 0] => "a time",
            [0, 0, 0, 1, 0] => "a current",
            [0, 0, 0, 0, 1] => "a temperature",
            [0, 0, -1, 0, 0] => "an angular speed or a frequency",
            [1, 0, -1, 0, 0] => "a speed",
            [1, 1, -2, 0, 0] => "a force",
            [2, 1, -2, 0, 0] => "a torque or an energy",
            [2, 1, -3, 0, 0] => "a power",
            [2, 1, -3, -1, 0] => "a voltage",
            [2, 1, -3, -2, 0] => "a resistance",
            [2, 1, -2, -1, 0] => "a torque constant (N·m/A = V·s/rad)",
            [2, 1, 0, 0, 0] => "an inertia",
            _ => "a different kind of quantity",
        }
    }
}

const fn u(factor: f64, dims: Dims) -> Unit {
    Unit { factor, dims }
}

/// Base symbols (before prefixes). Longest names are tried first.
const BASE: &[(&str, Unit)] = &[
    ("rpm", u(2.0 * PI / 60.0, [0, 0, -1, 0, 0])),
    ("rev", u(2.0 * PI, [0; 5])),
    ("turn", u(2.0 * PI, [0; 5])),
    ("rad", u(1.0, [0; 5])),
    ("deg", u(PI / 180.0, [0; 5])),
    ("°", u(PI / 180.0, [0; 5])),
    ("ohm", u(1.0, [2, 1, -3, -2, 0])),
    ("Ω", u(1.0, [2, 1, -3, -2, 0])),
    ("min", u(60.0, [0, 0, 1, 0, 0])),
    ("Hz", u(1.0, [0, 0, -1, 0, 0])),
    ("Wh", u(3600.0, [2, 1, -2, 0, 0])),
    ("m", u(1.0, [1, 0, 0, 0, 0])),
    ("g", u(1e-3, [0, 1, 0, 0, 0])),
    ("s", u(1.0, [0, 0, 1, 0, 0])),
    ("h", u(3600.0, [0, 0, 1, 0, 0])),
    ("A", u(1.0, [0, 0, 0, 1, 0])),
    ("K", u(1.0, [0, 0, 0, 0, 1])),
    ("N", u(1.0, [1, 1, -2, 0, 0])),
    ("J", u(1.0, [2, 1, -2, 0, 0])),
    ("W", u(1.0, [2, 1, -3, 0, 0])),
    ("V", u(1.0, [2, 1, -3, -1, 0])),
    ("C", u(1.0, [0, 0, 1, 1, 0])),
    ("F", u(1.0, [-2, -1, 4, 2, 0])),
    ("H", u(1.0, [2, 1, -2, -2, 0])),
    ("T", u(1.0, [0, 1, -2, -1, 0])),
    ("Pa", u(1.0, [-1, 1, -2, 0, 0])),
    ("%", u(0.01, [0; 5])),
];
const PREFIX: &[(&str, f64)] = &[("G", 1e9), ("M", 1e6), ("k", 1e3), ("c", 1e-2), ("m", 1e-3), ("µ", 1e-6), ("μ", 1e-6), ("u", 1e-6), ("n", 1e-9)];

fn base(symbol: &str) -> Option<Unit> {
    if let Some((_, u)) = BASE.iter().find(|(s, _)| *s == symbol) {
        return Some(*u);
    }
    // Degrees Celsius only as a temperature difference here.
    if symbol == "°C" {
        return Some(u(1.0, [0, 0, 0, 0, 1]));
    }
    for (p, f) in PREFIX {
        if let Some(rest) = symbol.strip_prefix(p) {
            if let Some((_, u)) = BASE.iter().find(|(s, _)| *s == rest && !matches!(*s, "rpm" | "rev" | "turn" | "deg" | "°" | "%" | "min" | "h")) {
                return Some(Unit { factor: u.factor * f, dims: u.dims });
            }
        }
    }
    None
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '⁰' => '0',
        '¹' => '1',
        '²' => '2',
        '³' => '3',
        '⁴' => '4',
        '⁻' => '-',
        _ => return None,
    })
}

/// Parse a unit expression: `N·m`, `mN·m`, `rad/s`, `V·s/rad`, `m/s^2`,
/// `m/s²`, `kg·m²`. An empty string is dimensionless.
pub fn parse_unit(text: &str) -> Result<Unit, String> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Unit::ONE);
    }
    let mut unit = Unit::ONE;
    let mut sign = 1i8;
    let mut term = String::new();
    let flush = |term: &mut String, sign: i8, unit: &mut Unit| -> Result<(), String> {
        if term.is_empty() {
            return Ok(());
        }
        let raw = std::mem::take(term);
        // Exponent: `^2`, `^-1`, or superscript digits.
        let (name, exp) = if let Some((n, e)) = raw.split_once('^') {
            (n.to_string(), e.parse::<i8>().map_err(|_| format!("bad exponent in `{raw}`"))?)
        } else if raw.chars().last().is_some_and(|c| superscript(c).is_some()) {
            let digits: String = raw.chars().rev().take_while(|c| superscript(*c).is_some()).collect::<Vec<_>>().into_iter().rev().filter_map(superscript).collect();
            let name: String = raw.chars().take(raw.chars().count() - digits.chars().count()).collect();
            (name, digits.parse::<i8>().map_err(|_| format!("bad exponent in `{raw}`"))?)
        } else {
            (raw.clone(), 1)
        };
        let b = base(&name).ok_or_else(|| format!("unknown unit `{name}`"))?;
        *unit = unit.mul(b.pow(exp), sign);
        Ok(())
    };
    for c in t.chars() {
        match c {
            '·' | '*' | '⋅' | ' ' | '.' => flush(&mut term, sign, &mut unit)?,
            '/' => {
                flush(&mut term, sign, &mut unit)?;
                sign = -1;
            }
            _ => term.push(c),
        }
    }
    flush(&mut term, sign, &mut unit)?;
    Ok(unit)
}

/// A number and the unit text after it: "24 mN·m" → (24, "mN·m").
/// Accepts `≈`, `~`, thousands separators and exponents; None without a number.
pub fn split_quantity(text: &str) -> Option<(f64, String)> {
    let t = text.trim().trim_start_matches(['≈', '~', '=']).trim().replace(',', "").replace('−', "-");
    let chars: Vec<char> = t.chars().collect();
    let mut end = 0;
    while end < chars.len() {
        let c = chars[end];
        let ok = c.is_ascii_digit() || c == '.' || ((c == '-' || c == '+') && (end == 0 || matches!(chars[end - 1], 'e' | 'E'))) || ((c == 'e' || c == 'E') && end > 0 && chars.get(end + 1).is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+'));
        if !ok {
            break;
        }
        end += 1;
    }
    let mut number: String = chars[..end].iter().collect();
    while !number.is_empty() && number.parse::<f64>().is_err() {
        number.pop();
    }
    let value: f64 = number.parse().ok().filter(|v: &f64| v.is_finite())?;
    let rest: String = chars[number.chars().count()..].iter().collect();
    Some((value, rest.trim().to_string()))
}

/// A given answer converted to the question's unit, and what to say about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    /// In the question's unit.
    pub value: f64,
    /// The unit the reader wrote, if any.
    pub written: String,
}

/// Read an answer like "24 mN·m" against a question in `unit` ("N·m").
/// No unit written: the number is taken in the question's unit.
pub fn read(text: &str, unit: &str) -> Result<Reading, String> {
    let (value, written) = split_quantity(text).ok_or("type a number first")?;
    let target = parse_unit(unit).map_err(|e| format!("question unit: {e}"))?;
    if written.is_empty() || unit.trim().is_empty() {
        return Ok(Reading { value, written });
    }
    let given = parse_unit(&written).map_err(|e| format!("{e} — write the number, then a unit like {unit}"))?;
    if given.dims != target.dims {
        return Err(format!("{written} is {}, but the question asks for {} ({unit})", given.kind(), target.kind()));
    }
    Ok(Reading { value: value * given.factor / target.factor, written })
}

/// The likely slip when `given` misses `correct` (both in the question's
/// unit) but a common conversion would make it right within `ok`.
pub fn slip(given: f64, correct: f64, unit: &str, ok: impl Fn(f64) -> bool) -> Option<String> {
    let target = parse_unit(unit).ok()?;
    let rpm = 2.0 * PI / 60.0;
    let checks: Vec<(f64, String)> = vec![
        (1e-3, "Check the prefix: your number is 1000 times too large (milli vs. base unit?).".into()),
        (1e3, "Check the prefix: your number is 1000 times too small (base unit vs. milli?).".into()),
        (1e-6, "Check the prefix: off by a factor of a million (micro?).".into()),
        (1e6, "Check the prefix: off by a factor of a million (micro?).".into()),
        (rpm, if target.dims == [0, 0, -1, 0, 0] { "That looks like rpm; this answer is in rad/s (× 2π/60).".into() } else { String::new() }),
        (1.0 / rpm, if target.dims == [0, 0, -1, 0, 0] { "That looks like rad/s; this answer is in rpm (× 60/2π).".into() } else { String::new() }),
        (2.0 * PI, "Off by 2π: turns and radians?".into()),
        (1.0 / (2.0 * PI), "Off by 2π: radians and turns?".into()),
        (PI / 180.0, "That looks like degrees; convert to radians (× π/180).".into()),
        (180.0 / PI, "That looks like radians; convert to degrees (× 180/π).".into()),
        (-1.0, "Right size, wrong sign: which way does it act?".into()),
    ];
    let _ = correct;
    checks.into_iter().find(|(f, msg)| !msg.is_empty() && ok(given * f)).map(|(_, m)| m)
}

/// Format `value` (SI) in `unit` with an SI prefix that keeps 1 ≤ |x| < 1000
/// where the unit takes one, and `digits` significant figures.
pub fn format_si(value: f64, unit: &str, digits: usize) -> String {
    // Ratio units (N·m/A, V·s/rad) read best unprefixed.
    let prefixable = !unit.is_empty() && !unit.contains('/') && !["rpm", "%", "°", "deg", "°C", "min", "h"].contains(&unit.trim());
    let (v, p) = if prefixable && value != 0.0 {
        let a = value.abs();
        if a >= 1e3 && a < 1e6 {
            (value / 1e3, "k")
        } else if a >= 1e-3 && a < 1.0 {
            (value * 1e3, "m")
        } else if a >= 1e-6 && a < 1e-3 {
            (value * 1e6, "µ")
        } else {
            (value, "")
        }
    } else {
        (value, "")
    };
    let text = significant(v, digits);
    if unit.is_empty() { text } else { format!("{text} {p}{unit}") }
}

/// `value` with `digits` significant figures, without trailing exponent noise.
pub fn significant(value: f64, digits: usize) -> String {
    if value == 0.0 || !value.is_finite() {
        return format!("{value}");
    }
    let magnitude = value.abs().log10().floor() as i32;
    // Very small or very large: scientific, so the figures stay readable.
    if !(-4..=6).contains(&magnitude) {
        return format!("{:.*e}", digits.saturating_sub(1), value);
    }
    let decimals = (digits as i32 - 1 - magnitude).max(0) as usize;
    format!("{value:.decimals$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }
    #[test]
    fn units_parse_convert_and_name_their_kind() {
        let nm = parse_unit("N·m").unwrap();
        assert_eq!(nm.dims, [2, 1, -2, 0, 0]);
        assert!(close(parse_unit("mN·m").unwrap().factor, 1e-3));
        assert_eq!(parse_unit("V·s/rad").unwrap().dims, parse_unit("N·m/A").unwrap().dims);
        assert_eq!(parse_unit("m/s²").unwrap().dims, [1, 0, -2, 0, 0]);
        assert_eq!(parse_unit("kg·m^2").unwrap().dims, [2, 1, 0, 0, 0]);
        assert!(close(parse_unit("rpm").unwrap().factor, 2.0 * PI / 60.0));
        assert!(parse_unit("furlong").is_err());
        assert!(close(read("24 mN·m", "N·m").unwrap().value, 0.024));
        assert!(close(read("4300 rpm", "rad/s").unwrap().value, 4300.0 * 2.0 * PI / 60.0));
        assert!(close(read("3.3", "A").unwrap().value, 3.3));
        assert!(close(read("≈ 1,500 mA", "A").unwrap().value, 1.5));
        assert!(read("3 V", "A").unwrap_err().contains("a voltage"));
        assert_eq!(split_quantity("4.4e2 rad/s"), Some((440.0, "rad/s".into())));
        assert_eq!(split_quantity("-0.5"), Some((-0.5, String::new())));
        assert_eq!(split_quantity("about"), None);
    }
    #[test]
    fn slips_are_named() {
        let ok = |v: f64| (v - 0.024).abs() < 0.001;
        assert!(slip(24.0, 0.024, "N·m", ok).unwrap().contains("1000 times too large"));
        let ok = |v: f64| (v - 450.0).abs() < 5.0;
        assert!(slip(4297.0, 450.0, "rad/s", ok).unwrap().contains("rpm"));
        assert!(slip(-450.0, 450.0, "rad/s", ok).unwrap().contains("sign"));
        assert_eq!(slip(300.0, 450.0, "rad/s", ok), None);
    }
    #[test]
    fn values_print_with_prefix_and_figures() {
        assert_eq!(format_si(0.03948, "N·m", 3), "39.5 mN·m");
        assert_eq!(format_si(444.83, "rad/s", 3), "445 rad/s");
        assert_eq!(format_si(3.2891, "A", 3), "3.29 A");
        assert_eq!(significant(0.012132, 2), "0.012");
    }
}
