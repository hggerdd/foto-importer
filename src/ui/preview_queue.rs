//! Warteschlange der Vorschau-Arbeiter mit zwei Bahnen: Das Foto unter der Maus (höchstens
//! eines, ein neues ersetzt das alte) geht allen Kacheln vor; Kacheln, die die Oberfläche
//! einige Frames nicht mehr gezeichnet hat (weggescrollt, andere Ansicht), fallen heraus.
//! Ein reservierter Arbeiter nimmt nur Mausaufträge, damit das Foto nie hinter einer gerade
//! laufenden Kachelrunde wartet.

use std::sync::{Condvar, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Priority {
    Tile,
    Hover,
}

struct Job<T> {
    key: String,
    payload: T,
    /// Letzter Frame, in dem die Oberfläche das Ergebnis noch wollte.
    wanted: u64,
    seq: u64,
}

struct State<T> {
    hover: Option<Job<T>>,
    tiles: Vec<Job<T>>,
    frame: u64,
    seq: u64,
    dropped: Vec<String>,
    closed: bool,
}

pub struct PreviewQueue<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
    capacity: usize,
    stale_frames: u64,
}

impl<T> PreviewQueue<T> {
    /// `capacity`: höchstens so viele wartende Kacheln (kurz, damit sichtbare Kacheln vor
    /// längst weggescrollten kommen); `stale_frames`: so viele Frames ohne Zeichnen, dann
    /// fällt ein wartender Auftrag heraus.
    pub fn new(capacity: usize, stale_frames: u64) -> Self {
        Self {
            state: Mutex::new(State {
                hover: None,
                tiles: Vec::new(),
                frame: 0,
                seq: 0,
                dropped: Vec::new(),
                closed: false,
            }),
            ready: Condvar::new(),
            capacity: capacity.max(1),
            stale_frames,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reiht einen Auftrag ein; `false`, wenn die Kachelbahn voll ist (die Oberfläche fragt im
    /// nächsten Frame erneut). Ein Mausauftrag gelingt immer: Er ersetzt den wartenden
    /// Mausauftrag und holt eine gleiche wartende Kachel nach vorn.
    pub fn push(&self, key: String, payload: T, priority: Priority) -> bool {
        let mut state = self.lock();
        if state.closed {
            return false;
        }
        state.seq += 1;
        let job = Job {
            key,
            payload,
            wanted: state.frame,
            seq: state.seq,
        };
        match priority {
            Priority::Hover => {
                state.tiles.retain(|tile| tile.key != job.key);
                if let Some(old) = state.hover.replace(job)
                    && state.hover.as_ref().is_some_and(|new| new.key != old.key)
                {
                    state.dropped.push(old.key);
                }
                drop(state);
                self.ready.notify_all();
                true
            }
            Priority::Tile => {
                if state.tiles.len() >= self.capacity {
                    return false;
                }
                state.tiles.push(job);
                drop(state);
                self.ready.notify_one();
                true
            }
        }
    }

    /// Holt eine wartende Kachel in die Mausbahn; `false`, wenn sie nicht (mehr) wartet,
    /// etwa weil ein Arbeiter sie schon dekodiert.
    pub fn promote(&self, key: &str) -> bool {
        let mut state = self.lock();
        let Some(index) = state.tiles.iter().position(|tile| tile.key == key) else {
            return state.hover.as_ref().is_some_and(|hover| hover.key == key);
        };
        let mut job = state.tiles.remove(index);
        job.wanted = state.frame;
        if let Some(old) = state.hover.replace(job) {
            state.dropped.push(old.key);
        }
        drop(state);
        self.ready.notify_all();
        true
    }

    /// Die Oberfläche will das Ergebnis weiterhin (im aktuellen Frame gezeichnet).
    pub fn touch(&self, key: &str) {
        let mut state = self.lock();
        let frame = state.frame;
        if let Some(hover) = state.hover.as_mut().filter(|hover| hover.key == key) {
            hover.wanted = frame;
            return;
        }
        if let Some(tile) = state.tiles.iter_mut().find(|tile| tile.key == key) {
            tile.wanted = frame;
        }
    }

    /// Neuer Frame: Aufträge, die seit `stale_frames` Frames niemand gezeichnet hat, fallen
    /// heraus (und erscheinen in [`Self::take_dropped`]).
    pub fn set_frame(&self, frame: u64) {
        let mut state = self.lock();
        state.frame = frame;
        let stale = |job: &Job<T>| job.wanted + self.stale_frames < frame;
        if state.hover.as_ref().is_some_and(stale)
            && let Some(old) = state.hover.take()
        {
            state.dropped.push(old.key);
        }
        let (keep, gone): (Vec<_>, Vec<_>) = std::mem::take(&mut state.tiles)
            .into_iter()
            .partition(|job| !stale(job));
        state.tiles = keep;
        state.dropped.extend(gone.into_iter().map(|job| job.key));
    }

    /// Schlüssel verworfener Aufträge seit dem letzten Aufruf.
    pub fn take_dropped(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().dropped)
    }

    /// Leert beide Bahnen ohne Meldung (neue Sammlung, Oberfläche vergisst alles).
    pub fn clear(&self) {
        let mut state = self.lock();
        state.hover = None;
        state.tiles.clear();
        state.dropped.clear();
    }

    pub fn close(&self) {
        self.lock().closed = true;
        self.ready.notify_all();
    }

    fn take(state: &mut State<T>, hover_only: bool) -> Option<(String, T, Priority)> {
        if let Some(job) = state.hover.take() {
            return Some((job.key, job.payload, Priority::Hover));
        }
        if hover_only {
            return None;
        }
        // Zuletzt gezeichnete zuerst, bei Gleichstand in Anfragereihenfolge.
        let index = state
            .tiles
            .iter()
            .enumerate()
            .max_by_key(|(_, job)| (job.wanted, std::cmp::Reverse(job.seq)))
            .map(|(index, _)| index)?;
        let job = state.tiles.remove(index);
        Some((job.key, job.payload, Priority::Tile))
    }

    /// Nächster Auftrag ohne zu warten (Tests).
    #[cfg(test)]
    pub fn try_pop(&self, hover_only: bool) -> Option<(String, T, Priority)> {
        Self::take(&mut self.lock(), hover_only)
    }

    /// Wartet auf den nächsten Auftrag; `None` nach [`Self::close`]. `hover_only` für den
    /// reservierten Mausarbeiter.
    pub fn pop(&self, hover_only: bool) -> Option<(String, T, Priority)> {
        let mut state = self.lock();
        loop {
            if state.closed {
                return None;
            }
            if let Some(job) = Self::take(&mut state, hover_only) {
                return Some(job);
            }
            state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn keys(queue: &PreviewQueue<u32>, hover_only: bool) -> Vec<(String, Priority)> {
        std::iter::from_fn(|| queue.try_pop(hover_only))
            .map(|(key, _, priority)| (key, priority))
            .collect()
    }

    #[test]
    fn hover_goes_before_queued_tiles() {
        let queue = PreviewQueue::new(8, 2);
        for key in ["a", "b", "c"] {
            assert!(queue.push(key.into(), 0, Priority::Tile));
        }
        assert!(queue.push("foto".into(), 0, Priority::Hover));
        let order = keys(&queue, false);
        assert_eq!(order[0], ("foto".to_string(), Priority::Hover));
        assert_eq!(
            order[1..]
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c"],
            "Kacheln danach in Anfragereihenfolge"
        );
    }

    #[test]
    fn hover_ignores_a_full_tile_lane_and_reserved_worker_takes_only_hover() {
        let queue = PreviewQueue::new(2, 2);
        assert!(queue.push("a".into(), 0, Priority::Tile));
        assert!(queue.push("b".into(), 0, Priority::Tile));
        assert!(
            !queue.push("c".into(), 0, Priority::Tile),
            "Kachelbahn voll"
        );
        assert!(queue.push("foto".into(), 0, Priority::Hover));
        assert_eq!(keys(&queue, true), [("foto".to_string(), Priority::Hover)]);
        assert!(
            queue.try_pop(true).is_none(),
            "reservierter Arbeiter nimmt keine Kacheln"
        );
        assert_eq!(keys(&queue, false).len(), 2);
    }

    #[test]
    fn newer_hover_supersedes_the_waiting_one() {
        let queue = PreviewQueue::new(4, 2);
        assert!(queue.push("eins".into(), 1, Priority::Hover));
        assert!(queue.push("zwei".into(), 2, Priority::Hover));
        assert!(queue.push("drei".into(), 3, Priority::Hover));
        assert_eq!(queue.take_dropped(), ["eins", "zwei"]);
        let (key, payload, priority) = queue.try_pop(false).expect("Auftrag");
        assert_eq!(
            (key.as_str(), payload, priority),
            ("drei", 3, Priority::Hover)
        );
        assert!(queue.try_pop(false).is_none());
    }

    #[test]
    fn hover_no_longer_drawn_is_dropped_and_tiles_continue() {
        let queue = PreviewQueue::new(4, 2);
        queue.set_frame(10);
        assert!(queue.push("a".into(), 0, Priority::Tile));
        assert!(queue.push("foto".into(), 0, Priority::Hover));
        // Die Maus ist weiter: das Foto wird nicht mehr gezeichnet, die Kachel schon.
        for frame in 11..=13 {
            queue.set_frame(frame);
            queue.touch("a");
        }
        assert_eq!(queue.take_dropped(), ["foto"]);
        assert_eq!(keys(&queue, false), [("a".to_string(), Priority::Tile)]);
    }

    #[test]
    fn offscreen_tiles_are_deprioritized_then_dropped() {
        let queue = PreviewQueue::new(8, 2);
        queue.set_frame(1);
        assert!(queue.push("weg".into(), 0, Priority::Tile));
        assert!(queue.push("sichtbar".into(), 0, Priority::Tile));
        queue.set_frame(2);
        queue.touch("sichtbar");
        assert_eq!(
            queue.try_pop(false).expect("Auftrag").0,
            "sichtbar",
            "zuletzt gezeichnet zuerst"
        );
        assert!(queue.push("sichtbar2".into(), 0, Priority::Tile));
        queue.set_frame(4);
        assert_eq!(queue.take_dropped(), ["weg"]);
        assert_eq!(
            keys(&queue, false),
            [("sichtbar2".to_string(), Priority::Tile)]
        );
    }

    #[test]
    fn promote_moves_a_waiting_tile_to_the_front() {
        let queue = PreviewQueue::new(4, 2);
        assert!(queue.push("a".into(), 0, Priority::Tile));
        assert!(queue.push("b".into(), 0, Priority::Tile));
        assert!(queue.promote("b"));
        assert!(!queue.promote("läuft schon"));
        assert_eq!(keys(&queue, true), [("b".to_string(), Priority::Hover)]);
        assert_eq!(keys(&queue, false), [("a".to_string(), Priority::Tile)]);
    }

    #[test]
    fn tiles_are_not_starved_after_hover_ends() {
        let queue = Arc::new(PreviewQueue::new(64, 1000));
        for i in 0..20 {
            assert!(queue.push(format!("kachel{i}"), i, Priority::Tile));
        }
        for i in 0..5 {
            assert!(queue.push(format!("foto{i}"), 100 + i, Priority::Hover));
        }
        let worker = {
            let queue = queue.clone();
            std::thread::spawn(move || {
                let mut seen = Vec::new();
                while let Some((key, _, priority)) = queue.pop(false) {
                    seen.push((key, priority));
                    if seen.len() == 21 {
                        break;
                    }
                }
                seen
            })
        };
        let seen = worker.join().expect("Arbeiter");
        assert_eq!(seen[0], ("foto4".to_string(), Priority::Hover));
        assert_eq!(
            seen.iter().filter(|(_, p)| *p == Priority::Tile).count(),
            20,
            "alle Kacheln laufen nach der Maus weiter"
        );
        queue.close();
        assert!(queue.pop(false).is_none());
    }
}
