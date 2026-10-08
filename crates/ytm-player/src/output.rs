//! Shared sample buffer between the decode thread (producer) and the audio
//! callback (consumer), plus the cpal / null sinks that drain it.
//!
//! The buffer also does the bookkeeping the engine needs: it counts consumed
//! frames, remembers where each track started (`Mark`s), and so can report the
//! playback position and tell when playback crossed into the next track
//! without any extra synchronisation with the audio thread.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

struct Mark {
    at_frame: u64,
    job: u64,
    offset_ms: u64,
}

struct Inner {
    samples: VecDeque<f32>,
    generation: u64,
    written: u64,
    consumed: u64,
    marks: VecDeque<Mark>,
    gain: f32,
}

pub struct AudioBuf {
    inner: Mutex<Inner>,
    space: Condvar,
    capacity: usize,
    pub channels: usize,
    pub rate: u32,
    paused: AtomicBool,
    volume_bits: AtomicU32,
    // Mirrors for lock-free reads from the UI / controller.
    consumed: AtomicU64,
    current_job: AtomicU64,
    job_base: AtomicU64,
    job_offset_ms: AtomicU64,
    underruns: AtomicU64,
}

impl AudioBuf {
    pub fn new(rate: u32, channels: usize, seconds: f32) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                samples: VecDeque::new(),
                generation: 0,
                written: 0,
                consumed: 0,
                marks: VecDeque::new(),
                gain: 0.0,
            }),
            space: Condvar::new(),
            capacity: (rate as f32 * seconds) as usize * channels,
            channels,
            rate,
            paused: AtomicBool::new(false),
            volume_bits: AtomicU32::new(1.0f32.to_bits()),
            consumed: AtomicU64::new(0),
            current_job: AtomicU64::new(0),
            job_base: AtomicU64::new(0),
            job_offset_ms: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        })
    }

    // ---- control side -------------------------------------------------

    pub fn set_paused(&self, p: bool) {
        self.paused.store(p, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Linear 0..=1 slider value; a squared curve is applied so the slider
    /// feels roughly perceptual.
    pub fn set_volume(&self, v: f32) {
        self.volume_bits
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Drops all buffered audio and invalidates producers holding the old
    /// generation. Returns the new generation.
    pub fn flush(&self) -> u64 {
        let mut st = self.inner.lock().unwrap();
        st.samples.clear();
        st.marks.clear();
        st.generation += 1;
        st.written = st.consumed;
        let g = st.generation;
        drop(st);
        self.space.notify_all();
        g
    }

    pub fn generation(&self) -> u64 {
        self.inner.lock().unwrap().generation
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().samples.is_empty()
    }

    pub fn buffered(&self) -> Duration {
        let st = self.inner.lock().unwrap();
        Duration::from_secs_f64(st.samples.len() as f64 / self.channels as f64 / self.rate as f64)
    }

    /// Job currently being heard, and the playback position within it.
    pub fn now_playing(&self) -> (u64, Duration) {
        let job = self.current_job.load(Ordering::Relaxed);
        let consumed = self.consumed.load(Ordering::Relaxed);
        let base = self.job_base.load(Ordering::Relaxed);
        let frames = consumed.saturating_sub(base);
        let pos = Duration::from_secs_f64(frames as f64 / self.rate as f64)
            + Duration::from_millis(self.job_offset_ms.load(Ordering::Relaxed));
        (job, pos)
    }

    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }

    // ---- producer side ------------------------------------------------

    /// Registers the start of a track at the current write position.
    /// Returns false if `generation` is stale.
    pub fn begin_job(&self, generation: u64, job: u64, offset_ms: u64) -> bool {
        let mut st = self.inner.lock().unwrap();
        if st.generation != generation {
            return false;
        }
        let at_frame = st.written;
        st.marks.push_back(Mark {
            at_frame,
            job,
            offset_ms,
        });
        true
    }

    /// Blocks until all samples are queued. Returns false if the generation
    /// changed (flush) before everything was written.
    pub fn push(&self, generation: u64, mut samples: &[f32]) -> bool {
        let mut st = self.inner.lock().unwrap();
        while !samples.is_empty() {
            if st.generation != generation {
                return false;
            }
            let free = self.capacity.saturating_sub(st.samples.len());
            if free == 0 {
                st = self
                    .space
                    .wait_timeout(st, Duration::from_millis(50))
                    .unwrap()
                    .0;
                continue;
            }
            let n = free.min(samples.len());
            let n = n - n % self.channels.max(1);
            let n = if n == 0 {
                samples.len().min(self.channels)
            } else {
                n
            };
            st.samples.extend(&samples[..n]);
            st.written += (n / self.channels) as u64;
            samples = &samples[n..];
        }
        st.generation == generation
    }

    // ---- consumer side (audio callback) ----------------------------------

    /// Fills `out` with interleaved f32 samples (silence when paused/underrun).
    pub fn fill(&self, out: &mut [f32]) {
        let Ok(mut st) = self.inner.try_lock() else {
            out.fill(0.0);
            return;
        };
        let target = {
            let v = self.volume();
            v * v
        };
        if self.paused() {
            out.fill(0.0);
            st.gain = target;
            return;
        }
        let avail = st.samples.len().min(out.len());
        // Activate track boundaries we have reached.
        while let Some(m) = st.marks.front() {
            if m.at_frame <= st.consumed {
                self.current_job.store(m.job, Ordering::Relaxed);
                self.job_base.store(m.at_frame, Ordering::Relaxed);
                self.job_offset_ms.store(m.offset_ms, Ordering::Relaxed);
                st.marks.pop_front();
            } else {
                break;
            }
        }
        let start_gain = st.gain;
        let steps = (out.len() / self.channels).max(1) as f32;
        for (i, slot) in out[..avail].iter_mut().enumerate() {
            let frame = (i / self.channels) as f32;
            let g = start_gain + (target - start_gain) * (frame / steps);
            *slot = st.samples.pop_front().unwrap_or(0.0) * g;
        }
        if avail < out.len() {
            out[avail..].fill(0.0);
            if !st.marks.is_empty() || avail > 0 {
                self.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        st.gain = target;
        st.consumed += (avail / self.channels) as u64;
        self.consumed.store(st.consumed, Ordering::Relaxed);
        drop(st);
        self.space.notify_one();
    }
}

// ---- sinks ------------------------------------------------------------------

/// Keeps the output stream alive; dropping it stops audio.
pub struct Sink {
    stop: Arc<AtomicBool>,
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputKind {
    /// The default system output device.
    #[default]
    Device,
    /// Consumes at real-time speed without producing sound (tests, headless CI).
    Null,
}

pub struct DeviceFormat {
    pub rate: u32,
    pub channels: usize,
}

/// Opens the output and returns its format plus a sink guard. `make_buf`
/// receives the device format and must build the shared buffer the callback drains.
pub fn open(
    kind: OutputKind,
    make_buf: impl FnOnce(&DeviceFormat) -> Arc<AudioBuf> + Send + 'static,
) -> Result<(Arc<AudioBuf>, Sink), String> {
    match kind {
        OutputKind::Null => {
            let buf = make_buf(&DeviceFormat {
                rate: 48_000,
                channels: 2,
            });
            let stop = Arc::new(AtomicBool::new(false));
            let (b, s) = (buf.clone(), stop.clone());
            std::thread::Builder::new()
                .name("ytm-null-sink".into())
                .spawn(move || {
                    // Drain by wall-clock time, not per wake-up: sleeps overshoot a lot on
                    // loaded machines (macOS timer coalescing), which would slow playback down.
                    let start = std::time::Instant::now();
                    let mut drained = 0u64; // frames
                    let mut tmp = Vec::new();
                    while !s.load(Ordering::Relaxed) {
                        let due = (start.elapsed().as_secs_f64() * b.rate as f64) as u64;
                        let frames = due.saturating_sub(drained) as usize;
                        if frames > 0 {
                            tmp.resize(frames * b.channels, 0.0);
                            b.fill(&mut tmp);
                            drained += frames as u64;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok((buf, Sink { stop }))
        }
        OutputKind::Device => open_device(make_buf),
    }
}

fn open_device(
    make_buf: impl FnOnce(&DeviceFormat) -> Arc<AudioBuf> + Send + 'static,
) -> Result<(Arc<AudioBuf>, Sink), String> {
    // cpal streams are not Send on every platform, so the stream lives on its own thread.
    let (tx, rx) = std::sync::mpsc::channel::<Result<Arc<AudioBuf>, String>>();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    std::thread::Builder::new()
        .name("ytm-audio".into())
        .spawn(move || {
            let built = (|| -> Result<(Arc<AudioBuf>, cpal::Stream), String> {
                let host = cpal::default_host();
                let device = host
                    .default_output_device()
                    .ok_or("no audio output device")?;
                let supported = device
                    .default_output_config()
                    .map_err(|e| format!("no output config: {e}"))?;
                let format = DeviceFormat {
                    rate: supported.sample_rate(),
                    channels: supported.channels() as usize,
                };
                let buf = make_buf(&format);
                let config = supported.config();
                let err_fn = |e: cpal::StreamError| match e {
                    // ALSA xruns happen when the process is starved; playback continues.
                    cpal::StreamError::BufferUnderrun => tracing::warn!("audio buffer xrun"),
                    e => tracing::error!(error = %e, "audio stream error"),
                };
                let b = buf.clone();
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &config,
                        move |data: &mut [f32], _| b.fill(data),
                        err_fn,
                        None,
                    ),
                    cpal::SampleFormat::I16 => {
                        let mut tmp = Vec::new();
                        device.build_output_stream(
                            &config,
                            move |data: &mut [i16], _| {
                                tmp.resize(data.len(), 0.0);
                                b.fill(&mut tmp);
                                for (o, s) in data.iter_mut().zip(&tmp) {
                                    *o = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    cpal::SampleFormat::U16 => {
                        let mut tmp = Vec::new();
                        device.build_output_stream(
                            &config,
                            move |data: &mut [u16], _| {
                                tmp.resize(data.len(), 0.0);
                                b.fill(&mut tmp);
                                for (o, s) in data.iter_mut().zip(&tmp) {
                                    *o =
                                        ((s.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                                }
                            },
                            err_fn,
                            None,
                        )
                    }
                    other => return Err(format!("unsupported sample format {other:?}")),
                }
                .map_err(|e| format!("cannot open output: {e}"))?;
                stream
                    .play()
                    .map_err(|e| format!("cannot start output: {e}"))?;
                Ok((buf, stream))
            })();
            match built {
                Ok((buf, stream)) => {
                    let _ = tx.send(Ok(buf));
                    while !thread_stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    drop(stream);
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        })
        .map_err(|e| e.to_string())?;
    let buf = rx.recv().map_err(|_| "audio thread died".to_string())??;
    Ok((buf, Sink { stop }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> Arc<AudioBuf> {
        AudioBuf::new(1000, 2, 1.0)
    }

    #[test]
    fn fill_applies_volume_curve_and_counts_frames() {
        let b = buf();
        b.set_volume(0.5);
        let g = b.generation();
        assert!(b.push(g, &vec![1.0; 200]));
        let mut out = vec![0.0; 200];
        b.fill(&mut out); // first call ramps from 0 to 0.25
        b.fill(&mut out); // empty: silence
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(b.is_empty());
        assert_eq!(b.now_playing().0, 0, "no job started yet");
    }

    #[test]
    fn steady_state_gain() {
        let b = buf();
        b.set_volume(0.5);
        let g = b.generation();
        b.push(g, &vec![1.0; 400]);
        let mut out = vec![0.0; 100];
        b.fill(&mut out);
        b.fill(&mut out);
        assert!((out[99] - 0.25).abs() < 1e-6, "{}", out[99]);
    }

    #[test]
    fn marks_switch_jobs_and_track_position() {
        let b = buf();
        let g = b.generation();
        assert!(b.begin_job(g, 1, 0));
        b.push(g, &vec![0.1; 2 * 500]); // 500 frames = 0.5 s
        assert!(b.begin_job(g, 2, 10_000));
        b.push(g, &vec![0.1; 2 * 500]);
        let mut out = vec![0.0; 2 * 250];
        b.fill(&mut out);
        assert_eq!(b.now_playing().0, 1);
        b.fill(&mut out);
        let (job, pos) = b.now_playing();
        assert_eq!(job, 1);
        assert_eq!(
            pos,
            Duration::from_millis(500),
            "position = end of consumed audio"
        );
        b.fill(&mut out); // consumed 500 ≥ job 2's start: activates it
        let (job, pos) = b.now_playing();
        assert_eq!(job, 2);
        assert_eq!(pos, Duration::from_millis(10_000 + 250));
    }

    #[test]
    fn pause_holds_audio() {
        let b = buf();
        let g = b.generation();
        b.push(g, &vec![1.0; 100]);
        b.set_paused(true);
        let mut out = vec![9.0; 100];
        b.fill(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(!b.is_empty());
        b.set_paused(false);
        b.fill(&mut out);
        assert!(b.is_empty());
    }

    #[test]
    fn flush_invalidates_producers_and_unblocks_them() {
        let b = buf();
        let g = b.generation();
        let b2 = b.clone();
        // 3 s of audio into a 1 s buffer: blocks until flushed
        let h = std::thread::spawn(move || b2.push(g, &vec![0.0; 2 * 3000]));
        std::thread::sleep(Duration::from_millis(100));
        let new_gen = b.flush();
        assert!(!h.join().unwrap(), "stale producer must be told to stop");
        assert!(b.is_empty());
        assert!(!b.begin_job(g, 1, 0));
        assert!(b.begin_job(new_gen, 1, 0));
    }

    #[test]
    fn null_sink_drains_in_real_time() {
        let (b, _sink) =
            open(OutputKind::Null, |f| AudioBuf::new(f.rate, f.channels, 2.0)).unwrap();
        let g = b.generation();
        b.begin_job(g, 7, 0);
        b.push(g, &vec![0.0; 48_000 / 2 * 2]); // 0.5 s
        std::thread::sleep(Duration::from_millis(300));
        let (job, pos) = b.now_playing();
        assert_eq!(job, 7);
        assert!(
            pos > Duration::from_millis(150) && pos < Duration::from_millis(450),
            "{pos:?}"
        );
    }
}
