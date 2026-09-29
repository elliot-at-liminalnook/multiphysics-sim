//! Numbers in lesson prose that come from the model:
//!
//! ```text
//! The rotor settles at {{value scene=load-step observe=rotor.shaft.speed reduce=mean window=0.25..0.3 | 444 rad/s}}.
//! For our motor, k = {{param motor/motor.torque_constant | 0.012 N·m/A}}.
//! At 15 % drive the knee turned at {{data knee-steady duty=0.15 | 0.45 rad/s}}.
//! ```
//!
//! `data` reads a `sim-measured` block's data file: the mean of the measured
//! points whose field equals the given value.
//!
//! The text after `|` is what a plain Markdown reader sees and what the
//! viewer shows until the value is known; the viewer then prints the model's
//! value with the same number of figures and the same unit. `sim-lesson
//! check` fails when the written number disagrees with the model (within
//! `tol=`, default: the written number's last digit, or 1 %).
use crate::units;
use crate::Reduce;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A reduction of a scene's recorded run.
    Value { scene: String, observe: String, reduce: Reduce, window: Option<[f64; 2]> },
    /// A parameter of a system: `system/instance/path.parameter` (the
    /// system name may be left out when the lesson has one system).
    Param { target: String },
    /// Mean measured value of a `sim-measured` block at `field = at`.
    Data { block: String, field: String, at: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineRef {
    pub source: Source,
    /// What the author wrote after `|`, e.g. `444 rad/s`.
    pub shown: String,
    /// Relative tolerance for `check` (`tol=2%`), else the default.
    pub tolerance: Option<f64>,
    /// Byte range of the whole `{{…}}` in the block text.
    pub start: usize,
    pub end: usize,
}
impl InlineRef {
    /// A stable key for caching resolved values.
    pub fn key(&self) -> String {
        match &self.source {
            Source::Value { scene, observe, reduce, window } => format!("value:{scene}:{observe}:{reduce:?}:{window:?}"),
            Source::Param { target } => format!("param:{target}"),
            Source::Data { block, field, at } => format!("data:{block}:{field}={at}"),
        }
    }
    /// The shown number and unit (`444`, `rad/s`).
    pub fn shown_quantity(&self) -> Option<(f64, String)> {
        units::split_quantity(&self.shown)
    }
}

fn reduce_named(s: &str) -> Option<Reduce> {
    serde_norway::from_str::<Reduce>(s).ok()
}

/// Every `{{…}}` in `text`. Errors carry the byte offset of the bad one.
pub fn find(text: &str) -> Result<Vec<InlineRef>, (usize, String)> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(open) = text[at..].find("{{").map(|i| i + at) {
        let close = text[open..].find("}}").map(|i| i + open).ok_or((open, "`{{` is never closed with `}}`".to_string()))?;
        let body = &text[open + 2..close];
        let (spec, shown) = body.split_once('|').ok_or((open, format!("`{{{{{body}}}}}` needs `| shown text` (what a reader sees before the value is known)")))?;
        let shown = shown.trim().to_string();
        if units::split_quantity(&shown).is_none() {
            return Err((open, format!("`{shown}`: the shown text must start with a number, e.g. `444 rad/s`")));
        }
        let mut words = spec.split_whitespace();
        let kind = words.next().unwrap_or("");
        let mut fields = std::collections::BTreeMap::new();
        let mut bare = Vec::new();
        for w in words {
            match w.split_once('=') {
                Some((k, v)) => {
                    fields.insert(k.to_string(), v.to_string());
                }
                None => bare.push(w.to_string()),
            }
        }
        let tolerance = match fields.remove("tol") {
            Some(t) => Some(t.trim_end_matches('%').parse::<f64>().ok().filter(|v| *v >= 0.0).map(|v| if t.ends_with('%') { v / 100.0 } else { v }).ok_or((open, format!("tol=`{t}` must be a percentage like 2%")))?),
            None => None,
        };
        let source = match kind {
            "value" => {
                let need = |k: &str, fields: &mut std::collections::BTreeMap<String, String>| fields.remove(k).ok_or((open, format!("{{{{value …}}}} needs {k}=")));
                let scene = need("scene", &mut fields)?;
                let observe = need("observe", &mut fields)?;
                let reduce_text = fields.remove("reduce").unwrap_or_else(|| "final".into());
                let reduce = reduce_named(&reduce_text).ok_or((open, format!("reduce=`{reduce_text}` (final, mean, max, min, peak, change or integral)")))?;
                let window = match fields.remove("window") {
                    Some(w) => {
                        let (a, b) = w.split_once("..").ok_or((open, format!("window=`{w}` must be start..end in seconds")))?;
                        let (a, b) = (a.parse::<f64>().map_err(|_| (open, format!("window start `{a}`")))?, b.parse::<f64>().map_err(|_| (open, format!("window end `{b}`")))?);
                        if !(a < b) {
                            return Err((open, format!("window {a}..{b}: start must be before end")));
                        }
                        Some([a, b])
                    }
                    None => None,
                };
                Source::Value { scene, observe, reduce, window }
            }
            "param" => {
                let target = bare.first().cloned().ok_or((open, "{{param …}} names system/instance.parameter".to_string()))?;
                if !target.contains('.') {
                    return Err((open, format!("`{target}`: write instance/path.parameter")));
                }
                Source::Param { target }
            }
            "data" => {
                let block = bare.first().cloned().ok_or((open, "{{data …}} names a sim-measured block".to_string()))?;
                let (field, at) = fields.pop_first().ok_or((open, "{{data BLOCK field=value | …}} names the point, e.g. duty=0.15".to_string()))?;
                let at = at.parse::<f64>().map_err(|_| (open, format!("{field}=`{at}` must be a number")))?;
                Source::Data { block, field, at }
            }
            other => return Err((open, format!("`{{{{{other} …}}}}`: use {{{{value …}}}}, {{{{param …}}}} or {{{{data …}}}}"))),
        };
        if let Some(k) = fields.keys().next() {
            return Err((open, format!("unknown field `{k}=` in {{{{{kind} …}}}}")));
        }
        out.push(InlineRef { source, shown, tolerance, start: open, end: close + 2 });
        at = close + 2;
    }
    Ok(out)
}

