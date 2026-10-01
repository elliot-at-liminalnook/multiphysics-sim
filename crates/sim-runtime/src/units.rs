//! Unit-aware numeric entry: a port of RoboCAD's `cad/robocad/units.py`.
//!
//! RoboCAD remains the reference for this evaluator until the CAD parity
//! harness retires the Python path; keep the two in step. CAD mode's numeric
//! bar calls [`evaluate`] on every keystroke, so it is cheap, never panics and
//! never recurses without bound.
//!
//! Lengths resolve to millimetres and angles to degrees. A bare number takes
//! the field's default unit (a key of [`LENGTH_UNITS`] or [`ANGLE_UNITS`]), or
//! the internal unit when there is none. The grammar is a small
//! recursive-descent parser, so a typed expression can only do arithmetic:
//!
//! ```text
//! expr  := term (('+'|'-') term)*
//! term  := unary (('*'|'/'|'%') unary)*
//! unary := ('-'|'+') unary | power
//! power := atom (('^'|'**') unary)?
//! atom  := number unit? | function '(' expr ')' | constant unit? | '(' expr ')' unit?
//! ```
//!
//! So `-2^2` is -4 and `2^3^2` is 512. A bare number or constant takes the
//! default unit's scale; a parenthesised group without a suffix does not.
//! Functions are `sin`, `cos`, `tan` (degrees), `sqrt`, `abs`, `round`
//! (half to even), `floor` and `ceil`; constants are `pi`, `e` and `tau`;
//! `%` takes the sign of the divisor, as Python's does.
//!
//! ```
//! use sim_runtime::units::evaluate;
//! assert!((evaluate("20mm + 0.3", false, None).unwrap() - 20.3).abs() < 1e-12);
//! assert_eq!(evaluate("1in", false, None).unwrap(), 25.4);
//! assert_eq!(evaluate("45deg", true, None).unwrap(), 45.0);
//! assert_eq!(evaluate("2", false, Some("cm")).unwrap(), 20.0);
//! ```
//!
//! Error messages keep Python's wording and add the offending token and its
//! character position, counted in the input with leading and trailing
//! whitespace removed, as Python counts them.
//!
//! Deliberate differences from the Python reference, each where Python would
//! crash the numeric bar, raise something other than `ExpressionError`, or
//! give a wrong answer:
//!
//! - `x % 0` returns "modulo by zero" (Python raises `ZeroDivisionError`).
//! - `sqrt` of a negative number, `sin`/`cos`/`tan` of an infinite angle and
//!   `round`/`floor`/`ceil` of a non-finite number return errors (Python
//!   raises `ValueError` or `OverflowError`).
//! - A negative base to a fractional power returns an error (Python returns
//!   a complex number and then fails with `TypeError`); zero to a negative
//!   power returns an error (Python: `ZeroDivisionError`); a power that
//!   overflows returns an error (Python: `OverflowError`).
//! - A default unit missing from the field's table returns an error when a
//!   bare number needs it (Python raises `KeyError` at the same point).
//! - Nesting deeper than 200 levels (parentheses or signs) returns an error
//!   (Python raises `RecursionError` at roughly the same depth; Rust would
//!   overflow the stack).
//! - "unexpected character" names the offending character. Python's regex
//!   consumes leading whitespace before failing, so for `1 $` it reports
//!   `' ' at 1`; this port reports `'$' at 2`.
//! - "expected" messages quote the expected token: `expected ')'` where
//!   Python prints `expected )`.
//! - `round`, `floor` and `ceil` return Python ints, which are exact and
//!   never negative zero; here they are `f64`, normalised to `+0.0`. Results
//!   beyond 2^53, and sign-of-zero after negating such an int, may differ.
//! - Digits are ASCII only (Python's `\d` and `float` accept any Unicode
//!   decimal digit). Whitespace follows `char::is_whitespace` plus the
//!   `\x1c`–`\x1f` separators, which approximates Python's `str.isspace`.
//! - [`format_length`] and [`format_angle`] strip trailing zeros only after a
//!   decimal point: Python's `rstrip("0")` turns `100` at `digits = 0` into
//!   `"1 mm"`. An unknown unit formats in millimetres (Python: `KeyError`).

use std::f64::consts::{E, PI, TAU};
use std::fmt;

