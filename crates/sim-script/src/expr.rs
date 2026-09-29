//! Small arithmetic expressions over named numbers, for lesson equations,
//! question answers and data mappings (`k * i`, `duty * supply_v`,
//! `(V - k * w) / R`). Evaluated by a sandboxed Rhai engine: numbers,
//! operators and the math functions only (no loops, no I/O, bounded work).
use rhai::{Dynamic, Engine, Scope};
use std::collections::BTreeMap;

fn engine() -> Engine {
    let mut e = Engine::new_raw();
    use rhai::packages::Package;
    rhai::packages::ArithmeticPackage::new().register_into_engine(&mut e);
    rhai::packages::BasicMathPackage::new().register_into_engine(&mut e);
    rhai::packages::LogicPackage::new().register_into_engine(&mut e);
    e.set_max_operations(10_000);
    e.set_max_expr_depths(32, 32);
    e.set_fast_operators(true);
    e.register_fn("pi", || std::f64::consts::PI);
    e
}

/// Variable names an expression uses, in order of first use.
pub fn variables(expr: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in tokens(expr) {
        if let Token::Name(n) = token {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// Evaluate `expr` with `vars`. Integers are promoted to floats, so `1/2`
/// is 0.5. Errors name the problem (unknown variable, bad syntax).
pub fn eval(expr: &str, vars: &BTreeMap<String, f64>) -> Result<f64, String> {
    if expr.trim().is_empty() {
        return Err("empty expression".into());
    }
    for v in variables(expr) {
        if !vars.contains_key(&v) {
            return Err(format!("`{expr}`: unknown name `{v}`"));
        }
    }
    let mut scope = Scope::new();
    for (k, v) in vars {
        scope.push_constant(k.as_str(), *v);
    }
    // Float literals everywhere: `1/2` must not be integer division.
    let source = floats(expr);
    let value: Dynamic = engine().eval_expression_with_scope(&mut scope, &source).map_err(|e| format!("`{expr}`: {e}"))?;
    let v = value.as_float().or_else(|_| value.as_int().map(|i| i as f64)).map_err(|t| format!("`{expr}` gives a {t}, not a number"))?;
    if v.is_finite() { Ok(v) } else { Err(format!("`{expr}` is not finite ({v})")) }
}

/// Check syntax and names without values (every name set to 1).
pub fn check(expr: &str, names: &[&str]) -> Result<(), String> {
    let vars = variables(expr).into_iter().map(|n| (n, 1.0)).collect::<BTreeMap<_, _>>();
    if let Some(bad) = vars.keys().find(|n| !names.contains(&n.as_str())) {
        return Err(format!("`{expr}`: unknown name `{bad}` (known: {})", names.join(", ")));
    }
    let source = floats(expr);
    engine().compile_expression(&source).map_err(|e| format!("`{expr}`: {e}"))?;
    Ok(())
}

/// The expression with each name replaced by `with(name)` and `*` shown as
/// `×`: `k * i` → `0.012 × 3.29`.
pub fn substitute(expr: &str, with: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    for token in tokens(expr) {
        match token {
            Token::Name(n) => out.push_str(&with(&n)),
            Token::Other(c) if c == '*' => out.push_str(" × "),
            Token::Other(c) if c == '/' => out.push_str(" / "),
            Token::Other(c) if c == '+' || c == '-' => {
                // Binary operators get spaces; a leading sign does not.
                if out.trim_end().is_empty() || out.trim_end().ends_with(['(', '×', '/', '+', '-']) {
                    out.push(c);
                } else {
                    out.push_str(&format!(" {c} "));
                }
            }
            Token::Other(c) if c.is_whitespace() => {}
            Token::Other(c) => out.push(c),
            Token::Number(n) => out.push_str(&n),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")")
}

enum Token {
    Name(String),
    Number(String),
    Other(char),
}

const FUNCTIONS: [&str; 16] = ["sqrt", "abs", "sin", "cos", "tan", "asin", "acos", "atan", "exp", "ln", "log", "min", "max", "pi", "floor", "round"];

fn tokens(expr: &str) -> Vec<Token> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            // Function names (followed by `(`) and method calls are not variables.
            let call = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&'(');
            let method = start > 0 && chars[start - 1] == '.';
            if (call && FUNCTIONS.contains(&word.as_str())) || method {
                out.push(Token::Number(word));
            } else {
                out.push(Token::Name(word));
            }
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == 'e' || chars[i] == 'E' || ((chars[i] == '-' || chars[i] == '+') && matches!(chars[i - 1], 'e' | 'E'))) {
                i += 1;
            }
            out.push(Token::Number(chars[start..i].iter().collect()));
        } else {
            out.push(Token::Other(c));
            i += 1;
        }
    }
    out
}

/// Integer literals as floats (`2` → `2.0`), leaving names and exponents alone.
fn floats(expr: &str) -> String {
    tokens(expr)
        .into_iter()
        .map(|t| match t {
            Token::Number(n) if n.chars().all(|c| c.is_ascii_digit()) => format!("{n}.0"),
            Token::Number(n) if n.chars().next().is_some_and(|c| c.is_ascii_digit()) && (n.contains('e') || n.contains('E')) && !n.contains('.') => {
                let (m, e) = n.split_once(['e', 'E']).unwrap_or((&n, "0"));
                format!("({m}.0 * 10.0 ** {e}.0)")
            }
            Token::Number(n) => n,
            Token::Name(n) => n,
            Token::Other(c) => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vars(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }
    #[test]
    fn evaluates_formulas_with_names_and_math() {
        assert!((eval("k * i", &vars(&[("k", 0.012), ("i", 3.29)])).unwrap() - 0.03948).abs() < 1e-12);
        assert_eq!(eval("1/2", &BTreeMap::new()).unwrap(), 0.5);
        assert!((eval("(V - k*w) / R", &vars(&[("V", 12.), ("k", 0.012), ("w", 500.), ("R", 1.8)])).unwrap() - 3.333333333).abs() < 1e-6);
        assert!((eval("sqrt(x) + atan(1.0) * 4.0", &vars(&[("x", 9.)])).unwrap() - (3. + std::f64::consts::PI)).abs() < 1e-12);
        assert!((eval("2e-3 * x", &vars(&[("x", 5.)])).unwrap() - 0.01).abs() < 1e-15);
        assert!(eval("k * j", &vars(&[("k", 1.)])).unwrap_err().contains("unknown name `j`"));
        assert!(eval("1 / 0", &BTreeMap::new()).is_err());
        assert!(check("k * (i", &["k", "i"]).is_err());
        assert!(check("k * i", &["k"]).unwrap_err().contains("`i`"));
        assert_eq!(variables("sqrt(a*a + b*b)"), vec!["a", "b"]);
        assert_eq!(substitute("k * i", |n| if n == "k" { "0.012".into() } else { "3.29".into() }), "0.012 × 3.29");
        assert_eq!(substitute("(V - k*w) / R", |n| n.to_uppercase()), "(V - K × W) / R");
    }
    #[test]
    fn runaway_scripts_are_not_expressions() {
        assert!(eval("loop { }", &BTreeMap::new()).is_err());
        assert!(eval("x = 3", &vars(&[("x", 1.)])).is_err());
    }
}
