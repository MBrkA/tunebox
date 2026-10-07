//! The playback engine.
//!
//! Threads:
//! * **controller** (tokio task): owns the [`Queue`], resolves streams, schedules
//!   prefetch, handles commands and publishes [`PlayerState`].
//! * **downloader** (one std thread per track, see `remote.rs`): fetches 1 MiB chunks.
//! * **decoder** (one std thread): turns jobs into samples and pushes them into
//!   the shared [`AudioBuf`]. It starts the next prefetched job the moment the
//!   current one is fully decoded, which is what makes transitions gapless.
//! * **audio** (cpal callback / null sink): drains the buffer and does the
//!   position/track-boundary bookkeeping.
//!
//! Anything that invalidates buffered audio (seek, skip, new queue) calls
//! `AudioBuf::flush`, which bumps a generation counter; jobs carrying an older
//! generation abort on their own, so no thread ever has to be killed.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc as std_mpsc, Arc, RwLock};
use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use ytm_api::{StreamResolver, Track};

use crate::decode::Decoder;
use crate::output::{self, AudioBuf, OutputKind, Sink};
use crate::queue::{Queue, RemoveOutcome, Repeat};
use crate::remote::{RemoteFile, RemoteReader};
use crate::resample::{remap_channels, StreamResampler};
use crate::PlayerError;

const TICK: Duration = Duration::from_millis(50);
/// "Previous" restarts the current track when it has played longer than this.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    /// Resolving/buffering; no audio yet.
    Loading,
    Playing,
    Paused,
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub status: Status,
    pub tracks: Arc<Vec<Track>>,
    /// Index into `tracks` of the current track.
    pub current: Option<usize>,
    /// Indices that play after the current one, in play order.
    pub upcoming: Arc<Vec<usize>>,
    pub duration: Option<Duration>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Last playback error, cleared when a track starts playing.
    pub error: Option<String>,
    /// Incremented on every published change.
    pub version: u64,
}

impl PlayerState {
    pub fn current_track(&self) -> Option<&Track> {
        self.current.and_then(|i| self.tracks.get(i))
    }
}

#[derive(Debug, Clone)]
pub enum Command {
    /// Replace the queue and start playing at `start`.
    Play {
        tracks: Vec<Track>,
        start: usize,
    },
    /// Rebuild a saved session: queue, shuffle/repeat and position, left paused.
    Restore {
        tracks: Vec<Track>,
        start: usize,
        position: Duration,
        shuffle: bool,
        repeat: Repeat,
    },
    Enqueue(Vec<Track>),
    PlayNext(Track),
    Remove(usize),
    /// Reorder the "next up" list: positions within `Queue::upcoming()`.
    MoveUpcoming {
        from: usize,
        to: usize,
    },
    Clear,
    /// Play the queue entry at this track index.
    Jump(usize),
    Pause,
    Resume,
    Toggle,
    Stop,
    Next,
    Prev,
    Seek(Duration),
    SetVolume(f32),
    SetShuffle(bool),
    SetRepeat(Repeat),
}

#[derive(Debug, Clone)]
pub enum PlayerEvent {
    /// Anything in [`PlayerState`] changed.
    StateChanged,
    Error(String),
}

pub struct PlayerOptions {
    pub volume: f32,
    pub output: OutputKind,
    pub buffer_secs: f32,
    /// Called (from the controller task) whenever state changes; use it to request a UI repaint.
    pub notify: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        Self {
            volume: 0.8,
            output: OutputKind::Device,
            buffer_secs: 3.0,
            notify: None,
        }
    }
}

struct Shared {
    state: RwLock<PlayerState>,
    buf: Arc<AudioBuf>,
    current_job: AtomicU64,
    pending_pos_ms: AtomicU64,
    events: broadcast::Sender<PlayerEvent>,
}

enum Msg {
    Cmd(Command),
    Loaded {
        epoch: u64,
        kind: LoadKind,
        result: Result<Prepared, PlayerError>,
    },
    Decode(DecodeEvent),
    Shutdown,
}

#[derive(Debug)]
enum DecodeEvent {
    Started(u64),
    Finished(u64),
    Failed(u64, PlayerError),
    Cancelled,
}

#[derive(Clone, Copy)]
enum LoadKind {
    Current { start: Duration },
    Next { index: usize },
}

struct Prepared {
    video_id: String,
    file: Arc<RemoteFile>,
    duration: Option<Duration>,
}