/// Length units and their size in millimetres, in RoboCAD's order.
pub const LENGTH_UNITS: &[(&str, f64)] = &[
    ("mm", 1.0),
    ("millimeter", 1.0),
    ("millimetre", 1.0),
    ("cm", 10.0),
    ("m", 1000.0),
    ("in", 25.4),
    ("inch", 25.4),
    ("\"", 25.4),
    ("ft", 304.8),
    ("'", 304.8),
    ("thou", 0.0254),
    ("mil", 0.0254),
    ("um", 0.001),
    ("\u{b5}m", 0.001),
];

/// Angle units and their size in degrees, in RoboCAD's order.
pub const ANGLE_UNITS: &[(&str, f64)] = &[
    ("deg", 1.0),
    ("\u{b0}", 1.0),
    ("rad", 180.0 / PI),
    ("grad", 0.9),
    ("turn", 360.0),
];

const CONSTANTS: &[(&str, f64)] = &[("pi", PI), ("e", E), ("tau", TAU)];
const FUNCTIONS: &[&str] = &["sin", "cos", "tan", "sqrt", "abs", "round", "floor", "ceil"];

/// Deepest nesting of parentheses and signs the parser accepts.
const MAX_DEPTH: usize = 200;

/// Why an expression did not evaluate. `token` is the offending token's text
/// and `position` its character index in the trimmed input; `position` alone
/// (no token) marks the end of the input.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitError {
    pub message: String,
    pub token: Option<String>,
    pub position: Option<usize>,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UnitError {}

impl UnitError {
    fn bare(message: &str) -> Self {
        Self { message: message.to_string(), token: None, position: None }
    }
}

fn lookup(table: &[(&str, f64)], key: &str) -> Option<f64> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Python's `str.isspace` (approximately), used by `strip` and `\s`.
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '\u{b5}' | '\u{b0}' | '"' | '\'')
}

/// Python's `repr` of a string: single quotes unless the text holds a single
/// quote and no double quote; non-printable characters escaped
/// (approximately: Rust has no Unicode category table for `isprintable`).
fn py_repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        let u = c as u32;
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            _ if u < 0x20 || (0x7f..=0xa0).contains(&u) || u == 0xad => {
                out.push_str(&format!("\\x{u:02x}"));
            }
            _ if (c.is_whitespace() && c != ' ')
                || (0x200b..=0x200f).contains(&u)
                || (0x202a..=0x202e).contains(&u)
                || (0x2060..=0x2064).contains(&u)
                || u == 0xfeff =>
            {
                if u <= 0xffff {
                    out.push_str(&format!("\\u{u:04x}"));
                } else {
                    out.push_str(&format!("\\U{u:08x}"));
                }
            }
            _ => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Num,
    Name,
    Op,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: Kind,
    text: String,
    /// Character index in the trimmed input.
    pos: usize,
    /// The literal's value for `Kind::Num`.
    value: f64,
}

