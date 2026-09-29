//! Narration audio for lesson explainers (`sim_lesson::narration`).
//!
//! - Speech: Gemini 3.8 Flash TTS through OpenRouter's `/audio/speech`
//!   (`openrouter-rs`), 24 kHz mono 16-bit PCM, stored as WAV. The section's
//!   spoken text is sent verbatim (inline Gemini tags such as `<short pause>`
//!   included); tone goes in `speech_metadata.style`.
//! - Timing: the audio is transcribed with word timestamps
//!   (`openai/whisper-1`, `verbose_json`) and aligned to the script, so cues
//!   fire on their words. If transcription fails, timing is estimated from
//!   the audio's length and the entry says why.
//! - Cache: one WAV per section, keyed by the section hash; the manifest is
//!   updated after every section, so an interrupted run keeps what it made.
//! - Spend: nothing is generated unless asked; every run has a dollar
//!   ceiling checked against an estimate first, and reports the account's
//!   measured usage change afterwards. The key is read from
//!   `OPENROUTER_API_KEY` or `~/OPENROUTER_API_KEY` and never written.
pub mod player;

use openrouter_rs::OpenRouterClient;
use openrouter_rs::api::audio::{SpeechProviderOptions, SpeechRequest, SpeechResponseFormat, TranscriptionInputAudio, TranscriptionRequest};
use serde::Serialize;
use sim_lesson::narration::{Entry, Explainer, Section, TimingKind};
use std::collections::HashMap;
use std::path::PathBuf;

/// OpenRouter's PCM output for Gemini TTS (`audio/pcm;rate=24000;channels=1`).
pub const SAMPLE_RATE: u32 = 24_000;
/// Measured 2026-09-26: $0.0018 for 6.2 s of Gemini 3.8 Flash TTS audio.
pub const TTS_USD_PER_SECOND: f64 = 0.0003;
/// Measured 2026-09-26: $0.0007 for 7 s of whisper-1 transcription.
pub const ALIGN_USD_PER_SECOND: f64 = 0.0001;
/// Default spend ceiling for one generation run.
pub const DEFAULT_BUDGET_USD: f64 = 1.0;
/// Speech rate used to estimate a section's length before generating it.
const ESTIMATE_WPM: f64 = 140.0;

/// The API key: `OPENROUTER_API_KEY`, else the file `~/OPENROUTER_API_KEY`.
pub fn api_key() -> Result<String, String> {
    if let Ok(k) = std::env::var("OPENROUTER_API_KEY") {
        if !k.trim().is_empty() {
            return Ok(k.trim().to_string());
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no OPENROUTER_API_KEY and no HOME")?;
    let key = std::fs::read_to_string(home.join("OPENROUTER_API_KEY")).map_err(|_| "set OPENROUTER_API_KEY or put the key in ~/OPENROUTER_API_KEY".to_string())?;
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err("~/OPENROUTER_API_KEY is empty".into());
    }
    Ok(key)
}

/// Time-stretch mono speech by `rate` (1.25 = faster, 0.8 = slower) without
/// changing its pitch: WSOLA, 40 ms Hann frames overlapped by half, each
/// placed where it best continues the previous one (within ±10 ms).
pub fn time_stretch(samples: &[i16], rate: f64, sample_rate: u32) -> Vec<i16> {
    if samples.is_empty() || (rate - 1.0).abs() < 1e-6 {
        return samples.to_vec();
    }
    let frame = ((0.040 * sample_rate as f64) as usize).max(64) & !1;
    let hop_out = frame / 2;
    let hop_in = hop_out as f64 * rate;
    let tolerance = (0.010 * sample_rate as f64) as isize;
    let x: Vec<f32> = samples.iter().map(|s| *s as f32).collect();
    let window: Vec<f32> = (0..frame).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / frame as f32).cos()).collect();
    let frames = ((x.len() as f64 - frame as f64) / hop_in).max(0.0) as usize + 1;
    let mut out = vec![0f32; frames * hop_out + frame];
    let mut norm = vec![0f32; out.len()];
    let mut previous: isize = 0;
    for k in 0..frames {
        let nominal = (k as f64 * hop_in) as isize;
        // The best continuation of the previous frame's natural next hop.
        let natural = previous + hop_out as isize;
        let at = if k == 0 {
            0
        } else {
            let mut best = (f32::NEG_INFINITY, nominal);
            for d in -tolerance..=tolerance {
                let cand = nominal + d;
                if cand < 0 || cand as usize + frame > x.len() || natural < 0 || natural as usize + hop_out > x.len() {
                    continue;
                }
                let c: f32 = (0..hop_out).step_by(2).map(|i| x[cand as usize + i] * x[natural as usize + i]).sum();
                if c > best.0 {
                    best = (c, cand);
                }
            }
            best.1
        };
        let at = at.clamp(0, (x.len().saturating_sub(frame)) as isize) as usize;
        for i in 0..frame.min(x.len() - at) {
            out[k * hop_out + i] += x[at + i] * window[i];
            norm[k * hop_out + i] += window[i];
        }
        previous = at as isize;
    }
    let len = ((samples.len() as f64) / rate) as usize;
    out.iter().zip(&norm).take(len).map(|(v, n)| (v / n.max(1e-3)).clamp(i16::MIN as f32, i16::MAX as f32) as i16).collect()
}

