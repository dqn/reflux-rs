use tracing::warn;

use crate::game::GameState;

/// Game state detector
///
/// ## State Transition Rules
///
/// Valid transitions:
/// - Unknown -> SongSelect | Playing (initial detection)
/// - SongSelect -> Playing (song start)
/// - Playing -> ResultScreen (song end)
/// - ResultScreen -> SongSelect (back to select)
///
/// Invalid transitions (blocked):
/// - SongSelect -> ResultScreen (must go through Playing)
/// - ResultScreen -> Playing (must go through SongSelect)
pub struct GameStateDetector {
    last_state: GameState,
}

impl GameStateDetector {
    pub fn new() -> Self {
        Self {
            last_state: GameState::Unknown,
        }
    }

    /// Check if a state transition is valid
    pub fn is_valid_transition(from: GameState, to: GameState) -> bool {
        if from == to {
            return true;
        }

        matches!(
            (from, to),
            // From Unknown, any state is valid (initial detection)
            (GameState::Unknown, _)
            // Normal flow
            | (GameState::SongSelect, GameState::Playing)
            | (GameState::Playing, GameState::ResultScreen)
            | (GameState::ResultScreen, GameState::SongSelect)
        )
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

        // Validate state transition
        if !Self::is_valid_transition(self.last_state, detected_state) {
            warn!(
                "Invalid state transition detected: {:?} -> {:?}, keeping {:?}",
                self.last_state, detected_state, self.last_state
            );
            return self.last_state;
        }

        self.last_state = detected_state;
        detected_state
    }

    /// Detect state from raw memory values without transition validation
    fn detect_raw(
        &self,
        judge_marker_54: i32,
        judge_marker_55: i32,
        song_select_marker: i32,
        last_state: GameState,
    ) -> GameState {
        // Check if playing (both markers must be non-zero)
        if judge_marker_54 != 0 && judge_marker_55 != 0 {
            return GameState::Playing;
        }

        // Check if in song select
        if song_select_marker == 1 {
            return GameState::SongSelect;
        }

        // Only treat as ResultScreen when transitioning from Playing
        if last_state == GameState::Playing {
            return GameState::ResultScreen;
        }

        // Maintain SongSelect during intermediate transitions (equivalent to C# implementation)
        // "Cannot go from song select to result screen anyway"
        if last_state == GameState::SongSelect {
            return GameState::SongSelect;
        }

        GameState::Unknown
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
    fn test_valid_transitions() {
        // From Unknown, any state is valid
        assert!(GameStateDetector::is_valid_transition(
            GameState::Unknown,
            GameState::SongSelect
        ));
        assert!(GameStateDetector::is_valid_transition(
            GameState::Unknown,
            GameState::Playing
        ));
        assert!(GameStateDetector::is_valid_transition(
            GameState::Unknown,
            GameState::ResultScreen
        ));

        // Normal flow
        assert!(GameStateDetector::is_valid_transition(
            GameState::SongSelect,
            GameState::Playing
        ));
        assert!(GameStateDetector::is_valid_transition(
            GameState::Playing,
            GameState::ResultScreen
        ));
        assert!(GameStateDetector::is_valid_transition(
            GameState::ResultScreen,
            GameState::SongSelect
        ));

        // Same state is always valid
        assert!(GameStateDetector::is_valid_transition(
            GameState::SongSelect,
            GameState::SongSelect
        ));
    }

    #[test]
    fn test_invalid_transitions() {
        // Cannot skip Playing
        assert!(!GameStateDetector::is_valid_transition(
            GameState::SongSelect,
            GameState::ResultScreen
        ));

        // Cannot go back to Playing from ResultScreen
        assert!(!GameStateDetector::is_valid_transition(
            GameState::ResultScreen,
            GameState::Playing
        ));
    }

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
    fn test_detect_unknown_when_idle() {
        let mut detector = GameStateDetector::new();
        let state = detector.detect(0, 0, 0);
        assert_eq!(state, GameState::Unknown);
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
}
