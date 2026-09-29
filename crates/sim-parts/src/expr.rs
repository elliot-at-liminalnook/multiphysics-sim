//! Expressions of authored parts: parsing, dimension inference and forward-
//! mode differentiation (dual numbers), which gives each part an exact
//! Jacobian without anyone writing derivatives.
use crate::units::Dim;

/// A value the equations read at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Var {
    Param(usize),
    State(usize),
    StateRate(usize),
    /// Acausal port index, lane 0 (voltage, angle, position, temperature).
    Across(usize),
    /// Its rate (angular or linear speed).
    AcrossRate(usize),
    Signal(usize),
    Time,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Var(Var),
    Neg(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    /// Integer power (dimensions scale by it).
    Pow(Box<Expr>, i8),
    Call(Func, Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Exp,
    Ln,
    Sqrt,
    Abs,
    Sign,
    Floor,
    Tanh,
    Sin,
    Cos,
    Atan,
    Min,
    Max,
}

impl Func {
    fn by_name(n: &str) -> Option<(Func, usize)> {
        Some(match n {
            "exp" => (Func::Exp, 1),
            "ln" => (Func::Ln, 1),
            "sqrt" => (Func::Sqrt, 1),
            "abs" => (Func::Abs, 1),
            "sign" => (Func::Sign, 1),
            "floor" => (Func::Floor, 1),
            "tanh" => (Func::Tanh, 1),
            "sin" => (Func::Sin, 1),
            "cos" => (Func::Cos, 1),
            "atan" => (Func::Atan, 1),
            "min" => (Func::Min, 2),
            "max" => (Func::Max, 2),
            _ => return None,
        })
    }
}

/// Resolves names while parsing: parameters, states, lets, ports.
pub trait Names {
    /// A bare identifier (parameter, state, let, `t`).
    fn ident(&self, name: &str) -> Result<Expr, String>;
    /// `port.variable`.
    fn member(&self, port: &str, variable: &str) -> Result<Expr, String>;
    /// `der(state)`.
    fn derivative(&self, name: &str) -> Result<Expr, String>;
}

struct Parser<'a, N: Names> {
    chars: Vec<char>,
    at: usize,
    names: &'a N,
}

pub fn parse(source: &str, names: &impl Names) -> Result<Expr, String> {
    let mut p = Parser { chars: source.chars().collect(), at: 0, names };
    let e = p.sum()?;
    p.skip();
    if p.at < p.chars.len() {
        return Err(format!("unexpected `{}`", p.chars[p.at..].iter().collect::<String>()));
    }
    Ok(e)
}