/// Port of `_tokenize` and its `_TOKEN` regex:
/// `\s*(?:(\d+\.\d*|\.\d+|\d+)(?:[eE][-+]?\d+)?|([A-Za-z_µ°"']+)|(\*\*|[-+*/^%(),]))`.
fn tokenize(chars: &[char]) -> Result<Vec<Tok>, UnitError> {
    let n = chars.len();
    let digits_from = |mut k: usize| {
        while k < n && chars[k].is_ascii_digit() {
            k += 1;
        }
        k
    };
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j < n && is_py_space(chars[j]) {
            j += 1;
        }
        if j == n {
            break; // unreachable after trimming; kept so the loop cannot misbehave
        }
        let c = chars[j];
        let next = chars.get(j + 1).copied();
        if c.is_ascii_digit() || (c == '.' && next.is_some_and(|d| d.is_ascii_digit())) {
            // Mantissa: \d+\.\d* | \.\d+ | \d+
            let int_end = digits_from(j);
            let mut k = int_end;
            let mut frac = (k, k);
            if k < n && chars[k] == '.' {
                let frac_end = digits_from(k + 1);
                frac = (k + 1, frac_end);
                k = frac_end;
            }
            // Optional exponent: [eE][-+]?\d+
            let mut exponent = String::new();
            if k < n && matches!(chars[k], 'e' | 'E') {
                let mut m = k + 1;
                if m < n && matches!(chars[m], '+' | '-') {
                    m += 1;
                }
                let exp_end = digits_from(m);
                if exp_end > m {
                    exponent = chars[k + 1..exp_end].iter().collect();
                    k = exp_end;
                }
            }
            // Canonical form so the parse never depends on `1.` or `.5` support.
            let int: String = chars[j..int_end].iter().collect();
            let frac_digits: String = chars[frac.0..frac.1].iter().collect();
            let canonical = format!(
                "{}.{}{}{}",
                if int.is_empty() { "0" } else { &int },
                if frac_digits.is_empty() { "0" } else { &frac_digits },
                if exponent.is_empty() { "" } else { "e" },
                exponent
            );
            let text: String = chars[j..k].iter().collect();
            let value = canonical.parse::<f64>().map_err(|_| UnitError {
                message: format!("bad number {} at {}", py_repr(&text), j),
                token: Some(text.clone()),
                position: Some(j),
            })?;
            out.push(Tok { kind: Kind::Num, text, pos: j, value });
            i = k;
        } else if is_name_char(c) {
            let mut k = j;
            while k < n && is_name_char(chars[k]) {
                k += 1;
            }
            out.push(Tok { kind: Kind::Name, text: chars[j..k].iter().collect(), pos: j, value: 0.0 });
            i = k;
        } else if c == '*' && next == Some('*') {
            out.push(Tok { kind: Kind::Op, text: "**".into(), pos: j, value: 0.0 });
            i = j + 2;
        } else if "-+*/^%(),".contains(c) {
            out.push(Tok { kind: Kind::Op, text: c.to_string(), pos: j, value: 0.0 });
            i = j + 1;
        } else {
            let text = c.to_string();
            return Err(UnitError {
                message: format!("unexpected character {} at {}", py_repr(&text), j),
                token: Some(text),
                position: Some(j),
            });
        }
    }
    Ok(out)
}

/// Python's float `%`: the result takes the divisor's sign. `w` is non-zero.
fn py_mod(v: f64, w: f64) -> f64 {
    let m = v % w;
    if m != 0.0 {
        if (w < 0.0) != (m < 0.0) { m + w } else { m }
    } else {
        0.0_f64.copysign(w)
    }
}