/// Sends `Shutdown` when the last `Player` clone is dropped.
struct Guard(mpsc::UnboundedSender<Msg>);

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.send(Msg::Shutdown);
    }
}

/// Cheap, clonable handle to the engine.
#[derive(Clone)]
pub struct Player {
    guard: Arc<Guard>,
    shared: Arc<Shared>,
}

impl Player {
    pub fn spawn(
        rt: &tokio::runtime::Handle,
        resolver: Arc<dyn StreamResolver>,
        opts: PlayerOptions,
    ) -> Result<Self, PlayerError> {
        let volume = opts.volume;
        let buffer_secs = opts.buffer_secs;
        let (buf, sink) = output::open(opts.output, move |fmt| {
            AudioBuf::new(fmt.rate, fmt.channels, buffer_secs)
        })
        .map_err(PlayerError::Output)?;
        buf.set_volume(volume);

        let (tx, rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(64);
        let shared = Arc::new(Shared {
            state: RwLock::new(PlayerState {
                status: Status::Idle,
                tracks: Arc::new(Vec::new()),
                current: None,
                upcoming: Arc::new(Vec::new()),
                duration: None,
                volume,
                shuffle: false,
                repeat: Repeat::Off,
                error: None,
                version: 0,
            }),
            buf: buf.clone(),
            current_job: AtomicU64::new(0),
            pending_pos_ms: AtomicU64::new(0),
            events,
        });

        let (jobs_tx, jobs_rx) = std_mpsc::channel::<Job>();
        {
            let buf = buf.clone();
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("ytm-decode".into())
                .spawn(move || decode_thread(jobs_rx, buf, tx))
                .map_err(|e| PlayerError::Output(e.to_string()))?;
        }

        let engine = Engine {
            queue: Queue::new(),
            shared: shared.clone(),
            resolver,
            jobs: jobs_tx,
            tx: tx.clone(),
            rt: rt.clone(),
            notify: opts.notify,
            _sink: sink,
            job_counter: 0,
            epoch: 0,
            next_epoch: 0,
            current: None,
            loading_current: false,
            next: None,
            loading_next: None,
            next_failed: None,
            paused: false,
            audible: false,
            error: None,
            consecutive_errors: 0,
            recovered: false,
            version: 0,
            tracks_cache: Arc::new(Vec::new()),
        };
        rt.spawn(engine.run(rx));
        Ok(Self {
            guard: Arc::new(Guard(tx)),
            shared,
        })
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.guard.0.send(Msg::Cmd(cmd));
    }

    pub fn state(&self) -> PlayerState {
        self.shared.state.read().unwrap().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.shared.events.subscribe()
    }

    /// Current playback position. Cheap enough to call every frame.
    pub fn position(&self) -> Duration {
        let (job, pos) = self.shared.buf.now_playing();
        if job != 0 && job == self.shared.current_job.load(Ordering::Relaxed) {
            pos
        } else {
            Duration::from_millis(self.shared.pending_pos_ms.load(Ordering::Relaxed))
        }
    }

    /// Number of audio callbacks that ran out of data (audible glitches).
    pub fn underruns(&self) -> u64 {
        self.shared.buf.underruns()
    }

    /// Seconds of audio currently buffered ahead of the playhead.
    pub fn buffered(&self) -> Duration {
        self.shared.buf.buffered()
    }
}

// ---------------------------------------------------------------------------
// decode thread

struct Job {
    id: u64,
    generation: u64,
    reader: RemoteReader,
    start: Duration,
    cancel: Arc<AtomicBool>,
}

fn decode_thread(
    jobs: std_mpsc::Receiver<Job>,
    buf: Arc<AudioBuf>,
    events: mpsc::UnboundedSender<Msg>,
) {
    while let Ok(job) = jobs.recv() {
        let id = job.id;
        let ev = match run_job(job, &buf, &events) {
            Ok(()) => DecodeEvent::Finished(id),
            Err(None) => DecodeEvent::Cancelled,
            Err(Some(e)) => DecodeEvent::Failed(id, e),
        };
        if events.send(Msg::Decode(ev)).is_err() {
            return;
        }
    }
}

/// `Err(None)` = cancelled/stale, `Err(Some(_))` = real failure.
fn run_job(
    job: Job,
    buf: &AudioBuf,
    events: &mpsc::UnboundedSender<Msg>,
) -> Result<(), Option<PlayerError>> {
    let stale = || job.cancel.load(Ordering::Relaxed) || buf.generation() != job.generation;
    let fail = |e: PlayerError| if stale() { None } else { Some(e) };
    if stale() {
        return Err(None);
    }
    tracing::debug!(job = job.id, "decoder opening");
    let t0 = std::time::Instant::now();
    let mut dec = Decoder::open(job.reader, job.start).map_err(fail)?;
    tracing::debug!(
        job = job.id,
        elapsed_ms = t0.elapsed().as_millis() as u64,
        rate = dec.rate,
        channels = dec.channels,
        "decoder opened"
    );
    let mut resampler = StreamResampler::new(dec.rate, buf.rate, buf.channels)
        .map_err(|e| fail(PlayerError::Decode(e)))?;
    if !buf.begin_job(job.generation, job.id, job.start.as_millis() as u64) {
        return Err(None);
    }
    let _ = events.send(Msg::Decode(DecodeEvent::Started(job.id)));

    let (mut raw, mut mapped, mut out) = (Vec::new(), Vec::new(), Vec::new());
    loop {
        if stale() {
            return Err(None);
        }
        raw.clear();
        if !dec.next_samples(&mut raw).map_err(fail)? {
            break;
        }
        mapped.clear();
        remap_channels(&raw, dec.channels, buf.channels, &mut mapped);
        out.clear();
        resampler
            .process(&mapped, &mut out)
            .map_err(|e| fail(PlayerError::Decode(e)))?;
        if !buf.push(job.generation, &out) {
            return Err(None);
        }
    }
    out.clear();
    resampler
        .finish(&mut out)
        .map_err(|e| fail(PlayerError::Decode(e)))?;
    if buf.push(job.generation, &out) {
        Ok(())
    } else {
        Err(None)
    }
}

// ---------------------------------------------------------------------------
// controller

struct Loaded {
    job: u64,
    video_id: String,
    file: Arc<RemoteFile>,
    duration: Option<Duration>,
    finished: bool,
}

struct Prefetched {
    index: usize,
    video_id: String,
    file: Arc<RemoteFile>,
    duration: Option<Duration>,
    job: Option<QueuedJob>,
}

struct QueuedJob {
    id: u64,
    started: bool,
    /// Decoding completed (possibly before the track became audible).
    finished: bool,
    cancel: Arc<AtomicBool>,
}

struct Engine {
    queue: Queue,
    shared: Arc<Shared>,
    resolver: Arc<dyn StreamResolver>,
    jobs: std_mpsc::Sender<Job>,
    tx: mpsc::UnboundedSender<Msg>,
    rt: tokio::runtime::Handle,
    notify: Option<Arc<dyn Fn() + Send + Sync>>,
    _sink: Sink,

