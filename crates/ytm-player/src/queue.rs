//! Play queue with shuffle and repeat. Pure logic: no audio, no I/O.
//!
//! `tracks` keeps the user-visible order; `order` is the play order (a
//! permutation of track indices) and `pos` points into it.

use ytm_api::Track;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// The removed track was the current one; `current()` now points at its successor (if any).
    CurrentRemoved,
    Other,
    OutOfRange,
}

#[derive(Debug, Default)]
pub struct Queue {
    tracks: Vec<Track>,
    order: Vec<usize>,
    pos: Option<usize>,
    shuffle: bool,
    repeat: Repeat,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    /// Index into `tracks()` of the current track.
    pub fn current_index(&self) -> Option<usize> {
        self.pos.and_then(|p| self.order.get(p).copied())
    }

    pub fn current(&self) -> Option<&Track> {
        self.current_index().map(|i| &self.tracks[i])
    }

    /// Track indices that will play after the current one, in play order (no repeat wrap).
    pub fn upcoming(&self) -> Vec<usize> {
        match self.pos {
            Some(p) => self.order[p + 1..].to_vec(),
            None => Vec::new(),
        }
    }

    pub fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// Replaces the queue and starts at `start` (clamped).
    pub fn set(&mut self, tracks: Vec<Track>, start: usize) {
        self.tracks = tracks;
        if self.tracks.is_empty() {
            self.order.clear();
            self.pos = None;
            return;
        }
        let start = start.min(self.tracks.len() - 1);
        self.rebuild_order(start);
    }

    fn rebuild_order(&mut self, current: usize) {
        if self.shuffle {
            let mut rest: Vec<usize> = (0..self.tracks.len()).filter(|&i| i != current).collect();
            fastrand::shuffle(&mut rest);
            self.order = std::iter::once(current).chain(rest).collect();
            self.pos = Some(0);
        } else {
            self.order = (0..self.tracks.len()).collect();
            self.pos = Some(current);
        }
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if self.shuffle == on {
            return;
        }
        self.shuffle = on;
        if let Some(cur) = self.current_index() {
            self.rebuild_order(cur);
        }
    }

    /// Appends to the end of the queue (and of the play order).
    pub fn push(&mut self, track: Track) {
        self.tracks.push(track);
        self.order.push(self.tracks.len() - 1);
        if self.pos.is_none() {
            self.pos = Some(0);
        }
    }

    /// Inserts so the track plays right after the current one.
    pub fn push_next(&mut self, track: Track) {
        self.tracks.push(track);
        let idx = self.tracks.len() - 1;
        match self.pos {
            Some(p) => self.order.insert(p + 1, idx),
            None => {
                self.order.push(idx);
                self.pos = Some(0);
            }
        }
    }

    pub fn remove(&mut self, index: usize) -> RemoveOutcome {
        if index >= self.tracks.len() {
            return RemoveOutcome::OutOfRange;
        }
        let removed_pos = self
            .order
            .iter()
            .position(|&i| i == index)
            .expect("order is a permutation");
        self.tracks.remove(index);
        self.order.remove(removed_pos);
        for i in &mut self.order {
            if *i > index {
                *i -= 1;
            }
        }
        let outcome = match self.pos {
            Some(p) if removed_pos < p => {
                self.pos = Some(p - 1);
                RemoveOutcome::Other
            }
            Some(p) if removed_pos == p => RemoveOutcome::CurrentRemoved,
            _ => RemoveOutcome::Other,
        };
        if self.order.is_empty() {
            self.pos = None;
        } else if let Some(p) = self.pos {
            if p >= self.order.len() {
                // Removed the last item while it was current.
                self.pos = if self.repeat == Repeat::All {
                    Some(0)
                } else {
                    None
                };
            }
        }
        outcome
    }

    pub fn clear(&mut self) {
        *self = Self {
            shuffle: self.shuffle,
            repeat: self.repeat,
            ..Self::default()
        };
    }

    /// Makes `track_index` current (e.g. user clicked it in the queue view).
    pub fn jump_to(&mut self, track_index: usize) -> bool {
        match self.order.iter().position(|&i| i == track_index) {
            Some(p) => {
                self.pos = Some(p);
                true
            }
            None => false,
        }
    }

    /// Track index that follows naturally when the current track ends.
    pub fn peek_advance(&self) -> Option<usize> {
        let p = self.pos?;
        if self.repeat == Repeat::One {
            return self.order.get(p).copied();
        }
        self.peek_skip_from(p)
    }

    fn peek_skip_from(&self, p: usize) -> Option<usize> {
        match self.order.get(p + 1) {
            Some(&i) => Some(i),
            None if self.repeat == Repeat::All && !self.order.is_empty() => Some(self.order[0]),
            None => None,
        }
    }

    /// Moves on after the current track finished on its own. `None` = end of queue.
    pub fn advance(&mut self) -> Option<&Track> {
        let p = self.pos?;
        if self.repeat != Repeat::One {
            self.pos = if p + 1 < self.order.len() {
                Some(p + 1)
            } else if self.repeat == Repeat::All {
                Some(0)
            } else {
                // Stay on the last track; caller stops playback.
                return None;
            };
        }
        self.current()
    }

    /// User pressed "next": like `advance` but ignores `Repeat::One`.
    pub fn skip_next(&mut self) -> Option<&Track> {
        let p = self.pos?;
        self.pos = if p + 1 < self.order.len() {
            Some(p + 1)
        } else if self.repeat == Repeat::All {
            Some(0)
        } else {
            return None;
        };
        self.current()
    }

