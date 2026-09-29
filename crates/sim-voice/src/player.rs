//! Narration playback on its own thread (rodio): load a WAV, play, pause,
//! seek, and report the playback position so the narration clock follows
//! the audio actually heard. A playback rate other than 1 time-stretches the
//! speech (WSOLA, pitch kept); positions are always in the original audio's
//! seconds, so word timings and cues stay valid at any rate. Without an output device every call is a
//! no-op and `state().error` says why; callers then run on their own clock.
use std::io::Cursor;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// Caller's key for the loaded audio (e.g. section ID + hash).
    pub loaded: Option<String>,
    pub position: f64,
    pub playing: bool,
    /// The loaded audio played to its end.
    pub finished: bool,
    pub error: Option<String>,
}

enum Msg {
    Load { key: String, bytes: Arc<Vec<u8>>, at: f64, play: bool, rate: f64 },
    Play,
    Pause,
    Seek(f64),
    Stop,
    Volume(f32),
}

pub struct Player {
    tx: mpsc::Sender<Msg>,
    state: Arc<Mutex<State>>,
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Player {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        std::thread::spawn(move || run(rx, shared));
        Self { tx, state }
    }
    pub fn load(&self, key: &str, bytes: Arc<Vec<u8>>, at: f64, play: bool) {
        self.load_at_rate(key, bytes, at, play, 1.0);
    }
    /// Load at a playback rate (0.5–2; 1 plays the file as it is).
    pub fn load_at_rate(&self, key: &str, bytes: Arc<Vec<u8>>, at: f64, play: bool, rate: f64) {
        let _ = self.tx.send(Msg::Load { key: key.into(), bytes, at, play, rate: rate.clamp(0.5, 2.0) });
    }
    pub fn play(&self) {
        let _ = self.tx.send(Msg::Play);
    }
    pub fn pause(&self) {
        let _ = self.tx.send(Msg::Pause);
    }
    pub fn seek(&self, at: f64) {
        let _ = self.tx.send(Msg::Seek(at));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Msg::Stop);
    }
    /// 0 mutes, 1 is full volume; kept across loads.
    pub fn set_volume(&self, v: f32) {
        let _ = self.tx.send(Msg::Volume(v.clamp(0., 1.)));
    }
    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn run(rx: mpsc::Receiver<Msg>, state: Arc<Mutex<State>>) {
    let output = rodio::OutputStream::try_default();
    let (_stream, handle) = match output {
        Ok(o) => o,
        Err(e) => {
            state.lock().unwrap().error = Some(format!("no audio output: {e}"));
            // Keep draining so senders never block or fail.
            while rx.recv().is_ok() {}
            return;
        }
    };
    let mut sink: Option<rodio::Sink> = None;
    // Played seconds per original second (1 / rate).
    let mut stretch = 1.0f64;
    let mut volume = 1.0f32;
    loop {
        match rx.recv_timeout(Duration::from_millis(15)) {
            Ok(Msg::Load { key, bytes, at, play, rate }) => {
                if let Some(old) = sink.take() {
                    old.stop();
                }
                stretch = 1.0 / rate;
                let made = rodio::Sink::try_new(&handle).map_err(|e| e.to_string()).and_then(|s| {
                    s.pause();
                    s.set_volume(volume);
                    if (rate - 1.0).abs() < 1e-3 {
                        let decoder = rodio::Decoder::new(Cursor::new(bytes.to_vec())).map_err(|e| e.to_string())?;
                        s.append(decoder);
                    } else {
                        let (pcm, sample_rate) = crate::pcm_of(&bytes);
                        let samples: Vec<i16> = pcm.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
                        let stretched = crate::time_stretch(&samples, rate, sample_rate);
                        s.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, stretched));
                    }
                    if at > 0.0 {
                        let _ = s.try_seek(Duration::from_secs_f64(at * stretch));
                    }
                    if play {
                        s.play();
                    }
                    Ok(s)
                });
                let mut st = state.lock().unwrap();
                match made {
                    Ok(s) => {
                        sink = Some(s);
                        *st = State { loaded: Some(key), position: at, playing: play, finished: false, error: None };
                    }
                    Err(e) => {
                        st.error = Some(format!("cannot play narration: {e}"));
                        st.loaded = None;
                    }
                }
            }
            Ok(Msg::Play) => {
                if let Some(s) = &sink {
                    s.play();
                }
            }
            Ok(Msg::Pause) => {
                if let Some(s) = &sink {
                    s.pause();
                }
            }
            Ok(Msg::Seek(at)) => {
                if let Some(s) = &sink {
                    let _ = s.try_seek(Duration::from_secs_f64(at.max(0.0) * stretch));
                }
            }
            Ok(Msg::Volume(v)) => {
                volume = v;
                if let Some(s) = &sink {
                    s.set_volume(v);
                }
            }
            Ok(Msg::Stop) => {
                if let Some(s) = sink.take() {
                    s.stop();
                }
                let mut st = state.lock().unwrap();
                st.loaded = None;
                st.playing = false;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if let Some(s) = &sink {
            let mut st = state.lock().unwrap();
            st.position = s.get_pos().as_secs_f64() / stretch;
            st.playing = !s.is_paused() && !s.empty();
            st.finished = s.empty();
        }
    }
}
