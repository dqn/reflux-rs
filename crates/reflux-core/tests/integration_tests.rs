//! Integration tests for reflux-core
//!
//! These tests verify that multiple modules work together correctly.
//! Tests requiring mock memory readers are in unit tests within the crate.

use reflux_core::chart::Difficulty;
use reflux_core::play::{GameState, GameStateDetector};
use reflux_core::score::{Grade, Lamp};
use reflux_core::retry::{ExponentialBackoff, FixedDelay, NoRetry, RetryStrategy};

/// Test game state detection
mod game_state_tests {
    use super::*;

    #[test]
    fn test_detect_playing_state() {
        let mut detector = GameStateDetector::new();

        // Marker1 > 0, Marker2 = 0 indicates playing
        let state = detector.detect(100, 0, 0);
        assert_eq!(state, GameState::Playing);
    }

    #[test]
    fn test_detect_result_screen_state() {
        let mut detector = GameStateDetector::new();

        // First transition from Unknown
        detector.detect(0, 0, 0);

        // Marker2 > 0 with marker1 = 0 indicates result screen
        let state = detector.detect(0, 100, 0);
        assert_eq!(state, GameState::ResultScreen);
    }

    #[test]
    fn test_detect_song_select_state() {
        let mut detector = GameStateDetector::new();

        // First set a different state
        detector.detect(100, 0, 0); // Playing

        // Then transition to song select
        let state = detector.detect(0, 0, 1);
        assert_eq!(state, GameState::SongSelect);
    }

    #[test]
    fn test_state_transitions() {
        let mut detector = GameStateDetector::new();

        // Unknown -> Playing
        let state = detector.detect(100, 0, 0);
        assert_eq!(state, GameState::Playing);

        // Playing -> Result Screen
        let state = detector.detect(0, 100, 0);
        assert_eq!(state, GameState::ResultScreen);

        // Result Screen -> Song Select
        let state = detector.detect(0, 0, 1);
        assert_eq!(state, GameState::SongSelect);

        // Song Select -> Playing (new song)
        let state = detector.detect(200, 0, 0);
        assert_eq!(state, GameState::Playing);
    }
}

/// Test retry strategies
mod retry_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_exponential_backoff_config() {
        let strategy = ExponentialBackoff::new();

        assert_eq!(strategy.max_attempts(), 5);
        assert_eq!(
            strategy.delay_for_attempt(0),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            strategy.delay_for_attempt(4),
            Some(Duration::from_millis(1600))
        );
        assert_eq!(strategy.delay_for_attempt(5), None);
    }

    #[test]
    fn test_fixed_delay_config() {
        let strategy = FixedDelay::new(3, Duration::from_millis(50));

        assert_eq!(strategy.max_attempts(), 3);
        assert_eq!(
            strategy.delay_for_attempt(0),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            strategy.delay_for_attempt(2),
            Some(Duration::from_millis(50))
        );
    }

    #[test]
    fn test_no_retry_config() {
        let strategy = NoRetry::new();

        assert_eq!(strategy.max_attempts(), 1);
        assert_eq!(strategy.delay_for_attempt(0), None);
    }

    #[test]
    fn test_retry_execute_with_eventual_success() {
        let strategy = FixedDelay::new(5, Duration::from_millis(1));
        let mut attempts = 0;

        let result: Result<i32, &str> = strategy.execute(|_| {
            attempts += 1;
            if attempts < 3 { Err("not yet") } else { Ok(42) }
        });

        assert_eq!(result, Ok(42));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn test_retry_execute_all_failures() {
        let strategy = FixedDelay::new(3, Duration::from_millis(1));
        let mut attempts = 0;

        let result: Result<i32, &str> = strategy.execute(|_| {
            attempts += 1;
            Err("always fails")
        });

        assert!(result.is_err());
        assert_eq!(attempts, 3);
    }
}

/// Test grade calculation
mod grade_tests {
    use super::*;

