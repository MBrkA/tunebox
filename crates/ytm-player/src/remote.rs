//! Seekable HTTP source for symphonia.
//!
//! googlevideo rejects open-ended and large ranges, so the file is fetched in
//! fixed-size chunks (256 KiB) by a few background workers (each request has
//! ~0.2 s of latency, so parallelism matters more than chunk size). Readers
//! block until the chunk they need is present and tell the workers which chunk
//! is wanted next, so seeking jumps the download queue. Fetched chunks stay in memory,
//! which makes seeking backwards and replaying free.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use symphonia::core::io::MediaSource;

pub const CHUNK: usize = 256 << 10;
const WORKERS: usize = 3;
const RETRIES: u32 = 3;

/// One pooled client for the whole process: keeps TLS connections warm
/// between chunks and tracks.
fn client() -> io::Result<reqwest::blocking::Client> {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    if let Some(c) = CLIENT.get() {
        return Ok(c.clone());
    }
    let c = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(to_io)?;
    Ok(CLIENT.get_or_init(|| c).clone())
}

struct State {
    chunks: Vec<Option<Arc<Vec<u8>>>>,
    inflight: Vec<bool>,
    /// Chunk a reader is currently waiting for (or last read).
    wanted: usize,
    error: Option<String>,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    cv: Condvar,
    len: u64,
    chunk: usize,
}

/// Owner handle: dropping it stops the downloader and makes readers fail.
pub struct RemoteFile {
    shared: Arc<Shared>,
}

impl RemoteFile {
    /// Starts downloading `url`. `len` is the total size; pass `None` to probe it.
    /// Blocking (probes the length if needed); call from a blocking context.
    pub fn open(url: &str, user_agent: &str, len: Option<u64>) -> io::Result<Self> {
        Self::open_with(url, user_agent, len, CHUNK)
    }

    pub fn open_with(
        url: &str,
        user_agent: &str,
        len: Option<u64>,
        chunk: usize,
    ) -> io::Result<Self> {
        let client = client()?;
        let len = match len {
            Some(l) => l,
            None => probe_len(&client, url, user_agent)?,
        };
        let n_chunks = (len as usize).div_ceil(chunk);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                chunks: vec![None; n_chunks],
                inflight: vec![false; n_chunks],
                wanted: 0,
                error: None,
                closed: false,
            }),
            cv: Condvar::new(),
            len,
            chunk,
        });
        for n in 0..WORKERS {
            let worker = shared.clone();
            let client = client.clone();
            let (url, ua) = (url.to_owned(), user_agent.to_owned());
            thread::Builder::new()
                .name(format!("ytm-download-{n}"))
                .spawn(move || download_loop(&worker, &client, &url, &ua))?;
        }
        Ok(Self { shared })
    }

    pub fn len(&self) -> u64 {
        self.shared.len
    }

    pub fn is_empty(&self) -> bool {
        self.shared.len == 0
    }

    /// A new independent read cursor over the shared data.
    pub fn reader(&self) -> RemoteReader {
        RemoteReader {
            shared: self.shared.clone(),
            pos: 0,
        }
    }

    pub fn buffered_bytes(&self) -> u64 {
        let st = self.shared.state.lock().unwrap();
        st.chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_some())
            .map(|(i, _)| chunk_len(&self.shared, i) as u64)
            .sum()
    }
}

impl Drop for RemoteFile {
    fn drop(&mut self) {
        if let Ok(mut st) = self.shared.state.lock() {
            st.closed = true;
        }
        self.shared.cv.notify_all();
    }
}

fn to_io(e: reqwest::Error) -> io::Error {
    io::Error::other(e)
}

fn chunk_len(sh: &Shared, idx: usize) -> usize {
    let start = idx as u64 * sh.chunk as u64;
    (sh.len - start).min(sh.chunk as u64) as usize
}

fn probe_len(client: &reqwest::blocking::Client, url: &str, ua: &str) -> io::Result<u64> {
    let resp = client
        .get(url)
        .header("User-Agent", ua)
        .header("Range", "bytes=0-0")
        .send()
        .map_err(to_io)?;
    resp.headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| io::Error::other("server did not report content length"))
}