    job_counter: u64,
    /// Bumped whenever a pending "current track" load becomes obsolete.
    epoch: u64,
    next_epoch: u64,

    current: Option<Loaded>,
    loading_current: bool,
    next: Option<Prefetched>,
    /// (epoch, index) of an in-flight prefetch.
    loading_next: Option<(u64, usize)>,
    /// Index whose prefetch failed; do not retry until the queue changes.
    next_failed: Option<usize>,

    paused: bool,
    /// The current job has become audible.
    audible: bool,
    error: Option<String>,
    consecutive_errors: usize,
    recovered: bool,
    version: u64,
    tracks_cache: Arc<Vec<Track>>,
}

impl Engine {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(Msg::Shutdown) | None => break,
                    Some(msg) => self.handle(msg),
                },
                _ = tick.tick() => self.on_tick(),
            }
        }
        self.reset_playback();
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Cmd(cmd) => self.on_command(cmd),
            Msg::Loaded {
                epoch,
                kind,
                result,
            } => self.on_loaded(epoch, kind, result),
            Msg::Decode(ev) => self.on_decode(ev),
            Msg::Shutdown => {}
        }
    }

    // ---- commands ----------------------------------------------------------

    fn on_command(&mut self, cmd: Command) {
        match cmd {
            Command::Play { tracks, start } => {
                self.queue.set(tracks, start);
                self.paused = false;
                self.shared.buf.set_paused(false);
                self.consecutive_errors = 0;
                self.next_failed = None;
                self.start_current(Duration::ZERO, false);
            }
            Command::Restore {
                tracks,
                start,
                position,
                shuffle,
                repeat,
            } => {
                self.queue.set_shuffle(shuffle);
                self.queue.set_repeat(repeat);
                self.queue.set(tracks, start);
                self.paused = true;
                self.shared.buf.set_paused(true);
                self.consecutive_errors = 0;
                self.next_failed = None;
                self.start_current(position, false);
            }
            Command::Enqueue(tracks) => {
                for t in tracks {
                    self.queue.push(t);
                }
                self.queue_changed();
            }
            Command::PlayNext(t) => {
                self.queue.push_next(t);
                self.queue_changed();
            }
            Command::Remove(i) => match self.queue.remove(i) {
                RemoveOutcome::CurrentRemoved => {
                    if self.queue.current().is_some() {
                        self.start_current(Duration::ZERO, false);
                    } else {
                        self.stop();
                    }
                }
                RemoveOutcome::Other => self.queue_changed(),
                RemoveOutcome::OutOfRange => {}
            },
            Command::MoveUpcoming { from, to } => {
                if self.queue.move_upcoming(from, to) {
                    self.queue_changed();
                }
            }
            Command::Clear => {
                self.queue.clear();
                self.stop();
            }
            Command::Jump(i) => {
                if self.queue.jump_to(i) {
                    self.paused = false;
                    self.shared.buf.set_paused(false);
                    self.start_current(Duration::ZERO, false);
                }
            }
            Command::Pause => self.set_paused(true),
            Command::Resume => {
                if self.current.is_none() && !self.loading_current && self.queue.current().is_some()
                {
                    self.start_current(Duration::ZERO, false);
                }
                self.set_paused(false);
            }
            Command::Toggle => {
                let idle = self.current.is_none() && !self.loading_current;
                if idle && self.queue.current().is_some() {
                    self.set_paused(false);
                    self.start_current(Duration::ZERO, false);
                } else {
                    self.set_paused(!self.paused);
                }
            }
            Command::Stop => self.stop(),
            Command::Next => {
                self.consecutive_errors = 0;
                if self.queue.skip_next().is_some() {
                    self.start_current(Duration::ZERO, false);
                }
            }
            Command::Prev => {
                if self.position() > RESTART_THRESHOLD {
                    self.seek(Duration::ZERO);
                } else if self.queue.skip_prev().is_some() {
                    self.start_current(Duration::ZERO, false);
                }
            }
            Command::Seek(pos) => self.seek(pos),
            Command::SetVolume(v) => {
                self.shared.buf.set_volume(v);
                self.publish();
            }
            Command::SetShuffle(on) => {
                self.queue.set_shuffle(on);
                self.queue_changed();
            }
            Command::SetRepeat(r) => {
                self.queue.set_repeat(r);
                self.queue_changed();
            }
        }
    }

    fn set_paused(&mut self, p: bool) {
        self.paused = p;
        self.shared.buf.set_paused(p);
        self.publish();
    }

    fn position(&self) -> Duration {
        let (job, pos) = self.shared.buf.now_playing();
        if self.audible && Some(job) == self.current.as_ref().map(|c| c.job) {
            pos
        } else {
            Duration::from_millis(self.shared.pending_pos_ms.load(Ordering::Relaxed))
        }
    }

    fn stop(&mut self) {
        self.reset_playback();
        self.shared.pending_pos_ms.store(0, Ordering::Relaxed);
        self.publish();
    }

    /// Drops all audio and playback state; the queue is left alone.
    fn reset_playback(&mut self) {
        self.epoch += 1;
        self.next_epoch += 1;
        self.shared.buf.flush();
        if let Some(n) = &self.next {
            if let Some(j) = &n.job {
                j.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.current = None;
        self.next = None;
        self.loading_current = false;
        self.loading_next = None;
        self.audible = false;
        self.recovered = false;
        self.shared.current_job.store(0, Ordering::Relaxed);
    }

    /// Queue contents/order changed in a way that may affect the upcoming track.
    fn queue_changed(&mut self) {
        self.next_failed = None;
        self.maybe_prefetch();
        self.publish();
    }

    // ---- loading -----------------------------------------------------------

    /// Starts playing the queue's current track from `start`.
    /// `fresh` forces a new URL resolution (used to recover from expired URLs).
    fn start_current(&mut self, start: Duration, fresh: bool) {
        let Some(track) = self.queue.current().cloned() else {
            self.stop();
            return;
        };
        let reusable = (!fresh)
            .then(|| {
                self.current
                    .as_ref()
                    .filter(|c| c.video_id == track.video_id)
                    .map(|c| (c.file.clone(), c.duration))
                    .or_else(|| {
                        self.next
                            .as_ref()
                            .filter(|n| n.video_id == track.video_id)
                            .map(|n| (n.file.clone(), n.duration))
                    })
            })
            .flatten();
        let keep_recovered = self.recovered && fresh;
        self.reset_playback();
        self.recovered = keep_recovered;
        self.error = None;
        self.shared
            .pending_pos_ms
            .store(start.as_millis() as u64, Ordering::Relaxed);

        match reusable {
            Some((file, duration)) => {
                self.begin_current(track.video_id, file, duration, start);
            }
            None => {
                self.loading_current = true;
                self.spawn_load(track, LoadKind::Current { start }, self.epoch);
            }
        }
        self.publish();
    }

    fn spawn_load(&self, track: Track, kind: LoadKind, epoch: u64) {
        let resolver = self.resolver.clone();
        let tx = self.tx.clone();
        self.rt.spawn(async move {
            let result = async {
                let t0 = std::time::Instant::now();
                let info = resolver.resolve(&track.video_id).await?;
                tracing::debug!(id = %track.video_id, itag = info.itag, bytes = ?info.content_length, elapsed_ms = t0.elapsed().as_millis() as u64, "stream resolved");
                let duration = info.duration_ms.map(Duration::from_millis).or(track
                    .duration_secs
                    .map(|s| Duration::from_secs(u64::from(s))));
                let (url, ua, len) = (info.url, info.user_agent, info.content_length);
                let file = tokio::task::spawn_blocking(move || RemoteFile::open(&url, &ua, len))
                    .await
                    .map_err(|e| PlayerError::Io(e.to_string()))?
                    .map_err(|e| PlayerError::Io(e.to_string()))?;
                Ok(Prepared {
                    video_id: track.video_id.clone(),
                    file: Arc::new(file),
                    duration,
                })
            }
            .await;
            let _ = tx.send(Msg::Loaded {
                epoch,
                kind,
                result,
            });
        });
    }

    fn on_loaded(&mut self, epoch: u64, kind: LoadKind, result: Result<Prepared, PlayerError>) {
        match kind {
            LoadKind::Current { start } => {
                if epoch != self.epoch || !self.loading_current {
                    return; // superseded
                }
                self.loading_current = false;
                match result {
                    Ok(p) => {
                        self.begin_current(p.video_id, p.file, p.duration, start);
                        self.publish();
                    }
                    Err(e) => self.fail_current(e),
                }
            }
            LoadKind::Next { index } => {
                if epoch != self.next_epoch || self.loading_next.map(|l| l.1) != Some(index) {
                    return;
                }
                self.loading_next = None;
                match result {
                    Ok(p) if self.queue.peek_advance() == Some(index) => {
                        self.next = Some(Prefetched {
                            index,
                            video_id: p.video_id,
                            file: p.file,
                            duration: p.duration,
                            job: None,
                        });
                        self.enqueue_next_job();
                    }
                    Ok(_) => self.maybe_prefetch(),
                    Err(e) => {
                        tracing::warn!(error = %e, "prefetch failed");
                        self.next_failed = Some(index);
                    }
                }
            }
        }
    }

    /// The file for the current track is ready: queue its decode job.
    fn begin_current(
        &mut self,
        video_id: String,
        file: Arc<RemoteFile>,
        duration: Option<Duration>,
        start: Duration,
    ) {
        let id = self.send_job(&file, start, Arc::new(AtomicBool::new(false)));
        self.shared.current_job.store(id, Ordering::Relaxed);
        self.current = Some(Loaded {
            job: id,
            video_id,
            file,
            duration,
            finished: false,
        });
        self.audible = false;
        self.maybe_prefetch();
    }

    fn send_job(&mut self, file: &RemoteFile, start: Duration, cancel: Arc<AtomicBool>) -> u64 {
        self.job_counter += 1;
        let id = self.job_counter;
        let _ = self.jobs.send(Job {
            id,
            generation: self.shared.buf.generation(),
            reader: file.reader(),
            start,
            cancel,
        });
        id
    }

    fn enqueue_next_job(&mut self) {
        let Some(n) = &self.next else { return };
        if n.job.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let file = n.file.clone();
        let id = self.send_job(&file, Duration::ZERO, cancel.clone());
        if let Some(n) = &mut self.next {
            n.job = Some(QueuedJob {
                id,
                started: false,
                finished: false,
                cancel,
            });
        }
    }

    /// Makes sure the track that follows the current one is being prepared.
    fn maybe_prefetch(&mut self) {
        if self.current.is_none() {
            return;
        }
        let want = self.queue.peek_advance();
        if let Some(n) = &self.next {
            if Some(n.index) == want {
                self.enqueue_next_job();
                return;
            }
            // Wrong track prefetched. If its audio already entered the buffer we
            // must rebuild the buffer from the current position.
            let started = n.job.as_ref().is_some_and(|j| j.started);
            if let Some(j) = &n.job {
                j.cancel.store(true, Ordering::Relaxed);
            }
            self.next = None;
            if started {
                self.seek(self.position());
            }
        }
        if let Some((_, idx)) = self.loading_next {
            if Some(idx) == want {
                return;
            }
            self.loading_next = None;
            self.next_epoch += 1;
        }
        let Some(index) = want else { return };
        if self.next_failed == Some(index) {
            return;
        }
        let Some(track) = self.queue.track(index).cloned() else {
            return;
        };
        // Repeat-one / single-track repeat: the file is already in memory.
        if let Some(cur) = self
            .current
            .as_ref()
            .filter(|c| c.video_id == track.video_id)
        {
            self.next = Some(Prefetched {
                index,
                video_id: cur.video_id.clone(),
                file: cur.file.clone(),
                duration: cur.duration,
                job: None,
            });
            self.enqueue_next_job();
            return;
        }
        self.next_epoch += 1;
        self.loading_next = Some((self.next_epoch, index));
        self.spawn_load(track, LoadKind::Next { index }, self.next_epoch);
    }

    fn seek(&mut self, pos: Duration) {
        let Some(cur) = &self.current else {
            return;
        };
        let file = cur.file.clone();
        let pos = match cur.duration {
            Some(d) => pos.min(d.saturating_sub(Duration::from_millis(500))),
            None => pos,
        };
        self.shared.buf.flush();
        // Queued prefetch job is now stale (old generation); re-queue it afterwards.
        if let Some(n) = &mut self.next {
            n.job = None;
        }
        let id = self.send_job(&file, pos, Arc::new(AtomicBool::new(false)));
        self.shared.current_job.store(id, Ordering::Relaxed);
        if let Some(c) = &mut self.current {
            c.job = id;
            c.finished = false;
        }
        self.audible = false;
        self.shared
            .pending_pos_ms
            .store(pos.as_millis() as u64, Ordering::Relaxed);
        self.enqueue_next_job();
        self.publish();
    }

    // ---- decode events & ticks -------------------------------------------------

    fn on_decode(&mut self, ev: DecodeEvent) {
        match ev {
            DecodeEvent::Started(id) => {
                if let Some(j) = self.next.as_mut().and_then(|n| n.job.as_mut()) {
                    if j.id == id {
                        j.started = true;
                    }
                }
            }
            DecodeEvent::Finished(id) => {
                if let Some(c) = self.current.as_mut().filter(|c| c.job == id) {
                    c.finished = true;
                } else if let Some(j) = self.next.as_mut().and_then(|n| n.job.as_mut()) {
                    if j.id == id {
                        j.finished = true;
                    }
                }
            }
            DecodeEvent::Cancelled => {}
            DecodeEvent::Failed(id, err) => {
                if self.current.as_ref().is_some_and(|c| c.job == id) {
                    self.on_current_failed(err);
                } else if self
                    .next
                    .as_ref()
                    .and_then(|n| n.job.as_ref())
                    .is_some_and(|j| j.id == id)
                {
                    tracing::warn!(error = %err, "prefetched track failed to decode");
                    self.next_failed = self.next.as_ref().map(|n| n.index);
                    self.next = None;
                }
            }
        }
    }

    fn on_current_failed(&mut self, err: PlayerError) {
        // Most mid-track failures are expired/dead URLs: resolve again once and resume.
        if !self.recovered {
            tracing::warn!(error = %err, "playback failed, re-resolving stream");
            let pos = self.position();
            self.start_current(pos, true);
            self.recovered = true;
            return;
        }
        self.fail_current(err);
    }

    /// Reports an error for the current track and moves on to the next one.
    fn fail_current(&mut self, err: PlayerError) {
        tracing::error!(error = %err, "track failed");
        let msg = err.to_string();
        let _ = self.shared.events.send(PlayerEvent::Error(msg.clone()));
        self.consecutive_errors += 1;
        let give_up = self.consecutive_errors >= self.queue.len().max(1);
        if !give_up && self.queue.advance().is_some() {
            self.start_current(Duration::ZERO, false);
        } else {
            self.reset_playback();
        }
        self.error = Some(msg);
        self.publish();
    }

    fn on_tick(&mut self) {
        let (job, _) = self.shared.buf.now_playing();
        let mut changed = false;

        // Gapless hand-over: the audio callback crossed into the prefetched track.
        let promote = self.next.as_ref().is_some_and(|n| {
            n.job.as_ref().is_some_and(|j| j.id == job)
                && self.current.as_ref().map(|c| c.job) != Some(job)
        });
        if promote {
            let n = self.next.take().expect("checked above");
            let finished = n.job.as_ref().is_some_and(|j| j.finished);
            if self.queue.advance().is_some() && self.queue.current_index() == Some(n.index) {
                self.current = Some(Loaded {
                    job,
                    video_id: n.video_id,
                    file: n.file,
                    duration: n.duration,
                    finished,
                });
                self.shared.current_job.store(job, Ordering::Relaxed);
                self.shared.pending_pos_ms.store(0, Ordering::Relaxed);
                self.audible = true;
                self.recovered = false;
                self.maybe_prefetch();
            } else {
                // Queue moved under us: resynchronise.
                self.start_current(Duration::ZERO, false);
            }
            changed = true;
        }

        // A track started while paused (restored session) is never consumed, so treat buffered
        // audio as "ready" instead of showing Loading until the user presses play.
        let ready_paused =
            self.paused && !self.audible && self.current.is_some() && !self.buffered_empty();
        if ready_paused {
            self.audible = true;
            self.error = None;
            changed = true;
        }

        if !self.audible && self.current.as_ref().is_some_and(|c| c.job == job) && job != 0 {
            self.audible = true;
            self.consecutive_errors = 0;
            self.error = None;
            changed = true;
        }

        // Natural end without a gapless successor (end of queue, or prefetch missing).
        let ended = self.audible
            && !self.paused
            && self.shared.buf.is_empty()
            && self
                .current
                .as_ref()
                .is_some_and(|c| c.finished && c.job == job)
            && self.next.as_ref().is_none_or(|n| n.job.is_none());
        if ended {
            if self.queue.advance().is_some() {
                self.start_current(Duration::ZERO, false);
            } else {
                self.reset_playback();
                self.shared.pending_pos_ms.store(0, Ordering::Relaxed);
                self.publish();
            }
            return;
        }
        if changed {
            self.publish();
        }
    }

    // ---- state publication -----------------------------------------------------

    fn buffered_empty(&self) -> bool {
        self.shared.buf.buffered().is_zero()
    }

    fn status(&self) -> Status {
        if self.current.is_none() && !self.loading_current {
            Status::Idle
        } else if !self.audible {
            Status::Loading
        } else if self.paused {
            Status::Paused
        } else {
            Status::Playing
        }
    }

    fn publish(&mut self) {
        self.version += 1;
        if self.tracks_cache.len() != self.queue.len()
            || self
                .tracks_cache
                .iter()
                .zip(self.queue.tracks())
                .any(|(a, b)| a.video_id != b.video_id)
        {
            self.tracks_cache = Arc::new(self.queue.tracks().to_vec());
        }
        let state = PlayerState {
            status: self.status(),
            tracks: self.tracks_cache.clone(),
            current: self.queue.current_index(),
            upcoming: Arc::new(self.queue.upcoming()),
            duration: self.current.as_ref().and_then(|c| c.duration).or_else(|| {
                self.queue
                    .current()
                    .and_then(|t| t.duration_secs)
                    .map(|s| Duration::from_secs(u64::from(s)))
            }),
            volume: self.shared.buf.volume(),
            shuffle: self.queue.shuffle(),
            repeat: self.queue.repeat(),
            error: self.error.clone(),
            version: self.version,
        };
        *self.shared.state.write().unwrap() = state;
        let _ = self.shared.events.send(PlayerEvent::StateChanged);
        if let Some(n) = &self.notify {
            n();
        }
    }
}
