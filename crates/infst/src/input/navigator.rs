//! Song navigation algorithm.
//!
//! Navigates the INFINITAS song select screen by sending cursor keys and
//! monitoring the current song via memory reads until the target is reached.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use tracing::debug;

use crate::process::ReadMemory;

use super::keyboard::{GameKey, send_key_press};

/// Result of a navigation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationResult {
    /// Successfully navigated to the target song.
    Success { steps: u32 },
    /// Traversed the entire song list without finding the target.
    NotFound { steps: u32 },
    /// Exceeded the maximum number of steps.
    Timeout { steps: u32 },
    /// Navigation was cancelled via the shutdown signal.
    Cancelled { steps: u32 },
}

/// Song navigator that moves the cursor to a target song.
pub struct SongNavigator<'a, R: ReadMemory> {
    reader: &'a R,
    current_song_addr: u64,
    key_delay: Duration,
    settle_delay: Duration,
}

impl<'a, R: ReadMemory> SongNavigator<'a, R> {
    pub fn new(reader: &'a R, current_song_addr: u64) -> Self {
        Self {
            reader,
            current_song_addr,
            key_delay: Duration::from_millis(80),
            settle_delay: Duration::from_millis(50),
        }
    }

    pub fn with_key_delay(mut self, delay: Duration) -> Self {
        self.key_delay = delay;
        self
    }

    #[allow(dead_code)]
    pub fn with_settle_delay(mut self, delay: Duration) -> Self {
        self.settle_delay = delay;
        self
    }

    /// Read the song_id currently under the cursor.
    fn read_current_song_id(&self) -> Result<u32> {
        self.reader
            .read_i32(self.current_song_addr)
            .map(|v| v as u32)
            .map_err(|e| anyhow::anyhow!("Failed to read current song_id: {}", e))
    }

    /// Navigate to `target_song_id` by pressing Down repeatedly.
    ///
    /// The algorithm:
    /// 1. Read current song_id; if it matches target, return immediately.
    /// 2. Press Down, wait, read again.
    /// 3. If current == target → success.
    /// 4. If current == start song_id again → cycled through the entire list.
    /// 5. If steps > max_steps → timeout.
    pub fn navigate_to_song(
        &self,
        target_song_id: u32,
        max_steps: u32,
        shutdown: &AtomicBool,
    ) -> Result<NavigationResult> {
        let start_song_id = self.read_current_song_id()?;
        debug!(
            "Navigation start: current={}, target={}",
            start_song_id, target_song_id
        );

        if start_song_id == target_song_id {
            return Ok(NavigationResult::Success { steps: 0 });
        }

        let mut steps: u32 = 0;
        let mut seen_start_again = false;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(NavigationResult::Cancelled { steps });
            }

            if steps >= max_steps {
                return Ok(NavigationResult::Timeout { steps });
            }

            send_key_press(GameKey::Down, self.key_delay)?;
            std::thread::sleep(self.settle_delay);
            steps += 1;

            let current = self.read_current_song_id()?;
            debug!("Step {}: song_id={}", steps, current);

            if current == target_song_id {
                return Ok(NavigationResult::Success { steps });
            }

            // If we see the start song again, we've cycled the whole list
            if current == start_song_id {
                if seen_start_again {
                    // Second time seeing start → definitely not in the list
                    return Ok(NavigationResult::NotFound { steps });
                }
                seen_start_again = true;
            }
        }
    }

    /// Change difficulty by pressing Left/Right until the target difficulty
    /// value is read from `current_song_addr + 4`.
    pub fn select_difficulty(
        &self,
        target_difficulty: u8,
        max_attempts: u32,
        shutdown: &AtomicBool,
    ) -> Result<bool> {
        for _ in 0..max_attempts {
            if shutdown.load(Ordering::Relaxed) {
                return Ok(false);
            }

            let current = self
                .reader
                .read_i32(self.current_song_addr + 4)
                .map(|v| v as u8)?;

            if current == target_difficulty {
                return Ok(true);
            }

            // Press Right to cycle difficulty
            send_key_press(GameKey::Right, self.key_delay)?;
            std::thread::sleep(self.settle_delay);
        }
        Ok(false)
    }

    /// Confirm the current selection by pressing Enter.
    pub fn confirm_selection(&self) -> Result<()> {
        send_key_press(GameKey::Enter, self.key_delay)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockMemoryBuilder;

    fn make_linear_song_list(song_ids: &[u32]) -> (crate::process::MockMemoryReader, u64) {
        // Layout: each "slot" is 4 bytes (song_id as i32) at sequential addresses.
        // We'll put the "cursor position" at the base; the test simulates moving
        // by mutating MockMemoryReader data directly — but since MockMemoryReader
        // is immutable we instead test the stateless read path.
        let base = 0x1000u64;
        let mut builder = MockMemoryBuilder::new().base(base);
        // Write first song_id at base (this represents the current_song address)
        if let Some(&first) = song_ids.first() {
            builder = builder.write_i32(0, first as i32);
        }
        (builder.build(), base)
    }

    #[test]
    fn already_on_target() {
        let (reader, base) = make_linear_song_list(&[1001]);
        let nav = SongNavigator::new(&reader, base);
        let shutdown = AtomicBool::new(false);

        let result = nav.navigate_to_song(1001, 100, &shutdown).unwrap();
        assert_eq!(result, NavigationResult::Success { steps: 0 });
    }

    #[test]
    fn cancelled_immediately() {
        let (reader, base) = make_linear_song_list(&[1001]);
        let nav = SongNavigator::new(&reader, base);
        let shutdown = AtomicBool::new(true);

        let result = nav.navigate_to_song(9999, 100, &shutdown).unwrap();
        assert!(matches!(result, NavigationResult::Cancelled { .. }));
    }

    #[test]
    fn navigation_result_debug() {
        let r = NavigationResult::Success { steps: 42 };
        assert!(format!("{:?}", r).contains("42"));
    }
}