fn fetch_chunk(
    sh: &Shared,
    client: &reqwest::blocking::Client,
    url: &str,
    ua: &str,
    idx: usize,
) -> Result<Vec<u8>, String> {
    let start = idx as u64 * sh.chunk as u64;
    let want = chunk_len(sh, idx);
    let end = start + want as u64 - 1;
    let mut last = String::new();
    for attempt in 0..RETRIES {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(300 * u64::from(attempt)));
        }
        let res = client
            .get(url)
            .header("User-Agent", ua)
            .header("Range", format!("bytes={start}-{end}"))
            .send();
        match res {
            Ok(r) if r.status().as_u16() == 206 || (r.status().is_success() && start == 0) => {
                match r.bytes() {
                    Ok(b) if b.len() >= want => return Ok(b[..want].to_vec()),
                    Ok(b) => last = format!("short read: {} of {want} bytes", b.len()),
                    Err(e) => last = e.to_string(),
                }
            }
            Ok(r) => {
                last = format!("HTTP {}", r.status());
                // 403/404/410: the URL is dead; retrying will not help.
                if matches!(r.status().as_u16(), 403 | 404 | 410) {
                    break;
                }
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

fn download_loop(sh: &Shared, client: &reqwest::blocking::Client, url: &str, ua: &str) {
    loop {
        let idx = {
            let mut st = sh.state.lock().unwrap();
            loop {
                if st.closed || st.error.is_some() {
                    return;
                }
                let n = st.chunks.len();
                let next = (st.wanted..n)
                    .chain(0..st.wanted.min(n))
                    .find(|&i| st.chunks[i].is_none() && !st.inflight[i]);
                match next {
                    Some(i) => {
                        st.inflight[i] = true;
                        break i;
                    }
                    None => st = sh.cv.wait(st).unwrap(),
                }
            }
        };
        let result = fetch_chunk(sh, client, url, ua, idx);
        let mut st = sh.state.lock().unwrap();
        st.inflight[idx] = false;
        match result {
            Ok(data) => st.chunks[idx] = Some(Arc::new(data)),
            Err(e) => {
                tracing::warn!(chunk = idx, error = %e, "chunk download failed");
                st.error = Some(e);
            }
        }
        drop(st);
        sh.cv.notify_all();
    }
}

#[derive(Clone)]
pub struct RemoteReader {
    shared: Arc<Shared>,
    pos: u64,
}

impl Read for RemoteReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.shared.len || buf.is_empty() {
            return Ok(0);
        }
        let sh = &self.shared;
        let idx = (self.pos / sh.chunk as u64) as usize;
        let chunk = {
            let mut st = sh.state.lock().unwrap();
            loop {
                if let Some(c) = &st.chunks[idx] {
                    break c.clone();
                }
                if st.closed {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "stream closed"));
                }
                // A failed *other* chunk must not fail this read: once an error is recorded the
                // workers stop picking up new chunks, so only a chunk that is not already being
                // fetched can never arrive. (A worker finishing wakes us via the condvar.)
                if let Some(e) = &st.error {
                    if !st.inflight[idx] {
                        return Err(io::Error::other(format!("download failed: {e}")));
                    }
                }
                if st.wanted != idx {
                    st.wanted = idx;
                    sh.cv.notify_all();
                }
                st = sh.cv.wait(st).unwrap();
            }
        };
        // Keep the downloader reading ahead of us.
        {
            let mut st = sh.state.lock().unwrap();
            if st.wanted != idx {
                st.wanted = idx;
                drop(st);
                sh.cv.notify_all();
            }
        }
        let off = (self.pos % sh.chunk as u64) as usize;
        let n = buf.len().min(chunk.len() - off);
        buf[..n].copy_from_slice(&chunk[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for RemoteReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let new = match from {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
            SeekFrom::End(d) => self.shared.len as i128 + d as i128,
        };
        if new < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before start",
            ));
        }
        self.pos = new as u64;
        Ok(self.pos)
    }
}