struct Parser<'a> {
    t: Vec<Tok>,
    i: usize,
    /// Character length of the trimmed input: the position of "end of expression".
    end: usize,
    angle: bool,
    units: &'static [(&'static str, f64)],
    default: Option<&'a str>,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.t.get(self.i)
    }

    fn peek_op(&self, ops: &[&str]) -> Option<Tok> {
        self.peek().filter(|t| t.kind == Kind::Op && ops.contains(&t.text.as_str())).cloned()
    }

    /// `message` followed by the token at `index` and its character position.
    fn at(&self, message: String, index: usize) -> UnitError {
        match self.t.get(index) {
            Some(tok) => UnitError {
                message: format!("{message} ({}, character {})", py_repr(&tok.text), tok.pos),
                token: Some(tok.text.clone()),
                position: Some(tok.pos),
            },
            None => UnitError {
                message: format!("{message} (end of expression, character {})", self.end),
                token: None,
                position: Some(self.end),
            },
        }
    }

    fn on(message: String, tok: &Tok) -> UnitError {
        UnitError {
            message: format!("{message} ({}, character {})", py_repr(&tok.text), tok.pos),
            token: Some(tok.text.clone()),
            position: Some(tok.pos),
        }
    }

    fn take(&mut self, text: &str) -> Result<(), UnitError> {
        if self.peek().is_some_and(|tok| tok.kind == Kind::Op && tok.text == text) {
            self.i += 1;
            Ok(())
        } else {
            Err(self.at(format!("expected {} at token {}", py_repr(text), self.i), self.i))
        }
    }

    fn expr(&mut self) -> Result<f64, UnitError> {
        let mut v = self.term()?;
        while let Some(tok) = self.peek_op(&["+", "-"]) {
            self.i += 1;
            let w = self.term()?;
            v = if tok.text == "+" { v + w } else { v - w };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, UnitError> {
        let mut v = self.unary()?;
        while let Some(tok) = self.peek_op(&["*", "/", "%"]) {
            self.i += 1;
            let w = self.unary()?;
            match tok.text.as_str() {
                "*" => v *= w,
                "/" => {
                    if w == 0.0 {
                        return Err(Self::on("division by zero".into(), &tok));
                    }
                    v /= w;
                }
                _ => {
                    if w == 0.0 {
                        return Err(Self::on("modulo by zero".into(), &tok));
                    }
                    v = py_mod(v, w);
                }
            }
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64, UnitError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.at("expression nests too deeply".into(), self.i));
        }
        let r = self.unary_inner();
        self.depth -= 1;
        r
    }

    fn unary_inner(&mut self) -> Result<f64, UnitError> {
        if let Some(tok) = self.peek_op(&["+", "-"]) {
            self.i += 1;
            let v = self.unary()?;
            return Ok(if tok.text == "-" { -v } else { v });
        }
        self.power()
    }

    fn power(&mut self) -> Result<f64, UnitError> {
        let v = self.atom()?;
        if let Some(tok) = self.peek_op(&["^", "**"]) {
            self.i += 1;
            let w = self.unary()?;
            return Self::pow(v, w, &tok);
        }
        Ok(v)
    }

    /// Python's float `**`: `powf` shares C's special cases; the cases where
    /// Python raises (or goes complex) become errors.
    fn pow(v: f64, w: f64, tok: &Tok) -> Result<f64, UnitError> {
        if w == 0.0 || v.is_nan() || w.is_nan() || w.is_infinite() || v.is_infinite() {
            return Ok(v.powf(w));
        }
        if v == 0.0 && w < 0.0 {
            return Err(Self::on("zero cannot be raised to a negative power".into(), tok));
        }
        if v < 0.0 && w != w.floor() {
            return Err(Self::on("a negative number cannot be raised to a fractional power".into(), tok));
        }
        let r = v.powf(w);
        if r.is_infinite() {
            return Err(Self::on("power overflows".into(), tok));
        }
        Ok(r)
    }

    fn unit_suffix(&mut self) -> Result<Option<f64>, UnitError> {
        let Some(tok) = self.peek().filter(|t| t.kind == Kind::Name).cloned() else {
            return Ok(None);
        };
        if let Some(scale) = lookup(self.units, &tok.text) {
            self.i += 1;
            return Ok(Some(scale));
        }
        // A length unit in an angle field (or the reverse) is an error worth naming.
        let other = if self.angle { LENGTH_UNITS } else { ANGLE_UNITS };
        if lookup(other, &tok.text).is_some() {
            let kind = if self.angle { "angle" } else { "length" };
            return Err(Self::on(format!("{} is not a {kind} unit", py_repr(&tok.text)), &tok));
        }
        Ok(None)
    }

    fn default_scale(&self, tok: &Tok) -> Result<f64, UnitError> {
        let Some(unit) = self.default else { return Ok(1.0) };
        lookup(self.units, unit).ok_or_else(|| {
            let kind = if self.angle { "an angle" } else { "a length" };
            Self::on(format!("unknown default unit {} for {kind} field", py_repr(unit)), tok)
        })
    }

    fn scaled(&mut self, v: f64, tok: &Tok) -> Result<f64, UnitError> {
        let scale = match self.unit_suffix()? {
            Some(s) => s,
            None => self.default_scale(tok)?,
        };
        Ok(v * scale)
    }

    fn atom(&mut self) -> Result<f64, UnitError> {
        let Some(tok) = self.peek().cloned() else {
            return Err(UnitError {
                message: format!("unexpected end of expression (character {})", self.end),
                token: None,
                position: Some(self.end),
            });
        };
        match tok.kind {
            Kind::Num => {
                self.i += 1;
                self.scaled(tok.value, &tok)
            }
            Kind::Name => {
                self.i += 1;
                if FUNCTIONS.contains(&tok.text.as_str()) {
                    self.take("(")?;
                    let arg = self.expr()?;
                    self.take(")")?;
                    return Self::call(&tok, arg);
                }
                if let Some(v) = lookup(CONSTANTS, &tok.text) {
                    return self.scaled(v, &tok);
                }
                Err(Self::on(format!("unknown name {}", py_repr(&tok.text)), &tok))
            }
            Kind::Op if tok.text == "(" => {
                self.i += 1;
                let v = self.expr()?;
                self.take(")")?;
                Ok(match self.unit_suffix()? {
                    Some(scale) => v * scale,
                    None => v,
                })
            }
            Kind::Op => Err(Self::on(format!("unexpected {}", py_repr(&tok.text)), &tok)),
        }
    }

    fn call(tok: &Tok, x: f64) -> Result<f64, UnitError> {
        let name = tok.text.as_str();
        let fail = |what: &str| Err(Self::on(format!("{name} of {what}"), tok));
        match name {
            "sin" | "cos" | "tan" => {
                if x.is_infinite() {
                    return fail("an infinite angle");
                }
                let r = x * (PI / 180.0); // math.radians
                Ok(match name {
                    "sin" => r.sin(),
                    "cos" => r.cos(),
                    _ => r.tan(),
                })
            }
            "sqrt" if x < 0.0 => fail("a negative number"),
            "sqrt" => Ok(x.sqrt()),
            "abs" => Ok(x.abs()),
            _ if !x.is_finite() => fail("a non-finite number"),
            // Python returns ints here, which have no negative zero.
            "round" => Ok(x.round_ties_even() + 0.0),
            "floor" => Ok(x.floor() + 0.0),
            _ => Ok(x.ceil() + 0.0), // "ceil"
        }
    }
}

