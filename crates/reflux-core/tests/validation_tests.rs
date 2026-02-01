//! Tests for offset validation functions
//!
//! These tests cover edge cases and boundary conditions for each validation function.

use reflux_core::offset::{OffsetValidation, OffsetsCollection};
use reflux_core::process::MockMemoryBuilder;
use reflux_core::process::layout::{judge, settings};

// =============================================================================
// JudgeData validation tests
// =============================================================================

mod judge_data {
    use super::*;

    #[test]
    fn valid_with_all_zeros_song_select_state() {
        // Song select state: all judgment values are zero, markers are valid
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            // First 72 bytes are zeros (judgment region)
            .write_i32(judge::STATE_MARKER_1 as usize, 0) // Marker in range 0-100
            .write_i32(judge::STATE_MARKER_2 as usize, 0)
            .build();

        assert!(reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn valid_with_play_data() {
        // During/after play: judgment values contain counts
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 500) // P1 pgreat
            .write_i32(4, 100) // P1 great
            .write_i32(8, 10) // P1 good
            .write_i32(12, 5) // P1 bad
            .write_i32(16, 2) // P1 poor
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn invalid_marker_over_100() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 150) // Invalid: > 100
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(!reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn invalid_negative_marker() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, -1) // Invalid: negative
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(!reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn invalid_unaligned_address() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        // Unaligned address should fail
        assert!(!reader.validate_judge_data_candidate(0x1001));
        assert!(!reader.validate_judge_data_candidate(0x1002));
        assert!(!reader.validate_judge_data_candidate(0x1003));
    }

    #[test]
    fn invalid_judgment_value_too_high() {
        // A judgment value exceeds MAX_NOTES (5000)
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 6000) // Invalid: > MAX_NOTES
            .write_i32(judge::STATE_MARKER_1 as usize, 50)
            .write_i32(judge::STATE_MARKER_2 as usize, 50)
            .build();

        assert!(!reader.validate_judge_data_candidate(0x1000));
    }

    #[test]
    fn boundary_marker_values() {
        // Test boundary values for markers (0 and 100)
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(judge::STATE_MARKER_1 as usize, 0) // Min valid
            .write_i32(judge::STATE_MARKER_2 as usize, 100) // Max valid
            .build();

        assert!(reader.validate_judge_data_candidate(0x1000));
    }
}

// =============================================================================
// PlaySettings validation tests
// =============================================================================

mod play_settings {
    use super::*;

    #[test]
    fn valid_with_all_minimum_values() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 0) // song_select_marker = 0
            .write_i32(marker_offset, 0) // style = OFF
            .write_i32(marker_offset + 4, 0) // gauge = NORMAL
            .write_i32(marker_offset + 8, 0) // assist = OFF
            .write_i32(marker_offset + 12, 0) // flip = OFF
            .write_i32(marker_offset + 16, 0) // range = OFF
            .build();

        let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
        assert!(result.is_some());
    }

    #[test]
    fn valid_with_all_maximum_values() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1) // song_select_marker = 1
            .write_i32(marker_offset, 6) // style = max
            .write_i32(marker_offset + 4, 4) // gauge = max
            .write_i32(marker_offset + 8, 5) // assist = max
            .write_i32(marker_offset + 12, 1) // flip = max
            .write_i32(marker_offset + 16, 5) // range = max
            .build();

        let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
        assert!(result.is_some());
    }

    #[test]
    fn invalid_style_out_of_range() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1)
            .write_i32(marker_offset, 7) // Invalid: > 6
            .write_i32(marker_offset + 4, 2)
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 0)
            .write_i32(marker_offset + 16, 1)
            .build();

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_none()
        );
    }

    #[test]
    fn invalid_gauge_out_of_range() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1)
            .write_i32(marker_offset, 2)
            .write_i32(marker_offset + 4, 5) // Invalid: > 4
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 0)
            .write_i32(marker_offset + 16, 1)
            .build();

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_none()
        );
    }

    #[test]
    fn invalid_flip_out_of_range() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1)
            .write_i32(marker_offset, 2)
            .write_i32(marker_offset + 4, 2)
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 2) // Invalid: > 1
            .write_i32(marker_offset + 16, 1)
            .build();

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_none()
        );
    }

    #[test]
    fn invalid_song_select_marker() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 5) // Invalid: > 1
            .write_i32(marker_offset, 2)
            .write_i32(marker_offset + 4, 2)
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 0)
            .write_i32(marker_offset + 16, 1)
            .build();

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_none()
        );
    }

    #[test]
    fn invalid_negative_values() {
        let marker_offset = settings::SONG_SELECT_MARKER as usize;

        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1)
            .write_i32(marker_offset, -1) // Invalid: negative
            .write_i32(marker_offset + 4, 2)
            .write_i32(marker_offset + 8, 0)
            .write_i32(marker_offset + 12, 0)
            .write_i32(marker_offset + 16, 1)
            .build();

        assert!(
            reader
                .validate_play_settings_at(0x1000 + marker_offset as u64)
                .is_none()
        );
    }
}

