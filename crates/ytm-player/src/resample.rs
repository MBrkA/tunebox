//! Streaming sample-rate conversion (interleaved f32) on top of rubato's FFT resampler.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

const CHUNK_FRAMES: usize = 1024;

pub struct StreamResampler {
    inner: Option<Fft<f32>>,
    channels: usize,
    pending: Vec<f32>,
    out_chunk: Vec<f32>,
    /// Output frames still to drop to cancel the resampler's start-up delay.
    skip_frames: usize,
}

impl StreamResampler {
    /// Passthrough when the rates are equal.
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let inner = if from == to {
            None
        } else {
            Some(
                Fft::<f32>::new(
                    from as usize,
                    to as usize,
                    CHUNK_FRAMES,
                    2,
                    channels,
                    FixedSync::Input,
                )
                .map_err(|e| e.to_string())?,
            )
        };
        let out_chunk = inner
            .as_ref()
            .map(|r| vec![0.0; r.output_frames_max() * channels])
            .unwrap_or_default();
        let skip_frames = inner.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        Ok(Self {
            inner,
            channels,
            pending: Vec::new(),
            out_chunk,
            skip_frames,
        })
    }

    /// Appends converted samples for `input` (interleaved) to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) -> Result<(), String> {
        let Some(rs) = self.inner.as_mut() else {
            out.extend_from_slice(input);
            return Ok(());
        };
        self.pending.extend_from_slice(input);
        loop {
            let need = rs.input_frames_next();
            if self.pending.len() < need * self.channels {
                return Ok(());
            }
            let in_ad = InterleavedSlice::new(&self.pending, self.channels, need)
                .map_err(|e| e.to_string())?;
            let cap = self.out_chunk.len() / self.channels;
            let mut out_ad = InterleavedSlice::new_mut(&mut self.out_chunk, self.channels, cap)
                .map_err(|e| e.to_string())?;
            let (_, produced) = rs
                .process_into_buffer(&in_ad, &mut out_ad, None)
                .map_err(|e| e.to_string())?;
            emit(
                &self.out_chunk,
                produced,
                self.channels,
                &mut self.skip_frames,
                out,
            );
            self.pending.drain(..need * self.channels);
        }
    }

    /// Flushes buffered input at end of stream.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        let Some(rs) = self.inner.as_mut() else {
            return Ok(());
        };
        let have = self.pending.len() / self.channels;
        if have == 0 {
            return Ok(());
        }
        let need = rs.input_frames_next();
        self.pending.resize(need * self.channels, 0.0);
        let in_ad =
            InterleavedSlice::new(&self.pending, self.channels, need).map_err(|e| e.to_string())?;
        let cap = self.out_chunk.len() / self.channels;
        let mut out_ad = InterleavedSlice::new_mut(&mut self.out_chunk, self.channels, cap)
            .map_err(|e| e.to_string())?;
        let indexing = Indexing {
            input_offset: 0,
            output_offset: 0,
            partial_len: Some(have),
            active_channels_mask: None,
        };
        let (_, produced) = rs
            .process_into_buffer(&in_ad, &mut out_ad, Some(&indexing))
            .map_err(|e| e.to_string())?;
        emit(
            &self.out_chunk,
            produced,
            self.channels,
            &mut self.skip_frames,
            out,
        );
        self.pending.clear();
        Ok(())
    }
}

fn emit(chunk: &[f32], produced: usize, ch: usize, skip: &mut usize, out: &mut Vec<f32>) {
    let drop = (*skip).min(produced);
    *skip -= drop;
    out.extend_from_slice(&chunk[drop * ch..produced * ch]);
}

/// Maps interleaved `from_ch` audio to `to_ch` channels (stereo→mono averages,
/// mono→N duplicates, extra output channels are silent).
pub fn remap_channels(input: &[f32], from_ch: usize, to_ch: usize, out: &mut Vec<f32>) {
    if from_ch == to_ch {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(from_ch) {
        for c in 0..to_ch {
            out.push(match (from_ch, to_ch) {
                (_, 1) => frame.iter().sum::<f32>() / from_ch as f32,
                (1, _) => frame[0],
                _ if c < from_ch => frame[c],
                _ => 0.0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, frames: usize, ch: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let s = (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5;
                std::iter::repeat_n(s, ch)
            })
            .collect()
    }

    #[test]
    fn passthrough_when_rates_match() {
        let mut r = StreamResampler::new(48_000, 48_000, 2).unwrap();
        let input = sine(440.0, 48_000, 500, 2);
        let mut out = vec![];
        r.process(&input, &mut out).unwrap();
        r.finish(&mut out).unwrap();
        assert_eq!(out, input);
    }

    #[test]
    fn converts_length_and_frequency() {
        let (from, to) = (44_100, 48_000);
        let frames = 44_100;
        let mut r = StreamResampler::new(from, to, 2).unwrap();
        let mut out = vec![];
        // feed in awkward packet sizes, as a decoder would
        for packet in sine(1000.0, from, frames, 2).chunks(2 * 1111) {
            r.process(packet, &mut out).unwrap();
        }
        r.finish(&mut out).unwrap();
        let out_frames = out.len() / 2;
        let expected = 48_000;
        assert!(
            out_frames.abs_diff(expected) < 2048,
            "got {out_frames} frames, expected ≈{expected}"
        );
        // Judge the steady interior only: the tail is zero-padded flush output.
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let mid = &left[4000..44_000];
        // 1 kHz tone over 40 000 frames at 48 kHz => ~833 upward zero crossings
        let crossings = mid.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i32 - 833).abs() <= 3, "crossings {crossings}");
        let peak = mid.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((0.45..0.55).contains(&peak), "peak {peak}");
    }

    #[test]
    fn channel_remap() {
        let mut o = vec![];
        remap_channels(&[1.0, 3.0, 2.0, 4.0], 2, 1, &mut o);
        assert_eq!(o, vec![2.0, 3.0]);
        o.clear();
        remap_channels(&[1.0, 2.0], 1, 2, &mut o);
        assert_eq!(o, vec![1.0, 1.0, 2.0, 2.0]);
        o.clear();
        remap_channels(&[1.0, 2.0], 2, 4, &mut o);
        assert_eq!(o, vec![1.0, 2.0, 0.0, 0.0]);
    }
}