/// `text` with every reference replaced: by `resolve` when it gives a
/// value (SI), printed like the shown text; else by the shown text.
pub fn render(text: &str, resolve: impl Fn(&InlineRef) -> Option<f64>) -> String {
    let Ok(refs) = find(text) else { return text.to_string() };
    let mut out = String::new();
    let mut at = 0;
    for r in refs {
        out.push_str(&text[at..r.start]);
        out.push_str(&resolve(&r).map(|v| format_like(&r.shown, v)).unwrap_or_else(|| r.shown.clone()));
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Plain text with shown values (for indexes, word counts and search).
pub fn plain(text: &str) -> String {
    render(text, |_| None)
}

/// `value` (SI) written like `shown`: same unit (with its prefix), same
/// number of decimals, and any leading `≈`/`~` kept.
pub fn format_like(shown: &str, value: f64) -> String {
    let lead: String = shown.chars().take_while(|c| matches!(c, '≈' | '~' | ' ')).collect();
    let Some((number, unit)) = units::split_quantity(shown) else { return shown.to_string() };
    let scale = units::parse_unit(&unit).map(|u| u.factor).unwrap_or(1.0);
    let v = value / scale;
    let body = shown[lead.len()..].trim_start();
    let written = body.split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | ',' | 'e' | 'E'))).next().unwrap_or("");
    let decimals = written.split_once('.').map(|(_, d)| d.chars().take_while(|c| c.is_ascii_digit()).count()).unwrap_or(0);
    let _ = number;
    let text = format!("{v:.decimals$}");
    if unit.is_empty() { format!("{lead}{text}") } else { format!("{lead}{text} {unit}") }
}

/// Does the shown number agree with `value` (SI)? Within `tolerance`
/// (relative) when given; else within half its last written digit or 1 %.
pub fn agrees(r: &InlineRef, value: f64) -> Result<bool, String> {
    let (number, unit) = r.shown_quantity().ok_or("the shown text has no number")?;
    let scale = units::parse_unit(&unit).map_err(|e| format!("`{}`: {e}", r.shown))?.factor;
    let v = value / scale;
    let diff = (v - number).abs();
    if let Some(t) = r.tolerance {
        return Ok(diff <= t * v.abs() + 1e-12);
    }
    let written = format_like(&r.shown, number * scale);
    let decimals = written.split_whitespace().find(|w| w.parse::<f64>().is_ok()).and_then(|w| w.split_once('.').map(|(_, d)| d.len())).unwrap_or(0);
    let half = 0.5 * 10f64.powi(-(decimals as i32));
    Ok(diff <= half * (1.0 + 1e-9) || diff <= 0.01 * v.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEXT: &str = "Settles at {{value scene=load-step observe=rotor.shaft.speed reduce=mean window=0.25..0.3 | 444 rad/s}}; k = {{param motor/motor.torque_constant | 0.012 N·m/A}}, τ ≈ {{value scene=s observe=motor.torque tol=2% | ≈ 39.5 mN·m}}.";
    #[test]
    fn references_parse_render_and_agree() {
        let refs = find(TEXT).unwrap();
        assert_eq!(refs.len(), 3);
        assert_eq!(refs[0].source, Source::Value { scene: "load-step".into(), observe: "rotor.shaft.speed".into(), reduce: Reduce::Mean, window: Some([0.25, 0.3]) });
        assert_eq!(refs[1].source, Source::Param { target: "motor/motor.torque_constant".into() });
        assert_eq!(refs[2].tolerance, Some(0.02));
        assert_eq!(plain(TEXT), "Settles at 444 rad/s; k = 0.012 N·m/A, τ ≈ ≈ 39.5 mN·m.");
        let shown = render(TEXT, |r| Some(match r.source { Source::Param { .. } => 0.01213, _ if r.shown.contains("mN") => 0.03948, _ => 444.83 }));
        assert_eq!(shown, "Settles at 445 rad/s; k = 0.012 N·m/A, τ ≈ ≈ 39.5 mN·m.");
        assert!(agrees(&refs[0], 444.4).unwrap());
        assert!(agrees(&refs[0], 447.0).unwrap(), "within 1 %");
        assert!(!agrees(&refs[0], 470.0).unwrap());
        assert!(agrees(&refs[1], 0.01213).unwrap());
        assert!(agrees(&refs[2], 0.0400).unwrap() && !agrees(&refs[2], 0.042).unwrap());
    }
    #[test]
    fn mistakes_name_themselves() {
        assert!(find("{{value scene=s | 3 A}}").unwrap_err().1.contains("observe="));
        assert!(find("{{value scene=s observe=x}}").unwrap_err().1.contains("| shown text"));
        assert!(find("{{param motor | 3 A}}").unwrap_err().1.contains("instance/path.parameter"));
        assert!(find("{{value scene=s observe=x reduce=avg | 3}}").unwrap_err().1.contains("reduce="));
        assert!(find("{{value scene=s observe=x | about three}}").unwrap_err().1.contains("start with a number"));
        assert!(find("{{guess x | 3}}").unwrap_err().1.contains("use"));
        assert!(find("open {{ never").is_err());
        assert!(find("no refs here").unwrap().is_empty());
    }
}