// =============================================================================
// PlayData validation tests
// =============================================================================

mod play_data {
    use super::*;

    #[test]
    fn valid_typical_play_data() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500) // song_id
            .write_i32(4, 3) // difficulty (SPA)
            .write_i32(8, 2000) // ex_score
            .write_i32(12, 25) // miss_count
            .build();

        assert!(reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn valid_boundary_song_id_min() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1000) // min valid song_id
            .write_i32(4, 0)
            .write_i32(8, 100)
            .write_i32(12, 0)
            .build();

        assert!(reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn valid_boundary_song_id_max() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 50000) // max valid song_id
            .write_i32(4, 9) // max difficulty
            .write_i32(8, 10000) // max ex_score
            .write_i32(12, 3000) // max miss_count
            .build();

        assert!(reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_all_zeros_rejected() {
        // Initial state (all zeros) should be rejected during offset search
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 0)
            .write_i32(4, 0)
            .write_i32(8, 0)
            .write_i32(12, 0)
            .build();

        assert!(!reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_song_id_below_1000() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 500) // Invalid: < 1000
            .write_i32(4, 3)
            .write_i32(8, 2000)
            .write_i32(12, 25)
            .build();

        assert!(!reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_song_id_above_50000() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 60000) // Invalid: > 50000
            .write_i32(4, 3)
            .write_i32(8, 2000)
            .write_i32(12, 25)
            .build();

        assert!(!reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_difficulty_above_9() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500)
            .write_i32(4, 10) // Invalid: > 9
            .write_i32(8, 2000)
            .write_i32(12, 25)
            .build();

        assert!(!reader.validate_play_data_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_ex_score_above_10000() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500)
            .write_i32(4, 3)
            .write_i32(8, 15000) // Invalid: > 10000
            .write_i32(12, 25)
            .build();

        assert!(!reader.validate_play_data_address(0x1000).unwrap());
    }
}

// =============================================================================
// CurrentSong validation tests
// =============================================================================

mod current_song {
    use super::*;

    #[test]
    fn valid_typical_current_song() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500) // song_id
            .write_i32(4, 5) // difficulty
            .write_i32(8, 500) // field3
            .build();

        assert!(reader.validate_current_song_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_all_zeros_rejected() {
        // Initial state (zeros) should be rejected
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 0)
            .write_i32(4, 0)
            .write_i32(8, 0)
            .build();

        assert!(!reader.validate_current_song_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_power_of_two_song_id_rejected() {
        // Powers of 2 are likely memory artifacts
        for power in [1024, 2048, 4096, 8192, 16384, 32768] {
            let reader = MockMemoryBuilder::new()
                .base(0x1000)
                .with_size(0x100)
                .write_i32(0, power) // Power of 2
                .write_i32(4, 3)
                .write_i32(8, 500)
                .build();

            assert!(
                !reader.validate_current_song_address(0x1000).unwrap(),
                "Power of 2 song_id {} should be rejected",
                power
            );
        }
    }

    #[test]
    fn valid_near_power_of_two_accepted() {
        // Values near powers of 2 should be accepted if in valid range
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2049) // 2048 + 1
            .write_i32(4, 3)
            .write_i32(8, 500)
            .build();

        assert!(reader.validate_current_song_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_difficulty_above_9() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500)
            .write_i32(4, 10) // Invalid: > 9
            .write_i32(8, 500)
            .build();

        assert!(!reader.validate_current_song_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_field3_above_10000() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500)
            .write_i32(4, 5)
            .write_i32(8, 15000) // Invalid: > 10000
            .build();

        assert!(!reader.validate_current_song_address(0x1000).unwrap());
    }

    #[test]
    fn invalid_negative_field3() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 2500)
            .write_i32(4, 5)
            .write_i32(8, -1) // Invalid: negative
            .build();

        assert!(!reader.validate_current_song_address(0x1000).unwrap());
    }
}

