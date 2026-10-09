//! Transport-neutral hardware request vocabulary and status contracts.
//! Browser adapters and native in-process sessions share these types.
pub mod bench;
pub mod calibration;
use serde_json::Value;
use std::time::Duration;
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const STOP_TIMEOUT: Duration = Duration::from_secs(12);
pub const CALIBRATION_MAX_BODY: usize = 4096;
pub const MOTOR_BENCH_MAX_BODY: usize = 8192;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerKind {
    Calibration,
    MotorBench,
}
/// Shared refusal categories. `Server` preserves compatibility status/error meanings;
/// it does not imply any network transport in an in-process session.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientError {
    NotLoopback(String),
    Transport(String),
    Server { status: u16, error: String },
    Decode(String),
}
impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLoopback(e) | Self::Transport(e) | Self::Decode(e) => f.write_str(e),
            Self::Server { error, .. } => f.write_str(error),
        }
    }
}
impl std::error::Error for ClientError {}
pub fn new_client_id() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .expect("hardware identity entropy unavailable");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let h: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}
pub fn process_client_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(new_client_id)
}
pub fn next_connection_generation() -> u64 {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
/// A JSON value whose object members keep the order they were written in
/// (serde_json's `Map` sorts keys; the pages' `JSON.stringify` does not).
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// A scalar, or any value whose member order does not matter. Written
    /// by serde_json: use it for strings, booleans and integers; make a
    /// non-integer `f64` with [`js_number`] (`Json::from(f64)` lands here and
    /// would not be written as JavaScript writes it).
    Value(Value),
    /// A number written as JavaScript writes it ([`js_number_text`]); made
    /// by [`js_number`]. serde_json writes some numbers differently
    /// (`3.2e-6` for JavaScript's `0.0000032`, every digit of `2^60`).
    Number(f64),
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
}
impl Json {
    /// The compact text, members in order (`JSON.stringify` without spacing).
    pub fn write(&self, out: &mut String) {
        match self {
            Json::Value(v) => out.push_str(&v.to_string()),
            Json::Number(x) => out.push_str(&js_number_text(*x)),
            Json::Object(members) => {
                out.push('{');
                for (i, (k, v)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&Value::from(k.as_str()).to_string());
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write(out);
                }
                out.push(']');
            }
        }
    }
    /// As a `serde_json::Value` (member order lost; for checks and reading).
    pub fn to_value(&self) -> Value {
        match self {
            Json::Value(v) => v.clone(),
            // What a server reading the written text gets (serde_json reads
            // `5` as an integer, `1152921504606847000` as that integer).
            Json::Number(x) => serde_json::from_str(&js_number_text(*x)).unwrap_or(Value::Null),
            Json::Object(members) => Value::Object(
                members
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_value()))
                    .collect(),
            ),
            Json::Array(items) => Value::Array(items.iter().map(Json::to_value).collect()),
        }
    }
}
impl<T: Into<Value>> From<T> for Json {
    fn from(v: T) -> Self {
        Json::Value(v.into())
    }
}

/// A request body: an object with its members in the page's order.
#[derive(Clone, Debug, PartialEq)]
pub struct Body(pub Vec<(String, Json)>);
impl Body {
    pub fn new(members: Vec<(&str, Json)>) -> Self {
        Body(
            members
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        )
    }
    /// `{}` (the bench's `/stop`).
    pub fn empty() -> Self {
        Body(Vec::new())
    }
    /// The exact bytes sent.
    pub fn text(&self) -> String {
        let mut out = String::new();
        Json::Object(self.0.clone()).write(&mut out);
        out
    }
    pub fn to_value(&self) -> Value {
        Json::Object(self.0.clone()).to_value()
    }
    /// A member's value, if present.
    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}
impl From<Body> for Json {
    fn from(b: Body) -> Self {
        Json::Object(b.0)
    }
}

/// A number as JavaScript's `JSON.stringify` writes it ([`js_number_text`]);
/// non-finite values are `null`, as in JavaScript.
pub fn js_number(x: f64) -> Json {
    if x.is_finite() {
        Json::Number(x)
    } else {
        Json::Value(Value::Null)
    }
}

/// The text `JSON.stringify` writes for `x`: ECMAScript
/// `Number::toString(x)` (ECMA-262 §6.1.6.1.20) for finite values, with
/// `-0` as `0`; `null` for NaN and the infinities.
///
/// With `k` the shortest round-trip digits `d₁…d_k` and `n` the decimal
/// exponent (the value is `0.d₁…d_k × 10ⁿ`): `k ≤ n ≤ 21` writes the
/// digits then `n − k` zeros; `0 < n ≤ 21` puts the point after `n`
/// digits; `−6 < n ≤ 0` writes `0.`, `−n` zeros and the digits; otherwise
/// `d₁[.d₂…d_k]e±(n−1)`. So `0.0000032`, `1e-7`, `1e+21`, and `2^60` as
/// `1152921504606847000`. The digits are Rust's shortest round-trip digits
/// (`{:e}`), the same set JavaScript requires (the shortest `k`, and among
/// those the closest to `x`).
pub fn js_number_text(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let k = digits.len() as i64;
    let n = exponent.parse::<i64>().unwrap_or(0) + 1;
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if n - 1 >= 0 { '+' } else { '-' });
        out.push_str(&(n - 1).abs().to_string());
    }
    out
}

/// `deserialize_with` for a status field: a value of the wrong shape reads
/// as the field's default instead of failing the whole answer, so one
/// malformed section (a server change, a partial write) cannot hide the rest.
pub fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// `deserialize_with` for a list: the items that parse (a malformed item is
/// dropped, not the list); anything but an array reads as empty.
pub fn lenient_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = <Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(match value {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        _ => Vec::new(),
    })
}

/// `encodeURIComponent`: everything but `A–Z a–z 0–9 - _ . ! ~ * ' ( )` is
/// percent-encoded as UTF-8.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
