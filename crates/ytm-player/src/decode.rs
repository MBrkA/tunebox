//! Container probing and packet decoding.
//!
//! YouTube's AAC audio is fragmented MP4; it is read with our own [`Fmp4`]
//! demuxer (fast start, cheap seeks) and decoded by symphonia's AAC decoder.
//! Anything else (e.g. a plain `.m4a` from the yt-dlp fallback) goes through
//! symphonia's general probe.

use std::io::SeekFrom;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CodecParameters, Decoder as SymDecoder, DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_NULL,
};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use crate::fmp4::Fmp4;
use crate::PlayerError;

enum Demux {
    Fragmented(Box<Fmp4>),
    Generic {
        format: Box<dyn FormatReader>,
        track_id: u32,
    },
}

pub struct Decoder {
    demux: Demux,
    decoder: Box<dyn SymDecoder>,
    pub rate: u32,
    pub channels: usize,
    pub duration: Option<Duration>,
    buf: Option<SampleBuffer<f32>>,
    /// Frames to discard after a seek landed before the target.
    skip_frames: u64,
}

fn dec_err(what: &str, e: impl std::fmt::Display) -> PlayerError {
    PlayerError::Decode(format!("{what}: {e}"))
}

impl Decoder {
    /// Opens `source` and positions it at `start`. `source` must be cheaply
    /// cloneable (a fresh cursor over shared data); it is cloned for the
    /// fallback probe.
    pub fn open<S>(source: S, start: Duration) -> Result<Self, PlayerError>
    where
        S: MediaSource + Clone + 'static,
    {
        let mut fallback = source.clone();
        match Fmp4::open(Box::new(source)) {
            Ok(demux) => Self::from_fragmented(demux, start),
            Err(e) => {
                // Real I/O failures must surface; only "not fMP4" falls through.
                if e.kind() != std::io::ErrorKind::InvalidData {
                    return Err(PlayerError::Io(e.to_string()));
                }
                tracing::debug!(error = %e, "not fragmented MP4, using generic probe");
                fallback
                    .seek(SeekFrom::Start(0))
                    .map_err(|e| PlayerError::Io(e.to_string()))?;
                Self::generic(Box::new(fallback), start)
            }
        }
    }

    fn from_fragmented(mut demux: Fmp4, start: Duration) -> Result<Self, PlayerError> {
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_AAC)
            .with_sample_rate(demux.sample_rate)
            .with_extra_data(demux.config.clone().into_boxed_slice());
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| dec_err("unsupported codec", e))?;
        let rate = demux.sample_rate;
        let mut skip_frames = 0;
        if !start.is_zero() {
            let target = (start.as_secs_f64() * f64::from(demux.timescale)) as u64;
            let landed = demux.seek(target);
            skip_frames =
                (target.saturating_sub(landed)) * u64::from(rate) / u64::from(demux.timescale);
        }
        let duration = Some(Duration::from_secs_f64(
            demux.duration_ticks as f64 / f64::from(demux.timescale),
        ));
        Ok(Self {
            channels: usize::from(demux.channels).max(1),
            demux: Demux::Fragmented(Box::new(demux)),
            decoder,
            rate,
            duration,
            buf: None,
            skip_frames,
        })
    }

    fn generic(source: Box<dyn MediaSource>, start: Duration) -> Result<Self, PlayerError> {
        let mss = MediaSourceStream::new(source, Default::default());
        let mut hint = Hint::new();
        hint.with_extension("m4a");
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions {
                    enable_gapless: true,
                    ..Default::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(|e| dec_err("unrecognised container", e))?;
        let mut format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| PlayerError::Decode("no audio track".into()))?;
        let track_id = track.id;
        let params = track.codec_params.clone();
        let rate = params
            .sample_rate
            .ok_or_else(|| PlayerError::Decode("unknown sample rate".into()))?;
        let channels = params.channels.map(|c| c.count()).unwrap_or(2);
        let duration = params
            .n_frames
            .map(|n| Duration::from_secs_f64(n as f64 / f64::from(rate)));
        let decoder = symphonia::default::get_codecs()
            .make(&params, &DecoderOptions::default())
            .map_err(|e| dec_err("unsupported codec", e))?;
        let mut skip_frames = 0;
        if !start.is_zero() {
            let seeked = format
                .seek(
                    SeekMode::Accurate,
                    SeekTo::Time {
                        time: Time::from(start.as_secs_f64()),
                        track_id: Some(track_id),
                    },
                )
                .map_err(|e| dec_err("seek failed", e))?;
            skip_frames = seeked.required_ts.saturating_sub(seeked.actual_ts);
        }
        Ok(Self {
            demux: Demux::Generic { format, track_id },
            decoder,
            rate,
            channels,
            duration,
            buf: None,
            skip_frames,
        })
    }

    fn next_packet(&mut self) -> Result<Option<Packet>, PlayerError> {
        match &mut self.demux {
            Demux::Fragmented(d) => Ok(d
                .next_packet()
                .map_err(|e| PlayerError::Io(e.to_string()))?
                .map(|p| Packet::new_from_boxed_slice(0, p.ts, p.dur, p.data.into_boxed_slice()))),
            Demux::Generic { format, track_id } => loop {
                match format.next_packet() {
                    Ok(p) if p.track_id() == *track_id => return Ok(Some(p)),
                    Ok(_) => continue,
                    Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                        return Ok(None)
                    }
                    Err(SymError::IoError(e)) => return Err(PlayerError::Io(e.to_string())),
                    Err(SymError::ResetRequired) => return Ok(None),
                    Err(e) => return Err(dec_err("demux", e)),
                }
            },
        }
    }

    /// Decodes the next packet, appending interleaved f32 samples to `out`.
    /// Returns `Ok(false)` at end of stream.
    pub fn next_samples(&mut self, out: &mut Vec<f32>) -> Result<bool, PlayerError> {
        loop {
            let Some(packet) = self.next_packet()? else {
                return Ok(false);
            };
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let spec = *decoded.spec();
                    let needed = decoded.capacity() * spec.channels.count();
                    if self.buf.as_ref().is_none_or(|b| b.capacity() < needed) {
                        self.buf = Some(SampleBuffer::new(decoded.capacity() as u64, spec));
                    }
                    let buf = self.buf.as_mut().expect("just created");
                    buf.copy_interleaved_ref(decoded);
                    // The decoder knows the true channel count.
                    self.channels = spec.channels.count();
                    let mut samples = buf.samples();
                    if self.skip_frames > 0 {
                        let frames = (samples.len() / self.channels) as u64;
                        let drop = self.skip_frames.min(frames);
                        self.skip_frames -= drop;
                        samples = &samples[drop as usize * self.channels..];
                    }
                    if samples.is_empty() {
                        continue;
                    }
                    out.extend_from_slice(samples);
                    return Ok(true);
                }
                // A corrupt packet is not fatal: skip it.
                Err(SymError::DecodeError(e)) => {
                    tracing::debug!(error = %e, "skipping undecodable packet");
                }
                Err(SymError::IoError(e)) => return Err(PlayerError::Io(e.to_string())),
                Err(e) => return Err(dec_err("decode", e)),
            }
        }
    }
}