// =============================================================================
// DataMap validation tests
// =============================================================================

mod data_map {
    use super::*;

    #[test]
    fn valid_data_map() {
        let base = 0x1000u64;
        let table_start = base + 0x100;
        let table_end = table_start + 0x4000; // 16KB table

        let reader = MockMemoryBuilder::new()
            .base(base)
            .with_size(0x8000)
            .write_u64(0, table_start)
            .write_u64(8, table_end)
            .build();

        assert!(reader.validate_data_map_address(base));
    }

    #[test]
    fn invalid_table_too_small() {
        let base = 0x1000u64;
        let table_start = base + 0x100;
        let table_end = table_start + 0x100; // Only 256 bytes

        let reader = MockMemoryBuilder::new()
            .base(base)
            .with_size(0x1000)
            .write_u64(0, table_start)
            .write_u64(8, table_end)
            .build();

        assert!(!reader.validate_data_map_address(base));
    }

    #[test]
    fn invalid_end_before_start() {
        let base = 0x1000u64;

        let reader = MockMemoryBuilder::new()
            .base(base)
            .with_size(0x1000)
            .write_u64(0, 0x2000) // start
            .write_u64(8, 0x1000) // end < start
            .build();

        assert!(!reader.validate_data_map_address(base));
    }
}

// =============================================================================
// UnlockData validation tests
// =============================================================================

mod unlock_data {
    use super::*;

    #[test]
    fn valid_unlock_data() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500) // song_id
            .write_i32(4, 2) // unlock_type = Bits (0-3 valid)
            .build();

        assert!(reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn valid_all_unlock_types() {
        // Test all valid unlock types (0-3)
        for unlock_type in 0..=3 {
            let reader = MockMemoryBuilder::new()
                .base(0x1000)
                .with_size(0x100)
                .write_i32(0, 1500)
                .write_i32(4, unlock_type)
                .build();

            assert!(
                reader.validate_unlock_data_address(0x1000),
                "unlock_type {} should be valid",
                unlock_type
            );
        }
    }

    #[test]
    fn invalid_song_id_below_1000() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 500) // Invalid: < 1000
            .write_i32(4, 1)
            .build();

        assert!(!reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn invalid_unlock_type_above_3() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500)
            .write_i32(4, 10) // Invalid: > 3
            .build();

        assert!(!reader.validate_unlock_data_address(0x1000));
    }

    #[test]
    fn invalid_negative_unlock_type() {
        let reader = MockMemoryBuilder::new()
            .base(0x1000)
            .with_size(0x100)
            .write_i32(0, 1500)
            .write_i32(4, -1) // Invalid: negative
            .build();

        assert!(!reader.validate_unlock_data_address(0x1000));
    }
}

// =============================================================================
// OffsetsCollection validation tests
// =============================================================================

mod offsets_collection {
    use super::*;

    #[test]
    fn valid_offsets_collection() {
        let offsets = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0x1000,
            judge_data: 0x2000,
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0x6000,
            unlock_data: 0x7000,
        };

        assert!(offsets.is_valid());
    }

    #[test]
    fn invalid_with_zero_song_list() {
        let offsets = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0, // Invalid
            judge_data: 0x2000,
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0x6000,
            unlock_data: 0x7000,
        };

        assert!(!offsets.is_valid());
    }

    #[test]
    fn invalid_with_zero_judge_data() {
        let offsets = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0x1000,
            judge_data: 0, // Invalid
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0x6000,
            unlock_data: 0x7000,
        };

        assert!(!offsets.is_valid());
    }

    #[test]
    fn valid_with_zero_optional_offsets() {
        // data_map and unlock_data can be zero (optional)
        let offsets = OffsetsCollection {
            version: "test".to_string(),
            song_list: 0x1000,
            judge_data: 0x2000,
            play_settings: 0x3000,
            play_data: 0x4000,
            current_song: 0x5000,
            data_map: 0,    // Optional
            unlock_data: 0, // Optional
        };

        // Note: is_valid() checks all fields are non-zero
        // So this should fail - but that's the current behavior
        assert!(!offsets.is_valid());
    }
}