impl MediaSource for RemoteReader {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.shared.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Minimal HTTP/1.1 range server. Ranges starting at or beyond
    /// `fail_from` bytes get a 403.
    fn serve(data: Arc<Vec<u8>>, fail_from: usize) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/a", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let (data, c) = (data.clone(), c.clone());
                thread::spawn(move || {
                    let mut rd = BufReader::new(stream.try_clone().unwrap());
                    let mut range = None;
                    loop {
                        let mut line = String::new();
                        if rd.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(r) = line.to_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            range =
                                Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                        }
                    }
                    c.fetch_add(1, Ordering::SeqCst);
                    if range.is_some_and(|(a, _)| a >= fail_from) {
                        let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    }
                    let (a, b) = range.unwrap_or((0, data.len() - 1));
                    let b = b.min(data.len() - 1);
                    let body = &data[a..=b];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {a}-{b}/{}\r\nConnection: close\r\n\r\n",
                        body.len(),
                        data.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                });
            }
        });
        (url, count)
    }

    fn data(n: usize) -> Arc<Vec<u8>> {
        Arc::new((0..n).map(|i| (i * 7 + i / 251) as u8).collect())
    }

    #[test]
    fn sequential_read_matches_source() {
        let d = data(10_000);
        let (url, _) = serve(d.clone(), usize::MAX);
        let f = RemoteFile::open_with(&url, "ua", None, 1024).unwrap();
        assert_eq!(f.len(), 10_000);
        let mut out = Vec::new();
        f.reader().read_to_end(&mut out).unwrap();
        assert_eq!(out, *d);
    }

    #[test]
    fn seek_jumps_and_rereads() {
        let d = data(10_000);
        let (url, _) = serve(d.clone(), usize::MAX);
        let f = RemoteFile::open_with(&url, "ua", Some(10_000), 1024).unwrap();
        let mut r = f.reader();
        let mut buf = [0u8; 100];
        r.seek(SeekFrom::Start(9_000)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf[..], d[9_000..9_100]);
        r.seek(SeekFrom::Start(5)).unwrap();
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf[..], d[5..105]);
        r.seek(SeekFrom::End(-10)).unwrap();
        let mut tail = Vec::new();
        r.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, d[9_990..]);
        assert!(r.seek(SeekFrom::Current(-100_000)).is_err());
    }

    #[test]
    fn dead_url_surfaces_error_but_cached_chunks_still_read() {
        let d = data(4096);
        let (url, _) = serve(d.clone(), 2048);
        let f = RemoteFile::open_with(&url, "ua", Some(4096), 1024).unwrap();
        let mut r = f.reader();
        let mut buf = [0u8; 10];
        r.read_exact(&mut buf).unwrap();
        assert_eq!(buf[..], d[..10]);
        r.seek(SeekFrom::Start(3000)).unwrap();
        let err = r.read(&mut buf).unwrap_err();
        assert!(err.to_string().contains("403"), "{err}");
        // chunk 0 is cached, still readable
        r.seek(SeekFrom::Start(0)).unwrap();
        r.read_exact(&mut buf).unwrap();
    }

    #[test]
    fn dropping_owner_unblocks_readers() {
        let d = data(4096);
        let (url, _) = serve(d, usize::MAX);
        let f = RemoteFile::open_with(&url, "ua", Some(4096), 1024).unwrap();
        let mut r = f.reader();
        drop(f);
        let mut buf = [0u8; 4096];
        // Either already-fetched data or a clean error; never a hang.
        let _ = r.read(&mut buf);
    }

    #[test]
    fn downloader_fetches_everything_in_the_background() {
        let d = data(5000);
        let (url, count) = serve(d, usize::MAX);
        let f = RemoteFile::open_with(&url, "ua", Some(5000), 1024).unwrap();
        for _ in 0..100 {
            if f.buffered_bytes() == 5000 {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(f.buffered_bytes(), 5000);
        assert_eq!(count.load(Ordering::SeqCst), 5);
    }
}