/// Evaluate `text` to millimetres (or degrees when `angle`). Bare numbers and
/// constants take `default_unit` (a key of the field's unit table) or the
/// internal unit when it is `None`.
pub fn evaluate(text: &str, angle: bool, default_unit: Option<&str>) -> Result<f64, UnitError> {
    let chars: Vec<char> = text.trim_matches(is_py_space).chars().collect();
    let tokens = tokenize(&chars)?;
    if tokens.is_empty() {
        return Err(UnitError::bare("empty expression"));
    }
    let mut p = Parser {
        t: tokens,
        i: 0,
        end: chars.len(),
        angle,
        units: if angle { ANGLE_UNITS } else { LENGTH_UNITS },
        default: default_unit,
        depth: 0,
    };
    let v = p.expr()?;
    if p.i < p.t.len() {
        return Err(p.at(format!("trailing input at token {}", p.i), p.i));
    }
    if !v.is_finite() {
        return Err(UnitError::bare("result is not a finite number"));
    }
    Ok(v)
}

/// [`evaluate`], or `None` when the text is not a valid expression.
pub fn try_evaluate(text: &str, angle: bool, default_unit: Option<&str>) -> Option<f64> {
    evaluate(text, angle, default_unit).ok()
}

/// Python's `f"{v:.{digits}f}"` with trailing fractional zeros removed.
fn fixed(v: f64, digits: usize) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let s = format!("{v:.digits$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s.is_empty() { "0".into() } else { s.to_string() }
}

/// `mm` shown in `unit` with at most `digits` decimals: `"12.5 mm"`, `"1 in"`.
/// An unknown unit falls back to millimetres.
pub fn format_length(mm: f64, unit: &str, digits: usize) -> String {
    let (scale, unit) = match lookup(LENGTH_UNITS, unit) {
        Some(scale) => (scale, unit),
        None => (1.0, "mm"),
    };
    format!("{} {unit}", fixed(mm / scale, digits))
}