    /// User pressed "previous". At the start of the queue this wraps only with `Repeat::All`;
    /// otherwise it stays put (the caller restarts the track).
    pub fn skip_prev(&mut self) -> Option<&Track> {
        let p = self.pos?;
        self.pos = if p > 0 {
            Some(p - 1)
        } else if self.repeat == Repeat::All {
            Some(self.order.len() - 1)
        } else {
            Some(0)
        };
        self.current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: usize) -> Track {
        Track {
            video_id: format!("{n:011}"),
            title: format!("t{n}"),
            ..Track::default()
        }
    }

    fn queue(n: usize) -> Queue {
        let mut q = Queue::new();
        q.set((0..n).map(t).collect(), 0);
        q
    }

    fn title(q: &Queue) -> &str {
        &q.current().unwrap().title
    }

    #[test]
    fn plays_through_and_stops() {
        let mut q = queue(3);
        assert_eq!(title(&q), "t0");
        assert_eq!(q.peek_advance(), Some(1));
        assert_eq!(q.advance().unwrap().title, "t1");
        assert_eq!(q.advance().unwrap().title, "t2");
        assert_eq!(q.peek_advance(), None);
        assert!(q.advance().is_none());
        assert_eq!(title(&q), "t2", "stays on last track");
    }

    #[test]
    fn repeat_all_wraps() {
        let mut q = queue(2);
        q.set_repeat(Repeat::All);
        q.advance();
        assert_eq!(q.peek_advance(), Some(0));
        assert_eq!(q.advance().unwrap().title, "t0");
    }

    #[test]
    fn repeat_one_replays_but_skip_moves_on() {
        let mut q = queue(3);
        q.set_repeat(Repeat::One);
        assert_eq!(q.peek_advance(), Some(0));
        assert_eq!(q.advance().unwrap().title, "t0");
        assert_eq!(q.skip_next().unwrap().title, "t1");
    }

    #[test]
    fn skip_next_at_end() {
        let mut q = queue(2);
        q.skip_next();
        assert!(q.skip_next().is_none());
        q.set_repeat(Repeat::All);
        assert_eq!(q.skip_next().unwrap().title, "t0");
    }

    #[test]
    fn skip_prev_clamps_at_start() {
        let mut q = queue(3);
        assert_eq!(q.skip_prev().unwrap().title, "t0");
        q.jump_to(2);
        assert_eq!(q.skip_prev().unwrap().title, "t1");
        q.jump_to(0);
        q.set_repeat(Repeat::All);
        assert_eq!(q.skip_prev().unwrap().title, "t2");
    }

    #[test]
    fn shuffle_keeps_current_first_and_visits_all_once() {
        let mut q = queue(20);
        q.jump_to(7);
        q.set_shuffle(true);
        assert_eq!(title(&q), "t7");
        let mut seen = vec![q.current_index().unwrap()];
        while let Some(t) = q.advance() {
            seen.push(t.video_id.parse::<usize>().unwrap());
        }
        seen.sort_unstable();
        assert_eq!(seen, (0..20).collect::<Vec<_>>());
    }

    #[test]
    fn unshuffle_continues_from_current_in_original_order() {
        let mut q = queue(10);
        q.set_shuffle(true);
        q.advance();
        let cur = q.current_index().unwrap();
        q.set_shuffle(false);
        assert_eq!(q.current_index(), Some(cur));
        if cur + 1 < 10 {
            assert_eq!(q.peek_advance(), Some(cur + 1));
        }
    }

    #[test]
    fn push_next_inserts_after_current_even_when_shuffled() {
        let mut q = queue(5);
        q.set_shuffle(true);
        q.push_next(t(99));
        assert_eq!(q.peek_advance(), Some(5));
        let mut q = queue(3);
        q.push_next(t(99));
        assert_eq!(q.advance().unwrap().title, "t99");
        assert_eq!(q.advance().unwrap().title, "t1");
    }

    #[test]
    fn push_appends_last() {
        let mut q = queue(2);
        q.push(t(9));
        assert_eq!(q.upcoming(), vec![1, 2]);
        let mut empty = Queue::new();
        empty.push(t(1));
        assert_eq!(title(&empty), "t1");
    }

    #[test]
    fn remove_before_current_keeps_current() {
        let mut q = queue(4);
        q.jump_to(2);
        assert_eq!(q.remove(0), RemoveOutcome::Other);
        assert_eq!(title(&q), "t2");
        assert_eq!(q.current_index(), Some(1));
    }

    #[test]
    fn remove_current_moves_to_successor() {
        let mut q = queue(3);
        q.jump_to(1);
        assert_eq!(q.remove(1), RemoveOutcome::CurrentRemoved);
        assert_eq!(title(&q), "t2");
    }

    #[test]
    fn remove_last_current_ends_queue() {
        let mut q = queue(2);
        q.jump_to(1);
        assert_eq!(q.remove(1), RemoveOutcome::CurrentRemoved);
        assert!(q.current().is_none());
        assert_eq!(q.remove(5), RemoveOutcome::OutOfRange);
    }

    #[test]
    fn remove_everything_then_reuse() {
        let mut q = queue(1);
        q.remove(0);
        assert!(q.is_empty() && q.current().is_none());
        q.push(t(3));
        assert_eq!(title(&q), "t3");
    }

    #[test]
    fn clear_keeps_modes() {
        let mut q = queue(3);
        q.set_shuffle(true);
        q.set_repeat(Repeat::All);
        q.clear();
        assert!(q.is_empty() && q.shuffle() && q.repeat() == Repeat::All);
    }

    #[test]
    fn set_clamps_start_and_handles_empty() {
        let mut q = Queue::new();
        q.set(vec![t(0), t(1)], 99);
        assert_eq!(title(&q), "t1");
        q.set(vec![], 0);
        assert!(q.current().is_none() && q.peek_advance().is_none());
    }
}
