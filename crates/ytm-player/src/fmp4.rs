//! Minimal demuxer for fragmented MP4 (DASH) audio, as served by YouTube.
//!
//! symphonia's MP4 reader walks every fragment while probing, so playback
//! could not start before the whole file had been downloaded. YouTube's
//! audio files always carry a `sidx` index, which lets us read only
//! `ftyp`/`moov`/`sidx` up front and then fetch one ~5 s segment at a time.
//! Seeking is a lookup in the segment index.
//!
//! Only what AAC-in-fMP4 needs is implemented: one audio track, `mp4a`+`esds`,
//! `tfhd`/`tfdt`/`trun` with the usual optional fields.

use std::collections::VecDeque;
use std::io::{self, SeekFrom};

use symphonia::core::io::MediaSource;

const MAX_HEADER_BOX: u64 = 8 << 20;

#[derive(Debug, Clone)]
struct Segment {
    /// Absolute file offset of the segment's `moof`.
    offset: u64,
    size: u64,
    /// Start time in track timescale ticks.
    start: u64,
    duration: u64,
}

#[derive(Debug)]
pub struct Packet {
    pub ts: u64,
    pub dur: u64,
    pub data: Vec<u8>,
}

pub struct Fmp4 {
    reader: Box<dyn MediaSource>,
    pub timescale: u32,
    pub sample_rate: u32,
    pub channels: u16,
    /// AAC AudioSpecificConfig.
    pub config: Vec<u8>,
    pub duration_ticks: u64,
    track_id: u32,
    trex_duration: u32,
    trex_size: u32,
    segments: Vec<Segment>,
    next_segment: usize,
    pending: VecDeque<Packet>,
}

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn be32(b: &[u8], at: usize) -> io::Result<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| bad("truncated box"))
}

fn be64(b: &[u8], at: usize) -> io::Result<u64> {
    b.get(at..at + 8)
        .map(|s| u64::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| bad("truncated box"))
}

fn be16(b: &[u8], at: usize) -> io::Result<u16> {
    b.get(at..at + 2)
        .map(|s| u16::from_be_bytes(s.try_into().unwrap()))
        .ok_or_else(|| bad("truncated box"))
}

/// Child boxes of `buf` as `(fourcc, payload)`.
fn boxes(buf: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 8 <= buf.len() {
        let size = u32::from_be_bytes(buf[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = buf[at + 4..at + 8].try_into().unwrap();
        let (header, total) = match size {
            0 => (8, buf.len() - at),
            1 => match buf.get(at + 8..at + 16) {
                Some(l) => (16, u64::from_be_bytes(l.try_into().unwrap()) as usize),
                None => break,
            },
            n => (8, n),
        };
        if total < header || at + total > buf.len() {
            break;
        }
        out.push((kind, &buf[at + header..at + total]));
        at += total;
    }
    out
}

fn find<'a>(buf: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(buf)
        .into_iter()
        .find(|(k, _)| k == kind)
        .map(|(_, p)| p)
}

fn find_path<'a>(buf: &'a [u8], path: &[&[u8; 4]]) -> Option<&'a [u8]> {
    path.iter().try_fold(buf, |cur, k| find(cur, k))
}

