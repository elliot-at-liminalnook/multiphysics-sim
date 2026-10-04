//! Video files from drawn frames: RGBA images encoded as H.264 (Cisco's
//! OpenH264, built from source by the `openh264` crate) in an MP4 container
//! (the `mp4` crate), playable by every common player. The viewer's
//! "Record video" (the browser's canvas MediaRecorder export) feeds it the
//! window's frames; a capture of drawn frames, not a physics benchmark and no
//! substitute for a saved input recording.
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

/// The movie timescale (ticks per second): frame times are rounded to it.
const TIMESCALE: u32 = 90_000;

/// One MP4 being written: frames are appended with their presentation time.
pub struct Mp4Encoder {
    encoder: Encoder,
    writer: Option<mp4::Mp4Writer<BufWriter<std::fs::File>>>,
    file: Option<std::fs::File>,
    width: usize,
    height: usize,
    /// The previous frame's sample (written once the next frame's time gives its duration).
    pending: Option<(u64, bool, Vec<u8>)>,
    /// The frame interval used for the last sample's duration.
    nominal: u32,
    pub frames: u64,
}

/// The NAL units of an Annex B stream, without their start codes (or the
/// zero byte a 4-byte start code leaves at the end of the previous unit).
fn nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 2 < stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let end = starts.get(k + 1).map_or(stream.len(), |&n| n - 3);
            let mut unit = &stream[s..end];
            while unit.last() == Some(&0) {
                unit = &unit[..unit.len() - 1];
            }
            unit
        })
        .filter(|u| !u.is_empty())
        .collect()
}