/// A mono 16-bit WAV around raw PCM.
pub fn wav(pcm: &[u8], rate: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(pcm.len() + 44);
    let data = pcm.len() as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// PCM bytes from the speech endpoint; tolerates a WAV body (reads its
/// rate and data chunk) in case the provider wraps it.
pub fn pcm_of(bytes: &[u8]) -> (Vec<u8>, u32) {
    if bytes.len() > 44 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let mut i = 12;
        while i + 8 <= bytes.len() {
            let len = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
            if &bytes[i..i + 4] == b"data" {
                return (bytes[i + 8..(i + 8 + len).min(bytes.len())].to_vec(), rate);
            }
            i += 8 + len + (len & 1);
        }
    }
    (bytes.to_vec(), SAMPLE_RATE)
}

/// OpenRouter client with its own small async runtime (callers stay sync).
pub struct Voice {
    client: OpenRouterClient,
    runtime: tokio::runtime::Runtime,
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl Voice {
    pub fn new(key: &str) -> Result<Self, String> {
        Self::with_base_url(key, None)
    }
    /// `base_url` overrides `https://openrouter.ai/api/v1` (tests).
    pub fn with_base_url(key: &str, base_url: Option<&str>) -> Result<Self, String> {
        let mut builder = OpenRouterClient::builder();
        builder.api_key(key).x_title("physics-simulator lessons");
        if let Some(url) = base_url {
            builder.base_url(url);
        }
        let client = builder.build().map_err(text)?;
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(text)?;
        Ok(Self { client, runtime })
    }

    /// Spoken text → PCM (and its sample rate).
    pub fn speak(&self, model: &str, voice: &str, style: &str, input: &str) -> Result<(Vec<u8>, u32), String> {
        let mut builder = SpeechRequest::builder();
        builder.model(model).input(input).voice(voice).response_format(SpeechResponseFormat::Pcm);
        if !style.trim().is_empty() {
            let mut options = HashMap::new();
            options.insert("google-ai-studio".to_string(), serde_json::json!({"speech_metadata": {"style": style}}));
            builder.provider(SpeechProviderOptions::new(options));
        }
        let request = builder.build().map_err(text)?;
        let bytes = self.runtime.block_on(self.client.audio().speech().create(&request)).map_err(|e| format!("speech ({model}): {e}"))?;
        if bytes.len() < 2 {
            return Err(format!("speech ({model}) returned no audio"));
        }
        Ok(pcm_of(&bytes))
    }

    /// WAV → words with start and end times.
    pub fn transcribe(&self, model: &str, wav: &[u8]) -> Result<Vec<(String, f64, f64)>, String> {
        use base64::Engine;
        let audio = TranscriptionInputAudio::builder().data(base64::engine::general_purpose::STANDARD.encode(wav)).format("wav").build().map_err(text)?;
        let request = TranscriptionRequest::builder().model(model).input_audio(audio).response_format("verbose_json").timestamp_granularities(["word"]).build().map_err(text)?;
        let response = self.runtime.block_on(self.client.audio().transcriptions().create(&request)).map_err(|e| format!("transcription ({model}): {e}"))?;
        let words = response.words.unwrap_or_default();
        if words.is_empty() {
            return Err(format!("transcription ({model}) returned no word timings"));
        }
        Ok(words.into_iter().map(|w| (w.word, w.start, w.end)).collect())
    }

    /// Total account usage in dollars (for measured spend).
    pub fn usage(&self) -> Option<f64> {
        self.runtime.block_on(self.client.get_credits()).ok().map(|c| c.total_usage)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Audio matches the current text, voice and style.
    Current,
    /// Audio exists for older text or settings.
    Stale,
    Missing,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanItem {
    pub id: String,
    pub status: Status,
    pub chars: usize,
    pub words: usize,
    pub estimated_seconds: f64,
    pub estimated_usd: f64,
}

fn estimate(section: &Section, align: bool) -> (f64, f64) {
    let seconds = section.words.len() as f64 * 60.0 / ESTIMATE_WPM + section.pauses.iter().map(|p| p.1).sum::<f64>();
    (seconds, seconds * (TTS_USD_PER_SECOND + if align { ALIGN_USD_PER_SECOND } else { 0.0 }))
}

/// What exists and what generating would cost.
pub fn plan(explainer: &Explainer) -> Vec<PlanItem> {
    let manifest = explainer.manifest();
    let align = explainer.meta.align != "none";
    explainer
        .sections
        .iter()
        .map(|s| {
            let status = match (explainer.audio(&manifest, s), manifest.sections.contains_key(&s.id)) {
                (Some(_), _) => Status::Current,
                (None, true) => Status::Stale,
                (None, false) => Status::Missing,
            };
            let (estimated_seconds, estimated_usd) = estimate(s, align);
            PlanItem { id: s.id.clone(), status, chars: s.spoken.chars().count(), words: s.words.len(), estimated_seconds, estimated_usd }
        })
        .collect()
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub generated: Vec<String>,
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
    pub estimated_usd: f64,
    /// Change in the account's total usage over the run, when readable.
    pub measured_usd: Option<f64>,
    pub audio_seconds: f64,
}

pub struct Request<'a> {
    /// Sections to (re)generate; empty means every stale or missing one.
    pub sections: &'a [String],
    /// With no sections named: regenerate current audio too.
    pub force: bool,
    pub budget_usd: f64,
}

/// Generate audio for the requested sections. Checks the estimate against
/// the budget before spending; saves the manifest after each section.
pub fn generate(explainer: &Explainer, voice: &Voice, request: &Request, progress: &dyn Fn(&str)) -> Result<Report, String> {
    for id in request.sections {
        if explainer.section(id).is_none() {
            return Err(format!("no section `{id}` ({})", explainer.sections.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")));
        }
    }
    let plan = plan(explainer);
    // Named sections always regenerate; otherwise only stale or missing ones (all with force).
    let chosen: Vec<&PlanItem> = plan.iter().filter(|p| if request.sections.is_empty() { request.force || p.status != Status::Current } else { request.sections.contains(&p.id) }).collect();
    let mut report = Report { skipped: plan.iter().filter(|p| !chosen.iter().any(|c| c.id == p.id)).map(|p| p.id.clone()).collect(), ..Default::default() };
    report.estimated_usd = chosen.iter().map(|p| p.estimated_usd).sum();
    if report.estimated_usd > request.budget_usd {
        return Err(format!("estimated ${:.3} for {} section(s) exceeds the ${:.2} ceiling; raise --budget or pick sections", report.estimated_usd, chosen.len(), request.budget_usd));
    }
    if chosen.is_empty() {
        return Ok(report);
    }
    let dir = explainer.narration_dir();
    std::fs::create_dir_all(&dir).map_err(text)?;
    let before = voice.usage();
    let mut manifest = explainer.manifest();
    let align_model = (explainer.meta.align != "none").then(|| explainer.meta.align.clone());
    for item in chosen {
        let s = explainer.section(&item.id).unwrap();
        let voice_name = s.voice.clone().unwrap_or_else(|| explainer.meta.voice.clone());
        let style = [explainer.meta.style.as_str(), s.style.as_deref().unwrap_or("")].iter().filter(|x| !x.trim().is_empty()).copied().collect::<Vec<_>>().join("; ");
        progress(&format!("{}: speaking {} words ({voice_name})", s.id, s.words.len()));
        let (pcm, rate) = voice.speak(&explainer.meta.model, &voice_name, &style, &s.spoken)?;
        let audio = wav(&pcm, rate);
        let duration = pcm.len() as f64 / (2.0 * rate as f64);
        let (timing, words, note) = match &align_model {
            None => (TimingKind::Estimated, vec![], "alignment disabled (align: none)".to_string()),
            Some(model) => {
                progress(&format!("{}: aligning {:.1} s of audio", s.id, duration));
                match voice.transcribe(model, &audio) {
                    Ok(words) => (TimingKind::Aligned, words, String::new()),
                    Err(e) => {
                        report.warnings.push(format!("{}: {e}; cue times are estimated", s.id));
                        (TimingKind::Estimated, vec![], e)
                    }
                }
            }
        };
        let file = format!("{}-{}.wav", s.id, &s.hash[..8]);
        sim_annotate::store::write_atomic(&dir.join(&file), &audio)?;
        if let Some(old) = manifest.sections.get(&s.id).map(|e| e.file.clone()).filter(|f| *f != file) {
            let _ = std::fs::remove_file(dir.join(old));
        }
        let created_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0).to_string();
        manifest.sections.insert(s.id.clone(), Entry { hash: s.hash.clone(), file, duration_s: duration, sample_rate: rate, timing, words, model: explainer.meta.model.clone(), voice: voice_name, align_model: align_model.clone(), created_at, chars: item.chars, note });
        manifest.save(&dir)?;
        report.audio_seconds += duration;
        report.generated.push(s.id.clone());
    }
    // Usage updates asynchronously on OpenRouter's side; a short wait helps.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    report.measured_usd = before.zip(voice.usage()).map(|(a, b)| b - a);
    Ok(report)
}

#[cfg(test)]
mod stretch_tests {
    use super::*;
    fn crossings(x: &[i16]) -> usize {
        x.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count()
    }
    #[test]
    fn stretching_keeps_pitch_and_scales_length() {
        let rate = 24_000u32;
        let tone: Vec<i16> = (0..rate as usize).map(|i| (8000.0 * (std::f64::consts::TAU * 440.0 * i as f64 / rate as f64).sin()) as i16).collect();
        for speed in [0.8, 1.25] {
            let out = time_stretch(&tone, speed, rate);
            let expected = (tone.len() as f64 / speed) as usize;
            assert!((out.len() as i64 - expected as i64).abs() < 50, "{speed}: {} vs {expected}", out.len());
            // Same pitch: zero crossings per second unchanged (±3 %).
            let per_s = crossings(&out[2400..out.len() - 2400]) as f64 / ((out.len() - 4800) as f64 / rate as f64);
            assert!((per_s - 880.0).abs() < 0.03 * 880.0, "{speed}: {per_s} crossings/s");
        }
        assert_eq!(time_stretch(&tone, 1.0, rate), tone);
    }
}