impl<N: Names> Parser<'_, N> {
    fn skip(&mut self) {
        while self.at < self.chars.len() && self.chars[self.at].is_whitespace() {
            self.at += 1;
        }
    }
    fn peek(&mut self) -> Option<char> {
        self.skip();
        self.chars.get(self.at).copied()
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn sum(&mut self) -> Result<Expr, String> {
        let mut e = self.product()?;
        loop {
            if self.eat('+') {
                e = Expr::Add(Box::new(e), Box::new(self.product()?));
            } else if self.eat('-') {
                e = Expr::Sub(Box::new(e), Box::new(self.product()?));
            } else {
                return Ok(e);
            }
        }
    }
    fn product(&mut self) -> Result<Expr, String> {
        let mut e = self.unary()?;
        loop {
            if self.eat('*') || self.eat('·') {
                e = Expr::Mul(Box::new(e), Box::new(self.unary()?));
            } else if self.eat('/') {
                e = Expr::Div(Box::new(e), Box::new(self.unary()?));
            } else {
                return Ok(e);
            }
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat('-') {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }
    fn power(&mut self) -> Result<Expr, String> {
        let base = self.atom()?;
        if self.eat('^') {
            let negative = self.eat('-');
            self.skip();
            let start = self.at;
            while self.at < self.chars.len() && self.chars[self.at].is_ascii_digit() {
                self.at += 1;
            }
            let n: i8 = self.chars[start..self.at].iter().collect::<String>().parse().map_err(|_| "exponents must be integers (use sqrt or exp/ln for others)".to_string())?;
            return Ok(Expr::Pow(Box::new(base), if negative { -n } else { n }));
        }
        if self.eat('²') {
            return Ok(Expr::Pow(Box::new(base), 2));
        }
        if self.eat('³') {
            return Ok(Expr::Pow(Box::new(base), 3));
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let e = self.sum()?;
                if !self.eat(')') {
                    return Err("missing `)`".into());
                }
                Ok(e)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.at < self.chars.len() && (self.chars[self.at].is_ascii_digit() || self.chars[self.at] == '.' || self.chars[self.at] == 'e' || self.chars[self.at] == 'E' || ((self.chars[self.at] == '-' || self.chars[self.at] == '+') && matches!(self.chars[self.at - 1], 'e' | 'E'))) {
                    self.at += 1;
                }
                let text: String = self.chars[start..self.at].iter().collect();
                text.parse().map(Expr::Num).map_err(|_| format!("bad number `{text}`"))
            }
            Some(c) if c.is_alphabetic() || c == '_' => {
                let name = self.ident();
                if self.eat('(') {
                    let mut args = Vec::new();
                    if name == "der" {
                        self.skip();
                        let inner = self.ident();
                        if !self.eat(')') {
                            return Err("der() takes one state name".into());
                        }
                        return self.names.derivative(&inner);
                    }
                    if !self.eat(')') {
                        loop {
                            args.push(self.sum()?);
                            if self.eat(')') {
                                break;
                            }
                            if !self.eat(',') {
                                return Err(format!("expected `,` or `)` in {name}(…)"));
                            }
                        }
                    }
                    let (f, arity) = Func::by_name(&name).ok_or_else(|| format!("unknown function `{name}` (available: exp ln sqrt abs sign tanh sin cos atan min max der)"))?;
                    if args.len() != arity {
                        return Err(format!("{name} takes {arity} argument(s)"));
                    }
                    return Ok(Expr::Call(f, args));
                }
                if self.peek() == Some('.') {
                    self.at += 1;
                    let member = self.ident();
                    return self.names.member(&name, &member);
                }
                self.names.ident(&name)
            }
            Some(c) => Err(format!("unexpected `{c}`")),
            None => Err("expression ends early".into()),
        }
    }
    fn ident(&mut self) -> String {
        self.skip();
        let start = self.at;
        while self.at < self.chars.len() && (self.chars[self.at].is_alphanumeric() || self.chars[self.at] == '_') {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }
}

/// Dimension of an expression; `None` for a bare literal zero, which adds
/// to anything. `dims` gives each variable's dimension.
pub fn dimension(e: &Expr, dims: &dyn Fn(Var) -> Dim) -> Result<Option<Dim>, String> {
    let same = |a: Option<Dim>, b: Option<Dim>, op: &str| -> Result<Option<Dim>, String> {
        match (a, b) {
            (Some(x), Some(y)) if x != y => Err(format!("cannot {op} {x} and {y}")),
            (Some(x), _) | (_, Some(x)) => Ok(Some(x)),
            _ => Ok(None),
        }
    };
    let dimensionless = |a: Option<Dim>, what: &str| -> Result<(), String> {
        match a {
            Some(d) if !d.is_none() => Err(format!("{what} needs a dimensionless argument, got {d}")),
            _ => Ok(()),
        }
    };
    Ok(match e {
        Expr::Num(v) => (*v != 0.0).then_some(Dim::NONE),
        Expr::Var(v) => Some(dims(*v)),
        Expr::Neg(a) => dimension(a, dims)?,
        Expr::Add(a, b) => same(dimension(a, dims)?, dimension(b, dims)?, "add")?,
        Expr::Sub(a, b) => same(dimension(a, dims)?, dimension(b, dims)?, "subtract")?,
        Expr::Mul(a, b) => match (dimension(a, dims)?, dimension(b, dims)?) {
            (Some(x), Some(y)) => Some(x.mul(y)),
            _ => None,
        },
        Expr::Div(a, b) => match (dimension(a, dims)?, dimension(b, dims)?) {
            (Some(x), Some(y)) => Some(x.div(y)),
            (None, _) => None,
            (Some(_), None) => return Err("division by a literal zero".into()),
        },
        Expr::Pow(a, n) => dimension(a, dims)?.map(|d| d.pow(*n)),
        Expr::Call(f, args) => match f {
            Func::Exp | Func::Ln | Func::Tanh | Func::Sin | Func::Cos | Func::Atan => {
                dimensionless(dimension(&args[0], dims)?, &format!("{f:?}").to_lowercase())?;
                Some(Dim::NONE)
            }
            Func::Sign => Some(Dim::NONE),
            Func::Floor => {
                dimensionless(dimension(&args[0], dims)?, "floor")?;
                Some(Dim::NONE)
            }
            Func::Abs => dimension(&args[0], dims)?,
            Func::Sqrt => match dimension(&args[0], dims)? {
                Some(d) => Some(d.sqrt().ok_or_else(|| format!("sqrt of {d} has no whole-number dimension"))?),
                None => None,
            },
            Func::Min | Func::Max => same(dimension(&args[0], dims)?, dimension(&args[1], dims)?, "compare")?,
        },
    })
}

/// A value and its derivative along one seeded variable.
#[derive(Debug, Clone, Copy)]
pub struct Dual {
    pub v: f64,
    pub d: f64,
}

/// Evaluate with `seed` (if any) as the differentiation variable.
pub fn eval(e: &Expr, read: &dyn Fn(Var) -> f64, seed: Option<Var>) -> Dual {
    let c = |v: f64| Dual { v, d: 0. };
    match e {
        Expr::Num(v) => c(*v),
        Expr::Var(v) => Dual { v: read(*v), d: if Some(*v) == seed { 1. } else { 0. } },
        Expr::Neg(a) => {
            let a = eval(a, read, seed);
            Dual { v: -a.v, d: -a.d }
        }
        Expr::Add(a, b) => {
            let (a, b) = (eval(a, read, seed), eval(b, read, seed));
            Dual { v: a.v + b.v, d: a.d + b.d }
        }
        Expr::Sub(a, b) => {
            let (a, b) = (eval(a, read, seed), eval(b, read, seed));
            Dual { v: a.v - b.v, d: a.d - b.d }
        }
        Expr::Mul(a, b) => {
            let (a, b) = (eval(a, read, seed), eval(b, read, seed));
            Dual { v: a.v * b.v, d: a.d * b.v + a.v * b.d }
        }
        Expr::Div(a, b) => {
            let (a, b) = (eval(a, read, seed), eval(b, read, seed));
            Dual { v: a.v / b.v, d: (a.d * b.v - a.v * b.d) / (b.v * b.v) }
        }
        Expr::Pow(a, n) => {
            let a = eval(a, read, seed);
            let n = *n as i32;
            Dual { v: a.v.powi(n), d: if n == 0 { 0. } else { n as f64 * a.v.powi(n - 1) * a.d } }
        }
        Expr::Call(f, args) => {
            let a = eval(&args[0], read, seed);
            match f {
                Func::Exp => {
                    let v = a.v.exp();
                    Dual { v, d: v * a.d }
                }
                Func::Ln => Dual { v: a.v.ln(), d: a.d / a.v },
                Func::Sqrt => {
                    let v = a.v.sqrt();
                    Dual { v, d: if v > 0. { a.d / (2. * v) } else { 0. } }
                }
                Func::Abs => Dual { v: a.v.abs(), d: if a.v > 0. { a.d } else if a.v < 0. { -a.d } else { 0. } },
                Func::Sign => c(if a.v > 0. { 1. } else if a.v < 0. { -1. } else { 0. }),
                // Piecewise constant: zero derivative. Use it for schedules of
                // time (period counts), never on a solved unknown.
                Func::Floor => c(a.v.floor()),
                Func::Tanh => {
                    let v = a.v.tanh();
                    Dual { v, d: (1. - v * v) * a.d }
                }
                Func::Sin => Dual { v: a.v.sin(), d: a.v.cos() * a.d },
                Func::Cos => Dual { v: a.v.cos(), d: -a.v.sin() * a.d },
                Func::Atan => Dual { v: a.v.atan(), d: a.d / (1. + a.v * a.v) },
                Func::Min | Func::Max => {
                    let b = eval(&args[1], read, seed);
                    let pick_a = if *f == Func::Min { a.v <= b.v } else { a.v >= b.v };
                    if pick_a { a } else { b }
                }
            }
        }
    }
}

/// Every variable an expression reads.
pub fn vars(e: &Expr, out: &mut std::collections::BTreeSet<Var>) {
    match e {
        Expr::Num(_) => {}
        Expr::Var(v) => {
            out.insert(*v);
        }
        Expr::Neg(a) | Expr::Pow(a, _) => vars(a, out),
        Expr::Add(a, b) | Expr::Sub(a, b) | Expr::Mul(a, b) | Expr::Div(a, b) => {
            vars(a, out);
            vars(b, out);
        }
        Expr::Call(_, args) => args.iter().for_each(|a| vars(a, out)),
    }
}