impl Mp4Encoder {
    /// Starts `path` (created, never overwritten) for `width` × `height`
    /// frames at a nominal `fps` (frames carry their own times). Odd sizes are
    /// cropped by one pixel (H.264 4:2:0 needs even dimensions).
    pub fn create(path: &Path, width: usize, height: usize, fps: f32) -> Result<Self, String> {
        let (width, height) = (width & !1, height & !1);
        if width < 16 || height < 16 {
            return Err(format!("a video needs at least 16 × 16 pixels, not {width} × {height}"));
        }
        let config = EncoderConfig::new()
            .max_frame_rate(FrameRate::from_hz(fps))
            .bitrate(BitRate::from_bps(((width * height) as f32 * fps * 0.15) as u32))
            .intra_frame_period(IntraFramePeriod::from_num_frames((fps.round() as u32).max(1) * 2));
        let encoder = Encoder::with_api_config(openh264::OpenH264API::from_source(), config).map_err(|e| format!("the H.264 encoder could not start: {e}"))?;
        let file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("{}: {e} (never overwritten)", path.display()))?;
        Ok(Self { encoder, writer: None, file: Some(file), width, height, pending: None, nominal: (TIMESCALE as f32 / fps.max(1.0)) as u32, frames: 0 })
    }
    /// The encoded size (even).
    pub fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    /// Appends one RGBA frame (`stride` bytes per row, at least `width` × 4)
    /// shown at `time_s` seconds from the start.
    pub fn push(&mut self, rgba: &[u8], stride: usize, time_s: f64) -> Result<(), String> {
        let row = self.width * 4;
        let mut packed = Vec::with_capacity(row * self.height);
        for y in 0..self.height {
            let start = y * stride;
            packed.extend_from_slice(rgba.get(start..start + row).ok_or("a video frame is smaller than the video")?);
        }
        let yuv = YUVBuffer::from_rgb_source(RgbaSliceU8::new(&packed, (self.width, self.height)));
        let stream = self.encoder.encode(&yuv).map_err(|e| format!("encoding a video frame: {e}"))?.to_vec();
        let units = nal_units(&stream);
        if self.writer.is_none() {
            let sps = units.iter().find(|u| u[0] & 0x1f == 7).ok_or("the first encoded frame carries no sequence parameter set")?.to_vec();
            let pps = units.iter().find(|u| u[0] & 0x1f == 8).ok_or("the first encoded frame carries no picture parameter set")?.to_vec();
            let file = self.file.take().ok_or("the video file is gone")?;
            let config = mp4::Mp4Config { major_brand: "isom".parse().map_err(|e| format!("{e}"))?, minor_version: 512, compatible_brands: ["isom", "iso2", "avc1", "mp41"].iter().map(|b| b.parse().expect("a four-letter brand")).collect(), timescale: TIMESCALE };
            let mut writer = mp4::Mp4Writer::write_start(BufWriter::new(file), &config).map_err(|e| format!("starting the MP4: {e}"))?;
            let track = mp4::TrackConfig { track_type: mp4::TrackType::Video, timescale: TIMESCALE, language: "und".into(), media_conf: mp4::MediaConfig::AvcConfig(mp4::AvcConfig { width: self.width as u16, height: self.height as u16, seq_param_set: sps, pic_param_set: pps }) };
            writer.add_track(&track).map_err(|e| format!("adding the video track: {e}"))?;
            self.writer = Some(writer);
        }
        // AVCC: each picture NAL with its 4-byte length (parameter sets live in the track config).
        let mut sample = Vec::with_capacity(stream.len());
        let mut sync = false;
        for u in units.iter().filter(|u| !matches!(u[0] & 0x1f, 7 | 8)) {
            sync |= u[0] & 0x1f == 5;
            sample.extend_from_slice(&(u.len() as u32).to_be_bytes());
            sample.extend_from_slice(u);
        }
        if sample.is_empty() {
            // A skipped frame: nothing to show; the previous frame lasts longer.
            return Ok(());
        }
        let at = (time_s.max(0.0) * f64::from(TIMESCALE)).round() as u64;
        self.flush_pending(Some(at))?;
        self.pending = Some((at, sync, sample));
        self.frames += 1;
        Ok(())
    }
    fn flush_pending(&mut self, next: Option<u64>) -> Result<(), String> {
        let Some((start, sync, bytes)) = self.pending.take() else { return Ok(()) };
        let duration = next.map_or(self.nominal, |n| n.saturating_sub(start).clamp(1, u64::from(u32::MAX)) as u32);
        let writer = self.writer.as_mut().ok_or("the MP4 has not started")?;
        writer.write_sample(1, &mp4::Mp4Sample { start_time: start, duration, rendering_offset: 0, is_sync: sync, bytes: bytes::Bytes::from(bytes) }).map_err(|e| format!("writing a video sample: {e}"))
    }
    /// Writes the last frame and the movie index; returns the frame count.
    pub fn finish(mut self) -> Result<u64, String> {
        if self.writer.is_none() {
            return Err("no frame was recorded".into());
        }
        self.flush_pending(None)?;
        let writer = self.writer.as_mut().expect("checked");
        writer.write_end().map_err(|e| format!("finishing the MP4: {e}"))?;
        let mut inner = self.writer.take().expect("checked").into_writer();
        inner.flush().map_err(|e| format!("writing the MP4: {e}"))?;
        let _ = inner.stream_position();
        Ok(self.frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_encode_to_a_playable_mp4() {
        let dir = std::env::temp_dir().join(format!("sim-render-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.mp4");
        let _ = std::fs::remove_file(&path);
        let (w, h) = (64, 48);
        let mut e = Mp4Encoder::create(&path, w, h, 30.0).unwrap();
        for k in 0..20u32 {
            let mut frame = vec![0u8; w * h * 4];
            for (i, px) in frame.chunks_mut(4).enumerate() {
                let x = (i % w) as u32;
                px.copy_from_slice(&[((x + k * 3) % 256) as u8, 80, 160, 255]);
            }
            e.push(&frame, w * 4, f64::from(k) / 30.0).unwrap();
        }
        assert_eq!(e.finish().unwrap(), 20);
        // The file reads back as one H.264 track of 20 samples.
        let f = std::fs::File::open(&path).unwrap();
        let size = f.metadata().unwrap().len();
        let reader = mp4::Mp4Reader::read_header(std::io::BufReader::new(f), size).unwrap();
        let track = reader.tracks().values().next().unwrap();
        assert_eq!(track.sample_count(), 20);
        assert_eq!((track.width(), track.height()), (64, 48));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