/// `deg` with at most `digits` decimals: `"45°"`.
pub fn format_angle(deg: f64, digits: usize) -> String {
    format!("{}\u{b0}", fixed(deg, digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(text: &str) -> f64 {
        evaluate(text, false, None).unwrap_or_else(|e| panic!("{text:?}: {e}"))
    }

    fn ev_angle(text: &str) -> f64 {
        evaluate(text, true, None).unwrap_or_else(|e| panic!("{text:?}: {e}"))
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-9 * b.abs().max(1.0)
    }

    fn err(text: &str, angle: bool, default_unit: Option<&str>) -> UnitError {
        match evaluate(text, angle, default_unit) {
            Ok(v) => panic!("{text:?} evaluated to {v}"),
            Err(e) => e,
        }
    }

    // ---- cad/tests/test_units.py

    #[test]
    fn bare_number_is_millimetres() {
        assert_eq!(ev("20"), 20.0);
    }

    #[test]
    fn unit_suffixes() {
        assert!(close(ev("20mm + 0.3"), 20.3));
        assert!(close(ev("1in"), 25.4));
        assert!(close(ev("2cm"), 20.0));
        assert!(close(ev("1m"), 1000.0));
        assert!(close(ev("1ft"), 304.8));
        assert!(close(ev("(1/2)\""), 12.7));
    }

    #[test]
    fn arithmetic_and_pi() {
        assert_eq!(ev("50/2"), 25.0);
        assert!(close(ev("pi*10"), PI * 10.0));
        assert!(close(ev("(1in + 2mm) * 2"), 54.8));
        assert_eq!(ev("2^3"), 8.0);
        assert_eq!(ev("-5 + 10"), 5.0);
        assert_eq!(ev("sqrt(16)"), 4.0);
    }

    #[test]
    fn angles() {
        assert_eq!(ev_angle("45deg"), 45.0);
        assert!(close(ev_angle("pi rad"), 180.0));
        assert!(close(ev_angle("0.5turn"), 180.0));
    }

    #[test]
    fn default_unit_for_bare_numbers() {
        assert!(close(evaluate("2", false, Some("in")).unwrap(), 50.8));
        assert!(close(evaluate("2 + 1mm", false, Some("in")).unwrap(), 51.8));
    }

    #[test]
    fn errors() {
        for text in ["", "2 +", "1/0", "__import__('os')", "30deg"] {
            assert!(evaluate(text, false, None).is_err(), "{text:?}");
        }
    }

    #[test]
    fn formatting() {
        assert_eq!(format_length(25.4, "in", 3), "1 in");
        assert_eq!(format_length(12.345, "mm", 3), "12.345 mm");
        assert_eq!(format_angle(90.0, 2), "90\u{b0}");
    }

    // ---- module docstring examples

    #[test]
    fn docstring_examples() {
        assert!(close(ev("20mm + 0.3"), 20.3));
        assert!(close(ev("1in"), 25.4));
        assert!(close(ev("pi*10"), 31.415926535897931));
        assert_eq!(ev_angle("45deg"), 45.0);
    }

    // ---- units and defaults

    #[test]
    fn default_units() {
        assert_eq!(evaluate("2", false, Some("cm")).unwrap(), 20.0);
        assert_eq!(evaluate("2", false, Some("mm")).unwrap(), 2.0);
        assert!(close(evaluate("1", true, Some("rad")).unwrap(), 180.0 / PI));
        assert_eq!(evaluate("1deg", true, Some("rad")).unwrap(), 1.0);
        // Constants take the default unit; a parenthesised group does not.
        assert!(close(evaluate("pi", false, Some("in")).unwrap(), PI * 25.4));
        assert!(close(evaluate("(2)", false, Some("in")).unwrap(), 50.8)); // the 2 inside is bare
        assert!(close(evaluate("(2mm)", false, Some("in")).unwrap(), 2.0));
        assert!(close(evaluate("(2mm)cm", false, Some("in")).unwrap(), 20.0));
        // Function arguments are expressions, so bare numbers there are scaled too, as in Python.
        assert!(close(evaluate("sqrt(4)", false, Some("in")).unwrap(), 101.6_f64.sqrt()));
    }

    #[test]
    fn angle_units() {
        assert!(close(ev_angle("1rad"), 180.0 / PI));
        assert!(close(ev_angle("0.5turn"), 180.0));
        assert!(close(ev_angle("100grad"), 90.0));
        assert_eq!(ev_angle("30\u{b0}"), 30.0);
        assert_eq!(ev_angle("30 \u{b0} + 15deg"), 45.0);
    }

    #[test]
    fn unicode_and_quote_units() {
        assert!(close(ev("5\u{b5}m"), 0.005));
        assert!(close(ev("5um"), 0.005));
        assert!(close(ev("1\""), 25.4));
        assert!(close(ev("2'"), 609.6));
        assert!(close(ev("1thou + 1mil"), 0.0508));
        assert!(close(ev("1 inch + 1 millimetre + 1 millimeter"), 27.4));
    }

    #[test]
    fn number_literals() {
        assert_eq!(ev("1.e5mm"), 1e5);
        assert_eq!(ev(".5"), 0.5);
        assert_eq!(ev("1."), 1.0);
        assert_eq!(ev("1e-3"), 0.001);
        assert_eq!(ev("1E3"), 1000.0);
        assert_eq!(ev("2.5e+1"), 25.0);
        // `2e` is the number 2 then the name `e`, which is not a unit: trailing input.
        assert!(evaluate("2e", false, None).is_err());
        assert!(evaluate("2pi", false, None).is_err());
    }

    // ---- grammar

    #[test]
    fn power_and_unary() {
        assert_eq!(ev("2^3^2"), 512.0);
        assert_eq!(ev("-2^2"), -4.0);
        assert_eq!(ev("(-2)^2"), 4.0);
        assert_eq!(ev("2**3"), 8.0);
        assert_eq!(ev("2^-1"), 0.5);
        assert_eq!(ev("--3"), 3.0);
        assert_eq!(ev("+-3"), -3.0);
        assert_eq!(ev("2*-3"), -6.0);
        assert_eq!(ev("1 - 2 - 3"), -4.0);
        assert_eq!(ev("8 / 4 / 2"), 1.0);
    }

    #[test]
    fn modulo_takes_the_divisor_sign() {
        assert_eq!(ev("7 % 3"), 1.0);
        assert_eq!(ev("-7 % 3"), 2.0);
        assert_eq!(ev("7 % -3"), -2.0);
        assert_eq!(ev("-7 % -3"), -1.0);
        let z = ev("-6 % 3");
        assert_eq!(z, 0.0);
        assert!(z.is_sign_positive());
        assert!(ev("6 % -3").is_sign_negative());
    }

    #[test]
    fn functions_and_constants() {
        assert!(close(ev("sin(30)"), 0.5));
        assert!(close(ev("cos(60)"), 0.5));
        assert!(close(ev("tan(45)"), 1.0));
        assert_eq!(ev("abs(-3)"), 3.0);
        assert_eq!(ev("round(2.5)"), 2.0);
        assert_eq!(ev("round(3.5)"), 4.0);
        assert!(ev("round(-0.4)").is_sign_positive());
        assert_eq!(ev("floor(-1.5)"), -2.0);
        assert_eq!(ev("ceil(1.2)"), 2.0);
        assert!(ev("ceil(-0.5)").is_sign_positive());
        assert!(close(ev("e"), E));
        assert!(close(ev("tau"), TAU));
        assert!(close(ev_angle("tau rad"), 360.0));
    }

    // ---- errors name the token and its position

    #[test]
    fn unexpected_character() {
        let e = err("1 $", false, None);
        assert_eq!(e.message, "unexpected character '$' at 2");
        assert_eq!(e.token.as_deref(), Some("$"));
        assert_eq!(e.position, Some(2));
        // Positions count characters of the trimmed text, not bytes.
        let e = err("  5\u{b5}m + 3$", false, None);
        assert_eq!(e.position, Some(7));
        assert_eq!(e.to_string(), "unexpected character '$' at 7");
    }

    #[test]
    fn wrong_kind_unit() {
        let e = err("30deg", false, None);
        assert_eq!(e.message, "'deg' is not a length unit ('deg', character 2)");
        assert_eq!(e.token.as_deref(), Some("deg"));
        assert_eq!(e.position, Some(2));
        let e = err("pi mm", true, None);
        assert_eq!(e.message, "'mm' is not a angle unit ('mm', character 3)");
        // A length unit used as a name in a length field without a number is unknown.
        assert_eq!(err("rad", false, None).message, "unknown name 'rad' ('rad', character 0)");
    }

    #[test]
    fn unknown_names() {
        let e = err("foo + 1", false, None);
        assert_eq!(e.message, "unknown name 'foo' ('foo', character 0)");
        assert_eq!(e.position, Some(0));
        assert_eq!(err("__import__('os')", false, None).token.as_deref(), Some("__import__"));
        assert_eq!(err("'", false, None).message, "unknown name \"'\" (\"'\", character 0)");
    }

    #[test]
    fn expected_and_trailing() {
        let e = err("(1 + 2", false, None);
        assert_eq!(e.message, "expected ')' at token 4 (end of expression, character 6)");
        assert_eq!(e.token, None);
        assert_eq!(e.position, Some(6));
        let e = err("round(2.5, 1)", false, None);
        assert_eq!(e.message, "expected ')' at token 3 (',', character 9)");
        assert_eq!(err("sin 30", false, None).message, "expected '(' at token 1 ('30', character 4)");
        let e = err("1 2", false, None);
        assert_eq!(e.message, "trailing input at token 1 ('2', character 2)");
        assert_eq!(e.token.as_deref(), Some("2"));
        assert_eq!(err("sqrt(16)mm", false, None).message, "trailing input at token 4 ('mm', character 8)");
    }

    #[test]
    fn end_empty_and_unexpected() {
        assert_eq!(err("", false, None).message, "empty expression");
        assert_eq!(err("   ", false, None).message, "empty expression");
        let e = err("2 +", false, None);
        assert_eq!(e.message, "unexpected end of expression (character 3)");
        assert_eq!(e.position, Some(3));
        assert_eq!(err(")", false, None).message, "unexpected ')' (')', character 0)");
    }

    #[test]
    fn arithmetic_errors() {
        let e = err("1/0", false, None);
        assert_eq!(e.message, "division by zero ('/', character 1)");
        assert_eq!(e.position, Some(1));
        assert_eq!(err("5 % 0", false, None).message, "modulo by zero ('%', character 2)");
        assert!(err("sqrt(-1)", false, None).message.starts_with("sqrt of a negative number"));
        assert!(err("(-8)^(1/3)", false, None).message.contains("fractional power"));
        assert!(err("0^-1", false, None).message.contains("negative power"));
        assert!(err("10^400", false, None).message.starts_with("power overflows"));
        assert_eq!(err("1e400", false, None).message, "result is not a finite number");
        assert_eq!(err("1e308 * 10", false, None).message, "result is not a finite number");
        assert!(err("sin(1e400)", false, None).message.starts_with("sin of an infinite angle"));
        assert!(err("round(1e400)", false, None).message.starts_with("round of a non-finite number"));
        assert_eq!(ev("(-8)^(1/1)"), -8.0);
    }

    #[test]
    fn bad_default_unit_is_an_error_only_when_needed() {
        let e = err("2", false, Some("deg"));
        assert_eq!(e.message, "unknown default unit 'deg' for a length field ('2', character 0)");
        assert_eq!(evaluate("2mm", false, Some("deg")).unwrap(), 2.0);
    }

    #[test]
    fn nesting_is_bounded() {
        let deep = format!("{}1{}", "(".repeat(1000), ")".repeat(1000));
        assert!(err(&deep, false, None).message.starts_with("expression nests too deeply"));
        let signs = format!("{}1", "-".repeat(10_000));
        assert!(err(&signs, false, None).message.starts_with("expression nests too deeply"));
        let ok = format!("{}1{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(ev(&ok), 1.0);
    }

    #[test]
    fn try_evaluate_and_error_traits() {
        assert_eq!(try_evaluate("2cm", false, None), Some(20.0));
        assert_eq!(try_evaluate("2 +", false, None), None);
        let e = err("foo", false, None);
        assert_eq!(e.to_string(), e.message);
        let boxed: Box<dyn std::error::Error> = Box::new(e);
        assert!(boxed.to_string().contains("'foo'"));
    }

    #[test]
    fn format_cases() {
        assert_eq!(format_length(12.5, "mm", 3), "12.5 mm");
        assert_eq!(format_length(0.0, "mm", 3), "0 mm");
        assert_eq!(format_length(-0.0, "mm", 3), "-0 mm"); // as Python's f-string
        assert_eq!(format_length(1000.0, "m", 3), "1 m");
        assert_eq!(format_length(12.3456, "mm", 2), "12.35 mm");
        assert_eq!(format_length(100.0, "mm", 0), "100 mm"); // Python: "1 mm"
        assert_eq!(format_length(5.0, "furlong", 3), "5 mm");
        assert_eq!(format_length(f64::NAN, "mm", 3), "nan mm");
        assert_eq!(format_angle(45.0, 2), "45\u{b0}");
        assert_eq!(format_angle(2.675, 2), "2.67\u{b0}"); // 2.675 is below the tie in binary
        assert_eq!(format_angle(0.0, 2), "0\u{b0}");
    }

    #[test]
    fn tables_match_robocad() {
        assert_eq!(LENGTH_UNITS.len(), 14);
        assert_eq!(ANGLE_UNITS.len(), 5);
        assert!(close(lookup(ANGLE_UNITS, "rad").unwrap(), 57.29577951308232));
        assert_eq!(lookup(LENGTH_UNITS, "\u{b5}m"), Some(0.001));
    }
}
