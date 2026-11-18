//! How many downloads, and how many unpacks, run at the same time (Settings → Downloads).
//!
//! Each is a counter with a limit the player can change while things run: a lower limit lets the
//! ones already running finish and holds the next ones back; a higher one lets waiting ones start
//! on their next look. A limit of 0 means no limit.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// Downloads of games and updates (and Data Hoarder Mode's saves). One at a time unless the player
/// picks more, which is how the launcher has always worked.
pub static DOWNLOADS: Slots = Slots::new(1);
/// Unpacking a finished download into its game folder (or staging an installer). No limit unless
/// the player picks one: unpacking is mostly the drive's work, and a slow drive is the reason to.
pub static UNPACKS: Slots = Slots::new(0);

/// How often a waiting one looks for a free slot, and for its cancel.
const LOOK_EVERY: Duration = Duration::from_millis(300);
/// The highest limit the Settings page offers; anything above it is treated as that.
pub const MOST: usize = 16;

pub struct Slots {
    limit: AtomicUsize,
    busy: Mutex<usize>,
}

/// A slot in use; it's freed when this is dropped.
pub struct Slot(&'static Slots);

impl Drop for Slot {
    fn drop(&mut self) {
        let mut busy = self.0.busy.lock().unwrap_or_else(|e| e.into_inner());
        *busy = busy.saturating_sub(1);
    }
}

impl Slots {
    pub const fn new(limit: usize) -> Self {
        Slots { limit: AtomicUsize::new(limit), busy: Mutex::new(0) }
    }

    pub fn set_limit(&self, limit: usize) {
        self.limit.store(limit.min(MOST), Ordering::Relaxed);
    }

    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Relaxed)
    }

    /// A slot if one is free right now.
    pub fn try_take(&'static self) -> Option<Slot> {
        let limit = self.limit();
        let mut busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        if limit != 0 && *busy >= limit {
            return None;
        }
        *busy += 1;
        Some(Slot(self))
    }

    /// Waits for a free slot. None once `cancel` is set, so cancelling (or pausing) a waiting
    /// download answers at once, not when its turn comes.
    pub async fn take(&'static self, cancel: &AtomicBool) -> Option<Slot> {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(slot) = self.try_take() {
                return Some(slot);
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    }

    #[cfg(test)]
    fn busy(&self) -> usize {
        *self.busy.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_of_one_lets_one_in_at_a_time() {
        static S: Slots = Slots::new(1);
        let first = S.try_take();
        assert!(first.is_some());
        assert!(S.try_take().is_none());
        drop(first);
        assert!(S.try_take().is_some());
    }

    #[test]
    fn no_limit_lets_everyone_in() {
        static S: Slots = Slots::new(0);
        let held: Vec<_> = (0..40).map(|_| S.try_take()).collect();
        assert!(held.iter().all(|s| s.is_some()));
        assert_eq!(S.busy(), 40);
    }

    #[test]
    fn a_lower_limit_holds_the_next_ones_back_until_enough_finish() {
        static S: Slots = Slots::new(3);
        let a = S.try_take().unwrap();
        let b = S.try_take().unwrap();
        S.set_limit(1);
        assert!(S.try_take().is_none());
        drop(a);
        assert!(S.try_take().is_none());
        drop(b);
        assert!(S.try_take().is_some());
    }

    #[test]
    fn limits_above_the_most_offered_are_capped() {
        static S: Slots = Slots::new(1);
        S.set_limit(1000);
        assert_eq!(S.limit(), MOST);
    }

    #[test]
    fn a_cancelled_wait_gives_up() {
        static S: Slots = Slots::new(1);
        let _held = S.try_take().unwrap();
        let cancel = AtomicBool::new(true);
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        assert!(rt.block_on(S.take(&cancel)).is_none());
    }
}