fn read_exact_at(r: &mut dyn MediaSource, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    r.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// MPEG-4 descriptor length (1–4 bytes, 7 bits each).
fn desc_len(b: &[u8], at: &mut usize) -> Option<usize> {
    let mut len = 0usize;
    for _ in 0..4 {
        let byte = *b.get(*at)?;
        *at += 1;
        len = (len << 7) | (byte & 0x7f) as usize;
        if byte & 0x80 == 0 {
            return Some(len);
        }
    }
    None
}

/// Extracts the DecoderSpecificInfo (AudioSpecificConfig) from an `esds` payload.
fn parse_esds(esds: &[u8]) -> Option<Vec<u8>> {
    let mut at = 4; // version + flags
    if *esds.get(at)? != 0x03 {
        return None;
    }
    at += 1;
    desc_len(esds, &mut at)?;
    at += 2; // ES_ID
    let flags = *esds.get(at)?;
    at += 1;
    if flags & 0x80 != 0 {
        at += 2;
    }
    if flags & 0x40 != 0 {
        at += 1 + *esds.get(at)? as usize;
    }
    if flags & 0x20 != 0 {
        at += 2;
    }
    if *esds.get(at)? != 0x04 {
        return None;
    }
    at += 1;
    desc_len(esds, &mut at)?;
    at += 13; // objectType, streamType, bufferSize, max/avg bitrate
    if *esds.get(at)? != 0x05 {
        return None;
    }
    at += 1;
    let len = desc_len(esds, &mut at)?;
    esds.get(at..at + len).map(<[u8]>::to_vec)
}

struct TrackInfo {
    track_id: u32,
    timescale: u32,
    sample_rate: u32,
    channels: u16,
    config: Vec<u8>,
}

fn parse_trak(trak: &[u8]) -> io::Result<Option<TrackInfo>> {
    let Some(mdia) = find(trak, b"mdia") else {
        return Ok(None);
    };
    let Some(stsd) = find_path(mdia, &[b"minf", b"stbl", b"stsd"]) else {
        return Ok(None);
    };
    let Some((_, mp4a)) = boxes(stsd.get(8..).unwrap_or_default())
        .into_iter()
        .find(|(k, _)| k == b"mp4a")
    else {
        return Ok(None);
    };
    let channels = be16(mp4a, 16)?;
    let sample_rate = be32(mp4a, 24)? >> 16;
    let esds = find(mp4a.get(28..).unwrap_or_default(), b"esds").ok_or_else(|| bad("no esds"))?;
    let config = parse_esds(esds).ok_or_else(|| bad("unparsable esds"))?;

    let mdhd = find(mdia, b"mdhd").ok_or_else(|| bad("no mdhd"))?;
    let timescale = if mdhd.first() == Some(&1) {
        be32(mdhd, 20)?
    } else {
        be32(mdhd, 12)?
    };
    let tkhd = find(trak, b"tkhd").ok_or_else(|| bad("no tkhd"))?;
    let track_id = if tkhd.first() == Some(&1) {
        be32(tkhd, 20)?
    } else {
        be32(tkhd, 12)?
    };
    Ok(Some(TrackInfo {
        track_id,
        timescale,
        sample_rate,
        channels,
        config,
    }))
}

impl Fmp4 {
    pub fn open(mut reader: Box<dyn MediaSource>) -> io::Result<Self> {
        let len = reader.byte_len().ok_or_else(|| bad("unknown length"))?;
        let mut offset = 0u64;
        let mut track: Option<TrackInfo> = None;
        let mut trex = (0u32, 0u32);
        let mut sidx: Option<(u64, Vec<u8>)> = None;

        while offset + 8 <= len {
            let head = read_exact_at(&mut *reader, offset, 16.min((len - offset) as usize))?;
            let mut size = u64::from(be32(&head, 0)?);
            let kind: [u8; 4] = head[4..8].try_into().unwrap();
            let header = if size == 1 {
                size = be64(&head, 8)?;
                16
            } else {
                8
            };
            if size == 0 {
                size = len - offset;
            }
            if size < header {
                return Err(bad("corrupt box size"));
            }
            match &kind {
                b"moof" => break,
                b"moov" | b"sidx" => {
                    if size > MAX_HEADER_BOX {
                        return Err(bad("header box too large"));
                    }
                    let body =
                        read_exact_at(&mut *reader, offset + header, (size - header) as usize)?;
                    if &kind == b"moov" {
                        for (k, p) in boxes(&body) {
                            if &k == b"trak" && track.is_none() {
                                track = parse_trak(p)?;
                            } else if &k == b"mvex" {
                                if let Some(trex_box) = find(p, b"trex") {
                                    trex = (be32(trex_box, 12)?, be32(trex_box, 16)?);
                                }
                            }
                        }
                    } else {
                        sidx = Some((offset + size, body));
                    }
                }
                _ => {}
            }
            offset += size;
            if track.is_some() && sidx.is_some() {
                break;
            }
        }

        let track = track.ok_or_else(|| bad("no AAC audio track"))?;
        let (sidx_end, sidx) = sidx.ok_or_else(|| bad("no sidx index"))?;
        let segments = parse_sidx(&sidx, sidx_end, track.timescale, len)?;
        let duration_ticks = segments.last().map(|s| s.start + s.duration).unwrap_or(0);
        Ok(Self {
            reader,
            timescale: track.timescale,
            sample_rate: track.sample_rate,
            channels: track.channels,
            config: track.config,
            duration_ticks,
            track_id: track.track_id,
            trex_duration: trex.0,
            trex_size: trex.1,
            segments,
            next_segment: 0,
            pending: VecDeque::new(),
        })
    }

    /// Positions at the segment containing `ticks`; returns the start tick of
    /// the first packet that will be produced.
    pub fn seek(&mut self, ticks: u64) -> u64 {
        let idx = self
            .segments
            .iter()
            .rposition(|s| s.start <= ticks)
            .unwrap_or(0);
        self.next_segment = idx;
        self.pending.clear();
        self.segments.get(idx).map(|s| s.start).unwrap_or(0)
    }

    pub fn next_packet(&mut self) -> io::Result<Option<Packet>> {
        while self.pending.is_empty() {
            let Some(seg) = self.segments.get(self.next_segment).cloned() else {
                return Ok(None);
            };
            self.next_segment += 1;
            let buf = read_exact_at(&mut *self.reader, seg.offset, seg.size as usize)?;
            self.parse_segment(&buf, &seg)?;
        }
        Ok(self.pending.pop_front())
    }

    fn parse_segment(&mut self, buf: &[u8], seg: &Segment) -> io::Result<()> {
        let top = boxes(buf);
        let (moof_start, moof) = {
            let mut pos = 0usize;
            let mut found = None;
            for (k, p) in &top {
                // payload slice -> box start: every box here has an 8 byte header
                if k == b"moof" {
                    found = Some((pos, *p));
                    break;
                }
                pos += p.len() + 8;
            }
            found.ok_or_else(|| bad("segment without moof"))?
        };
        let moof_len = moof.len() + 8;
        let mut cursor = moof_len + 8; // first byte after the following mdat header
        for (k, traf) in boxes(moof) {
            if &k != b"traf" {
                continue;
            }
            let tfhd = find(traf, b"tfhd").ok_or_else(|| bad("no tfhd"))?;
            let tf_flags = be32(tfhd, 0)? & 0xff_ffff;
            if be32(tfhd, 4)? != self.track_id {
                continue;
            }
            let mut at = 8;
            let base = if tf_flags & 0x1 != 0 {
                let b = be64(tfhd, at)?;
                at += 8;
                b.saturating_sub(seg.offset) as i64
            } else {
                moof_start as i64
            };
            if tf_flags & 0x2 != 0 {
                at += 4;
            }
            let def_dur = if tf_flags & 0x8 != 0 {
                let v = be32(tfhd, at)?;
                at += 4;
                v
            } else {
                self.trex_duration
            };
            let def_size = if tf_flags & 0x10 != 0 {
                be32(tfhd, at)?
            } else {
                self.trex_size
            };
            let mut ts = match find(traf, b"tfdt") {
                Some(t) if t.first() == Some(&1) => be64(t, 4)?,
                Some(t) => u64::from(be32(t, 4)?),
                None => seg.start,
            };
            for (k, trun) in boxes(traf) {
                if &k != b"trun" {
                    continue;
                }
                let flags = be32(trun, 0)? & 0xff_ffff;
                let count = be32(trun, 4)? as usize;
                let mut at = 8;
                let mut data_at = if flags & 0x1 != 0 {
                    let off = be32(trun, at)? as i32;
                    at += 4;
                    (base + i64::from(off)) as usize
                } else {
                    cursor
                };
                if flags & 0x4 != 0 {
                    at += 4;
                }
                for _ in 0..count {
                    let dur = if flags & 0x100 != 0 {
                        let v = be32(trun, at)?;
                        at += 4;
                        v
                    } else {
                        def_dur
                    };
                    let size = if flags & 0x200 != 0 {
                        let v = be32(trun, at)?;
                        at += 4;
                        v
                    } else {
                        def_size
                    } as usize;
                    if flags & 0x400 != 0 {
                        at += 4;
                    }
                    if flags & 0x800 != 0 {
                        at += 4;
                    }
                    let data = buf
                        .get(data_at..data_at + size)
                        .ok_or_else(|| bad("sample outside segment"))?;
                    self.pending.push_back(Packet {
                        ts,
                        dur: u64::from(dur),
                        data: data.to_vec(),
                    });
                    data_at += size;
                    ts += u64::from(dur);
                }
                cursor = data_at;
            }
        }
        Ok(())
    }
}

fn parse_sidx(
    sidx: &[u8],
    end: u64,
    track_timescale: u32,
    file_len: u64,
) -> io::Result<Vec<Segment>> {
    let version = *sidx.first().ok_or_else(|| bad("empty sidx"))?;
    let timescale = be32(sidx, 8)?;
    let (earliest, first_offset, mut at) = if version == 0 {
        (u64::from(be32(sidx, 12)?), u64::from(be32(sidx, 16)?), 20)
    } else {
        (be64(sidx, 12)?, be64(sidx, 20)?, 28)
    };
    at += 2; // reserved
    let count = be16(sidx, at)? as usize;
    at += 2;
    if timescale == 0 {
        return Err(bad("sidx timescale 0"));
    }
    let to_track = |v: u64| v * u64::from(track_timescale) / u64::from(timescale);
    let mut offset = end + first_offset;
    let mut start = earliest;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let word = be32(sidx, at)?;
        let duration = u64::from(be32(sidx, at + 4)?);
        at += 12;
        if word >> 31 != 0 {
            return Err(bad("hierarchical sidx not supported"));
        }
        let size = u64::from(word & 0x7fff_ffff);
        if offset + size > file_len {
            return Err(bad("sidx points past end of file"));
        }
        out.push(Segment {
            offset,
            size,
            start: to_track(start),
            duration: to_track(duration),
        });
        offset += size;
        start += duration;
    }
    if out.is_empty() {
        return Err(bad("empty sidx"));
    }
    Ok(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{Cursor, Read, Seek};

    fn mk(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend(kind);
        v.extend(payload);
        v
    }

    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    struct Mem(Cursor<Vec<u8>>);
    impl Read for Mem {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.0.read(b)
        }
    }
    impl Seek for Mem {
        fn seek(&mut self, p: SeekFrom) -> io::Result<u64> {
            self.0.seek(p)
        }
    }
    impl MediaSource for Mem {
        fn is_seekable(&self) -> bool {
            true
        }
        fn byte_len(&self) -> Option<u64> {
            Some(self.0.get_ref().len() as u64)
        }
    }

    /// Builds a synthetic fMP4: `segments` fragments of `per_seg` frames each.
    /// Frame `n` (global) carries payload `[n as u8; 10 + n % 5]`.
    pub fn build(segments: usize, per_seg: usize) -> Vec<u8> {
        let asc = [0x12u8, 0x10]; // AAC-LC, 44.1 kHz, stereo
        let mut esds = vec![0, 0, 0, 0, 0x03, 25, 0, 1, 0, 0x04, 17, 0x40, 0x15, 0, 0, 0];
        esds.extend([0u8; 8]);
        esds.extend([0x05, 2]);
        esds.extend(asc);
        esds.extend([0x06, 1, 2]);
        let mut mp4a = vec![0u8; 6];
        mp4a.extend([0, 1]);
        mp4a.extend([0u8; 8]);
        mp4a.extend([0, 2, 0, 16, 0, 0, 0, 0]);
        mp4a.extend((44_100u32 << 16).to_be_bytes());
        mp4a.extend(mk(b"esds", &esds));
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend(mk(b"mp4a", &mp4a));
        let mut mdhd = vec![0u8; 12];
        mdhd.extend(44_100u32.to_be_bytes());
        mdhd.extend([0u8; 8]);
        let mut tkhd = vec![0u8; 12];
        tkhd.extend(1u32.to_be_bytes());
        tkhd.extend([0u8; 72]);
        let trak = mk(
            b"trak",
            &cat(&[
                mk(b"tkhd", &tkhd),
                mk(
                    b"mdia",
                    &cat(&[
                        mk(b"mdhd", &mdhd),
                        mk(b"minf", &mk(b"stbl", &mk(b"stsd", &stsd))),
                    ]),
                ),
            ]),
        );
        let mut trex = vec![0u8; 4]; // version + flags
        trex.extend(1u32.to_be_bytes()); // track id
        trex.extend(1u32.to_be_bytes()); // sample description index
        trex.extend(1024u32.to_be_bytes());
        trex.extend(0u32.to_be_bytes());
        trex.extend(0u32.to_be_bytes());
        let moov = mk(b"moov", &cat(&[trak, mk(b"mvex", &mk(b"trex", &trex))]));
        let ftyp = mk(b"ftyp", b"dash\0\0\0\0iso6mp41");

        // fragments
        let mut frags = Vec::new();
        let mut sizes = Vec::new();
        for s in 0..segments {
            let samples: Vec<Vec<u8>> = (0..per_seg)
                .map(|i| {
                    let n = s * per_seg + i;
                    vec![n as u8; 10 + n % 5]
                })
                .collect();
            let mut trun = Vec::new();
            trun.extend(0x0000_0201u32.to_be_bytes()); // data offset + sample size
            trun.extend((per_seg as u32).to_be_bytes());
            let data_offset_pos = trun.len();
            trun.extend(0i32.to_be_bytes());
            for smp in &samples {
                trun.extend((smp.len() as u32).to_be_bytes());
            }
            let mut tfhd = 0x0002_0000u32.to_be_bytes().to_vec(); // default-base-is-moof
            tfhd.extend(1u32.to_be_bytes()); // track id
            let mut tfdt = vec![0u8; 4];
            tfdt.extend(((s * per_seg * 1024) as u32).to_be_bytes());
            let build_moof = |trun: &[u8]| {
                mk(
                    b"moof",
                    &cat(&[
                        mk(b"mfhd", &[0u8; 8]),
                        mk(
                            b"traf",
                            &cat(&[mk(b"tfhd", &tfhd), mk(b"tfdt", &tfdt), mk(b"trun", trun)]),
                        ),
                    ]),
                )
            };
            let moof_len = build_moof(&trun).len();
            let off = (moof_len + 8) as i32;
            trun[data_offset_pos..data_offset_pos + 4].copy_from_slice(&off.to_be_bytes());
            let moof = build_moof(&trun);
            let mdat = mk(b"mdat", &samples.concat());
            let frag = cat(&[moof, mdat]);
            sizes.push(frag.len() as u32);
            frags.push(frag);
        }

        let mut sidx = vec![0u8; 4];
        sidx.extend(1u32.to_be_bytes());
        sidx.extend(44_100u32.to_be_bytes());
        sidx.extend(0u32.to_be_bytes());
        sidx.extend(0u32.to_be_bytes());
        sidx.extend(0u16.to_be_bytes());
        sidx.extend((segments as u16).to_be_bytes());
        for sz in &sizes {
            sidx.extend(sz.to_be_bytes());
            sidx.extend(((per_seg * 1024) as u32).to_be_bytes());
            sidx.extend(0u32.to_be_bytes());
        }
        cat(&[ftyp, moov, mk(b"sidx", &sidx), frags.concat()])
    }

    #[test]
    fn reads_track_info_and_index() {
        let f = Fmp4::open(Box::new(Mem(Cursor::new(build(3, 4))))).unwrap();
        assert_eq!(
            (f.timescale, f.sample_rate, f.channels),
            (44_100, 44_100, 2)
        );
        assert_eq!(f.config, vec![0x12, 0x10]);
        assert_eq!(f.duration_ticks, 3 * 4 * 1024);
        assert_eq!(f.segments.len(), 3);
    }

    #[test]
    fn yields_all_packets_in_order_with_timestamps() {
        let mut f = Fmp4::open(Box::new(Mem(Cursor::new(build(3, 4))))).unwrap();
        let mut n = 0usize;
        while let Some(p) = f.next_packet().unwrap() {
            assert_eq!(p.ts, (n * 1024) as u64);
            assert_eq!(p.dur, 1024);
            assert_eq!(p.data, vec![n as u8; 10 + n % 5], "packet {n}");
            n += 1;
        }
        assert_eq!(n, 12);
    }

    #[test]
    fn seek_lands_on_segment_start() {
        let mut f = Fmp4::open(Box::new(Mem(Cursor::new(build(3, 4))))).unwrap();
        let first = f.seek(5 * 1024 + 7);
        assert_eq!(first, 4 * 1024);
        let p = f.next_packet().unwrap().unwrap();
        assert_eq!(p.ts, 4 * 1024);
        assert_eq!(p.data, vec![4u8; 14]);
        // seeking backwards works too
        assert_eq!(f.seek(0), 0);
        assert_eq!(f.next_packet().unwrap().unwrap().ts, 0);
        // past the end clamps to the last segment
        assert_eq!(f.seek(u64::MAX / 2), 8 * 1024);
    }

    #[test]
    fn rejects_files_without_index() {
        let mut data = build(1, 2);
        // corrupt the 'sidx' fourcc
        let pos = data.windows(4).position(|w| w == b"sidx").unwrap();
        data[pos..pos + 4].copy_from_slice(b"free");
        assert!(Fmp4::open(Box::new(Mem(Cursor::new(data)))).is_err());
        assert!(Fmp4::open(Box::new(Mem(Cursor::new(vec![1, 2, 3])))).is_err());
    }

    #[test]
    fn truncated_segment_is_an_error_not_a_panic() {
        let mut data = build(2, 4);
        let len = data.len();
        data.truncate(len - 20);
        // sidx now points past the end
        assert!(Fmp4::open(Box::new(Mem(Cursor::new(data)))).is_err());
    }

    #[test]
    fn esds_parser() {
        let mut e = vec![0, 0, 0, 0, 0x03, 25, 0, 1, 0, 0x04, 17, 0x40, 0x15, 0, 0, 0];
        e.extend([0u8; 8]);
        e.extend([0x05, 2, 0x12, 0x10]);
        assert_eq!(parse_esds(&e), Some(vec![0x12, 0x10]));
        assert_eq!(parse_esds(&[0, 0, 0, 0, 9]), None);
    }
}
