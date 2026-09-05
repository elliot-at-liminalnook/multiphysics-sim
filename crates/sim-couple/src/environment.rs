//! Environment mode: the seam turned around. A controller-driven loop
//! (`Coupler`) has the simulation ask the controller for a command at
//! every sample; a learner instead wants to be the master — `reset`, then
//! `step(action)` until `done` — and to hold several environments at once.
//! [`Environment`] is that contract on the simulation side, [`serve`]
//! speaks it over newline-delimited JSON (the same transport as the
//! controller protocol, so any language can train against it), and the
//! Python client is `simloop.Gym`.
//!
//! Protocol, one JSON object per line. Server first:
//!
//! ```text
//! {"hello": {"envs": 2, "period": 0.02, "obs": [...], "priv": [...], "act": [...]}}
//! ```
//!
//! then each request is answered with a frame per environment:
//!
//! ```text
//! -> {"reset": [{"seed": 1, "level": 0.0}, null]}      null keeps that environment as it is
//! <- {"obs": [[..],[..]], "priv": [[..],[..]], "t": [..], "done": [..], "terrain": [[[x0,x1,y],..], null]}
//! -> {"step": [[a..],[a..]]}
//! <- {"obs": .., "priv": .., "t": .., "done": .., "terrain": null}
//! -> {"snapshot": null}            <- {"snapshot": [[..],[..]]}
//! -> {"restore": [[..],[..]]}      <- the frame after restoring
//! -> {"close": null}
//! ```

use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

/// What a learner sees after a reset or a step.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Frame {
    /// The policy's observation: what a deployed controller could measure.
    pub obs: Vec<f64>,
    /// Privileged state for rewards, critics and teachers: ground truth.
    #[serde(rename = "priv")]
    pub privileged: Vec<f64>,
    pub t: f64,
    pub done: bool,
    /// The terrain of a fresh episode as `(x0, x1, y)` patches; `None`
    /// after a step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terrain: Option<Vec<(f64, f64, f64)>>,
}

/// Channel names and the policy period, sent once.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spaces {
    pub period: f64,
    pub obs: Vec<String>,
    #[serde(rename = "priv")]
    pub privileged: Vec<String>,
    pub act: Vec<String>,
}

pub trait Environment: Send {
    fn spaces(&self) -> Spaces;
    /// Start an episode: `seed` fixes the terrain and randomisation,
    /// `level` in `[0, 1]` is the curriculum knob.
    fn reset(&mut self, seed: u64, level: f64) -> Result<Frame, String>;
    /// Apply `action` for one policy period.
    fn step(&mut self, action: &[f64]) -> Result<Frame, String>;
    /// The state as numbers, to come back to with [`Self::restore`].
    fn snapshot(&self) -> Vec<f64>;
    fn restore(&mut self, snapshot: &[f64]) -> Result<Frame, String>;
}

#[derive(Deserialize)]
struct ResetRequest {
    seed: u64,
    #[serde(default)]
    level: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Request {
    Reset(Vec<Option<ResetRequest>>),
    Step(Vec<Vec<f64>>),
    Snapshot(#[allow(dead_code)] Option<()>),
    Restore(Vec<Vec<f64>>),
    Close(#[allow(dead_code)] Option<()>),
}

#[derive(Serialize)]
struct Batch<'a> {
    obs: Vec<&'a [f64]>,
    #[serde(rename = "priv")]
    privileged: Vec<&'a [f64]>,
    t: Vec<f64>,
    done: Vec<bool>,
    terrain: Option<Vec<Option<&'a Vec<(f64, f64, f64)>>>>,
}

fn write_json(writer: &mut impl Write, value: &impl Serialize) -> std::io::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Serve `envs` over `reader`/`writer` until `close` or end of input. The
/// environments step on their own threads, so a batch costs one step's
/// wall time.
pub fn serve(mut envs: Vec<Box<dyn Environment>>, reader: impl BufRead, writer: &mut impl Write) -> std::io::Result<()> {
    let spaces = envs.first().map(|e| e.spaces()).unwrap_or(Spaces { period: 0.0, obs: vec![], privileged: vec![], act: vec![] });
    write_json(writer, &serde_json::json!({ "hello": { "envs": envs.len(), "period": spaces.period, "obs": spaces.obs, "priv": spaces.privileged, "act": spaces.act } }))?;
    let mut frames: Vec<Frame> = vec![Frame::default(); envs.len()];
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                write_json(writer, &serde_json::json!({ "error": format!("malformed request: {e}") }))?;
                continue;
            }
        };
        let outcome: Result<(), String> = match request {
            Request::Close(_) => break,
            Request::Snapshot(_) => {
                let snapshots: Vec<Vec<f64>> = envs.iter().map(|e| e.snapshot()).collect();
                write_json(writer, &serde_json::json!({ "snapshot": snapshots }))?;
                continue;
            }
            Request::Reset(requests) => parallel(&mut envs, &mut frames, |k, env, frame| {
                if let Some(Some(r)) = requests.get(k) {
                    *frame = env.reset(r.seed, r.level)?;
                } else {
                    frame.terrain = None;
                }
                Ok(())
            }),
            Request::Step(actions) => parallel(&mut envs, &mut frames, |k, env, frame| {
                let action = actions.get(k).ok_or_else(|| format!("no action for environment {k}"))?;
                *frame = env.step(action)?;
                Ok(())
            }),
            Request::Restore(snapshots) => parallel(&mut envs, &mut frames, |k, env, frame| {
                let snapshot = snapshots.get(k).ok_or_else(|| format!("no snapshot for environment {k}"))?;
                *frame = env.restore(snapshot)?;
                Ok(())
            }),
        };
        if let Err(message) = outcome {
            write_json(writer, &serde_json::json!({ "error": message }))?;
            continue;
        }
        let any_terrain = frames.iter().any(|f| f.terrain.is_some());
        let batch = Batch {
            obs: frames.iter().map(|f| f.obs.as_slice()).collect(),
            privileged: frames.iter().map(|f| f.privileged.as_slice()).collect(),
            t: frames.iter().map(|f| f.t).collect(),
            done: frames.iter().map(|f| f.done).collect(),
            terrain: any_terrain.then(|| frames.iter().map(|f| f.terrain.as_ref()).collect()),
        };
        write_json(writer, &batch)?;
    }
    Ok(())
}

