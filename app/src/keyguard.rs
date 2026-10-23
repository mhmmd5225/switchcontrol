//! Makes sure the keys we release on this machine really go up.
//!
//! Windows drops injected input without reporting it while an elevated
//! window (e.g. Task Manager) is in front, so a key-up we inject for the peer
//! can be lost and leave that key stuck down until something releases it.
//! Every key we inject up is therefore checked against the keys the OS
//! still considers down, and released again a few times if needed. Only keys
//! we released ourselves are touched, never keys pressed on this keyboard.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// Releases tried again before giving up (the key may then be held on this
/// machine's own keyboard).
pub const MAX_RETRIES: u8 = 5;
/// Time the OS gets to process a key-up before we look at its key state.
pub const SETTLE: Duration = Duration::from_millis(200);

#[derive(Debug, Default)]
pub struct KeyGuard {
    /// Usage -> when it was last released, and how often it was retried.
    pending: BTreeMap<u16, (Instant, u8)>,
}

impl KeyGuard {
    /// Records that we injected `usage` going down or up.
    pub fn injected(&mut self, usage: u16, down: bool, now: Instant) {
        if down {
            self.pending.remove(&usage);
        } else {
            self.pending.insert(usage, (now, 0));
        }
    }

    pub fn is_idle(&self) -> bool {
        self.pending.is_empty()
    }

    /// Given the keys the OS considers down, returns those to release again.
    pub fn check(&mut self, down: &[u16], now: Instant) -> Vec<u16> {
        let mut again = Vec::new();
        self.pending.retain(|&usage, (since, tries)| {
            if now.saturating_duration_since(*since) < SETTLE {
                return true;
            }
            if !down.contains(&usage) || *tries >= MAX_RETRIES {
                return false;
            }
            *tries += 1;
            *since = now;
            again.push(usage);
            true
        });
        again
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: u16 = 0xE1;
    const A: u16 = 0x04;

    #[test]
    fn released_keys_that_went_up_are_forgotten() {
        let t0 = Instant::now();
        let mut guard = KeyGuard::default();
        guard.injected(SHIFT, false, t0);
        assert!(guard.check(&[], t0 + SETTLE).is_empty());
        assert!(guard.is_idle());
    }

    #[test]
    fn stuck_keys_are_released_again_until_they_go_up() {
        let t0 = Instant::now();
        let mut guard = KeyGuard::default();
        guard.injected(SHIFT, false, t0);
        // Too early: the OS may not have processed the key-up yet.
        assert!(guard.check(&[SHIFT], t0).is_empty());
        assert_eq!(guard.check(&[SHIFT], t0 + SETTLE), vec![SHIFT]);
        assert_eq!(guard.check(&[SHIFT], t0 + SETTLE * 2), vec![SHIFT]);
        assert!(guard.check(&[], t0 + SETTLE * 3).is_empty());
        assert!(guard.is_idle());
    }

    #[test]
    fn gives_up_after_a_few_tries() {
        let t0 = Instant::now();
        let mut guard = KeyGuard::default();
        guard.injected(SHIFT, false, t0);
        let tries = (1..=10u32).filter(|&i| !guard.check(&[SHIFT], t0 + SETTLE * i).is_empty());
        assert_eq!(tries.count(), MAX_RETRIES as usize);
        assert!(guard.is_idle());
    }

    #[test]
    fn pressing_again_cancels_the_check() {
        let t0 = Instant::now();
        let mut guard = KeyGuard::default();
        guard.injected(A, false, t0);
        guard.injected(A, true, t0);
        assert!(guard.check(&[A], t0 + SETTLE).is_empty());
        assert!(guard.is_idle());
    }
}