    #[test]
    fn test_grade_from_score_ratio() {
        // AAA: 88.89%+ (8/9)
        assert_eq!(Grade::from_score_ratio(0.8889), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(0.9), Grade::Aaa);

        // AA: 77.78%+ (7/9)
        assert_eq!(Grade::from_score_ratio(0.7778), Grade::Aa);
        assert_eq!(Grade::from_score_ratio(0.8), Grade::Aa);

        // A: 66.67%+ (6/9)
        assert_eq!(Grade::from_score_ratio(0.6667), Grade::A);

        // B: 55.56%+ (5/9)
        assert_eq!(Grade::from_score_ratio(0.5556), Grade::B);

        // C: 44.45%+ (4/9)
        assert_eq!(Grade::from_score_ratio(0.4445), Grade::C);

        // D: 33.34%+ (3/9)
        assert_eq!(Grade::from_score_ratio(0.3334), Grade::D);

        // E: 22.23%+ (2/9)
        assert_eq!(Grade::from_score_ratio(0.2223), Grade::E);

        // F: below 22.23%
        assert_eq!(Grade::from_score_ratio(0.2), Grade::F);
        assert_eq!(Grade::from_score_ratio(0.0), Grade::F);
    }

    #[test]
    fn test_grade_perfect_score() {
        assert_eq!(Grade::from_score_ratio(1.0), Grade::Aaa);
    }

    #[test]
    fn test_grade_boundary_values() {
        // Test exact boundary values using the 1/9 thresholds
        assert_eq!(Grade::from_score_ratio(8.0 / 9.0), Grade::Aaa);
        assert_eq!(Grade::from_score_ratio(7.0 / 9.0), Grade::Aa);
        assert_eq!(Grade::from_score_ratio(6.0 / 9.0), Grade::A);
        assert_eq!(Grade::from_score_ratio(5.0 / 9.0), Grade::B);
        assert_eq!(Grade::from_score_ratio(4.0 / 9.0), Grade::C);
        assert_eq!(Grade::from_score_ratio(3.0 / 9.0), Grade::D);
        assert_eq!(Grade::from_score_ratio(2.0 / 9.0), Grade::E);
        assert_eq!(Grade::from_score_ratio(1.0 / 9.0), Grade::F);
    }
}

/// Test difficulty enum
mod difficulty_tests {
    use super::*;

    #[test]
    fn test_difficulty_from_u8() {
        // Order: SPB(0), SPN(1), SPH(2), SPA(3), SPL(4), DPB(5), DPN(6), DPH(7), DPA(8), DPL(9)
        assert_eq!(Difficulty::from_u8(0), Some(Difficulty::SpB));
        assert_eq!(Difficulty::from_u8(1), Some(Difficulty::SpN));
        assert_eq!(Difficulty::from_u8(2), Some(Difficulty::SpH));
        assert_eq!(Difficulty::from_u8(3), Some(Difficulty::SpA));
        assert_eq!(Difficulty::from_u8(4), Some(Difficulty::SpL));
        assert_eq!(Difficulty::from_u8(5), Some(Difficulty::DpB));
        assert_eq!(Difficulty::from_u8(6), Some(Difficulty::DpN));
        assert_eq!(Difficulty::from_u8(7), Some(Difficulty::DpH));
        assert_eq!(Difficulty::from_u8(8), Some(Difficulty::DpA));
        assert_eq!(Difficulty::from_u8(9), Some(Difficulty::DpL));
        assert_eq!(Difficulty::from_u8(10), None);
    }

    #[test]
    fn test_difficulty_is_sp_dp() {
        assert!(Difficulty::SpN.is_sp());
        assert!(Difficulty::SpA.is_sp());
        assert!(!Difficulty::DpN.is_sp());
        assert!(!Difficulty::DpA.is_sp());

        assert!(!Difficulty::SpN.is_dp());
        assert!(Difficulty::DpN.is_dp());
        assert!(Difficulty::DpA.is_dp());
    }
}

/// Test lamp ordering
mod lamp_tests {
    use super::*;

    #[test]
    fn test_lamp_ordering() {
        assert!(Lamp::NoPlay < Lamp::Failed);
        assert!(Lamp::Failed < Lamp::AssistClear);
        assert!(Lamp::AssistClear < Lamp::EasyClear);
        assert!(Lamp::EasyClear < Lamp::Clear);
        assert!(Lamp::Clear < Lamp::HardClear);
        assert!(Lamp::HardClear < Lamp::ExHardClear);
        assert!(Lamp::ExHardClear < Lamp::FullCombo);
        assert!(Lamp::FullCombo < Lamp::Pfc);
    }

    #[test]
    fn test_lamp_max() {
        let lamps = vec![Lamp::Clear, Lamp::HardClear, Lamp::EasyClear];
        assert_eq!(lamps.into_iter().max(), Some(Lamp::HardClear));
    }
}