/// Run `f` on every environment on its own thread; the first error wins.
fn parallel(envs: &mut [Box<dyn Environment>], frames: &mut [Frame], f: impl Fn(usize, &mut Box<dyn Environment>, &mut Frame) -> Result<(), String> + Sync) -> Result<(), String> {
    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = envs.iter_mut().zip(frames.iter_mut()).enumerate().map(|(k, (env, frame))| {
            let f = &f;
            scope.spawn(move || f(k, env, frame))
        }).collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("environment thread panicked".into()))).collect()
    });
    results.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter that reports its state: enough to exercise the protocol.
    struct Counter {
        value: f64,
        level: f64,
    }
    impl Environment for Counter {
        fn spaces(&self) -> Spaces {
            Spaces { period: 0.1, obs: vec!["value".into()], privileged: vec!["level".into()], act: vec!["delta".into()] }
        }
        fn reset(&mut self, seed: u64, level: f64) -> Result<Frame, String> {
            self.value = seed as f64;
            self.level = level;
            Ok(Frame { obs: vec![self.value], privileged: vec![self.level], t: 0.0, done: false, terrain: Some(vec![(0.0, 1.0, level)]) })
        }
        fn step(&mut self, action: &[f64]) -> Result<Frame, String> {
            self.value += action.first().copied().ok_or("no action")?;
            Ok(Frame { obs: vec![self.value], privileged: vec![self.level], t: 0.1, done: self.value > 10.0, terrain: None })
        }
        fn snapshot(&self) -> Vec<f64> {
            vec![self.value, self.level]
        }
        fn restore(&mut self, snapshot: &[f64]) -> Result<Frame, String> {
            self.value = snapshot[0];
            self.level = snapshot[1];
            Ok(Frame { obs: vec![self.value], privileged: vec![self.level], t: 0.0, done: false, terrain: None })
        }
    }

    #[test]
    fn protocol_round_trip() {
        let envs: Vec<Box<dyn Environment>> = vec![Box::new(Counter { value: 0.0, level: 0.0 }), Box::new(Counter { value: 0.0, level: 0.0 })];
        let script = concat!(
            "{\"reset\": [{\"seed\": 3, \"level\": 0.5}, {\"seed\": 4}]}\n",
            "{\"step\": [[1.0], [2.0]]}\n",
            "{\"snapshot\": null}\n",
            "{\"step\": [[9.0], [0.0]]}\n",
            "{\"restore\": [[3.0, 0.5], [4.0, 0.0]]}\n",
            "{\"step\": [[1.0]]}\n",
            "{\"close\": null}\n",
        );
        let mut out = Vec::new();
        serve(envs, std::io::Cursor::new(script), &mut out).unwrap();
        let lines: Vec<serde_json::Value> = String::from_utf8(out).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(lines[0]["hello"]["envs"], 2);
        assert_eq!(lines[0]["hello"]["act"][0], "delta");
        assert_eq!(lines[1]["obs"], serde_json::json!([[3.0], [4.0]]));
        assert_eq!(lines[1]["terrain"][0][0][2], 0.5);
        assert_eq!(lines[2]["obs"], serde_json::json!([[4.0], [6.0]]));
        assert!(lines[2]["terrain"].is_null());
        assert_eq!(lines[3]["snapshot"], serde_json::json!([[4.0, 0.5], [6.0, 0.0]]));
        assert_eq!(lines[4]["done"], serde_json::json!([true, false]));
        assert_eq!(lines[5]["obs"], serde_json::json!([[3.0], [4.0]]));
        assert!(lines[6]["error"].as_str().unwrap().contains("no action for environment 1"));
    }
}
