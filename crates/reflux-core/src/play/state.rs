use crate::play::GameState;

/// Game state detector
pub struct GameStateDetector {
    last_state: GameState,
}

impl GameStateDetector {
    pub fn new() -> Self {
        Self {
            last_state: GameState::Unknown,
        }
    }

    /// Determine game state from memory values
    ///
    /// Based on the original C# implementation:
    /// - Check marker at JudgeData + word * 54
    /// - If marker != 0, check next position to confirm playing
    /// - Check PlaySettings - word * 6 for song select marker
    pub fn detect(
        &mut self,
        judge_marker_54: i32,
        judge_marker_55: i32,
        song_select_marker: i32,
    ) -> GameState {
        let detected_state = self.detect_raw(
            judge_marker_54,
            judge_marker_55,
            song_select_marker,
            self.last_state,
        );

        self.last_state = detected_state;
        detected_state
    }

    /// Detect state from raw memory values without transition validation
    ///
    /// Based on C# implementation:
    /// 1. marker54 != 0 → Playing
    /// 2. last_state == SongSelect → stay in SongSelect (can't go directly to ResultScreen)
    /// 3. song_select_marker == 1 → SongSelect
    /// 4. Otherwise → ResultScreen
    fn detect_raw(
        &self,
        judge_marker_54: i32,
        _judge_marker_55: i32,
        song_select_marker: i32,
        last_state: GameState,
    ) -> GameState {
        // Check if playing (marker1 must be non-zero)
        // Note: marker2 check removed as it may be at a different offset
        // in newer game versions (confirmed marker1=1, marker2=0 during play)
        if judge_marker_54 != 0 {
            return GameState::Playing;
        }

        // "Cannot go from song select to result screen anyway" (C# implementation)
        // Maintain SongSelect during intermediate transitions
        if last_state == GameState::SongSelect {
            return GameState::SongSelect;
        }

        // Check if in song select
        if song_select_marker == 1 {
            return GameState::SongSelect;
        }

        // Otherwise it's ResultScreen (matches C# behavior)
        // This allows detecting ResultScreen even when starting from Unknown state
        GameState::ResultScreen
    }

    /// Reset state (e.g., when reconnecting to process)
    pub fn reset(&mut self) {
        self.last_state = GameState::Unknown;
    }

    pub fn last_state(&self) -> GameState {
        self.last_state
    }
}

impl Default for GameStateDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_playing() {
        let mut detector = GameStateDetector::new();
        // Both markers non-zero means playing
        let state = detector.detect(1, 1, 0);
        assert_eq!(state, GameState::Playing);
    }

    #[test]
    fn test_detect_song_select() {
        let mut detector = GameStateDetector::new();
        // song_select_marker == 1 means song select
        let state = detector.detect(0, 0, 1);
        assert_eq!(state, GameState::SongSelect);
    }

    #[test]
    fn test_detect_result_screen_when_idle() {
        let mut detector = GameStateDetector::new();
        // When starting from Unknown with all markers zero, it's ResultScreen
        // (matches C# behavior where it falls through to resultScreen)
        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::ResultScreen);
    }

    #[test]
    fn test_detect_result_screen() {
        let mut detector = GameStateDetector::new();
        // First go to Playing
        detector.detect(1, 1, 0);
        // Then result screen (not playing, not song select)
        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::ResultScreen);
    }

    #[test]
    fn test_song_select_intermediate_state() {
        let mut detector = GameStateDetector::new();
        // Go to SongSelect
        detector.detect(0, 0, 1);
        assert_eq!(detector.last_state(), GameState::SongSelect);

        // Intermediate state during transition (both markers = 0)
        // Should stay in SongSelect without warning
        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::SongSelect);

        // Then transition to Playing
        let state = detector.detect(1, 1, 0);
        assert_eq!(state, GameState::Playing);
    }

    #[test]
    fn test_reset() {
        let mut detector = GameStateDetector::new();
        detector.detect(0, 0, 1);
        assert_eq!(detector.last_state(), GameState::SongSelect);

        detector.reset();
        assert_eq!(detector.last_state(), GameState::Unknown);
    }

    #[test]
    fn test_only_marker1_nonzero_is_playing() {
        let mut detector = GameStateDetector::new();
        // Only marker1 non-zero IS Playing (marker2 check removed for newer game versions)
        let state = detector.detect(1, 0, 0);
        assert_eq!(state, GameState::Playing);
    }

    #[test]
    fn test_only_marker2_nonzero_is_result_screen() {
        let mut detector = GameStateDetector::new();
        // Only marker2 non-zero with marker1=0 falls through to ResultScreen
        let state = detector.detect(0, 1, 0);
        assert_eq!(state, GameState::ResultScreen);
    }

    #[test]
    fn test_all_zero_from_unknown_is_result_screen() {
        let mut detector = GameStateDetector::new();
        // All markers zero from Unknown should be ResultScreen (matches C# behavior)
        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::ResultScreen);
    }

    #[test]
    fn test_playing_to_result_to_song_select() {
        let mut detector = GameStateDetector::new();
        // Full cycle: Unknown -> Playing -> ResultScreen -> SongSelect
        let state = detector.detect(1, 1, 0);
        assert_eq!(state, GameState::Playing);

        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::ResultScreen);

        let state = detector.detect(0, 0, 1);
        assert_eq!(state, GameState::SongSelect);
    }

    #[test]
    fn test_song_select_to_playing_with_marker1_only() {
        let mut detector = GameStateDetector::new();
        // Go to SongSelect
        detector.detect(0, 0, 1);
        assert_eq!(detector.last_state(), GameState::SongSelect);

        // marker1 only should transition to Playing (marker2 check removed)
        let state = detector.detect(1, 0, 0);
        assert_eq!(state, GameState::Playing);
    }
}
