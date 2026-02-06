//! Tests for offset searcher validation and search functions

use reflux_core::offset::OffsetsCollection;
use reflux_core::offset::{OffsetSearcher, OffsetValidation, merge_byte_representations};
use reflux_core::process::MockMemoryBuilder;
use reflux_core::process::layout::{judge, settings};

#[test]
fn test_validate_judge_data_candidate_valid() {
    // STATE_MARKER_1 is at offset 0xD8 (WORD * 54)
    // STATE_MARKER_2 is at offset 0xDC (WORD * 55)
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(judge::STATE_MARKER_1 as usize, 50) // Valid marker (0-100)
        .write_i32(judge::STATE_MARKER_2 as usize, 50) // Valid marker (0-100)
        .build();

    assert!(reader.validate_judge_data_candidate(0x1000));
}

#[test]
fn test_validate_judge_data_candidate_invalid_markers() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(judge::STATE_MARKER_1 as usize, 200) // Invalid marker (> 100)
        .write_i32(judge::STATE_MARKER_2 as usize, 50)
        .build();

    // Should fail because first marker is > 100
    assert!(!reader.validate_judge_data_candidate(0x1000));
}

#[test]
fn test_validate_play_settings_valid() {
    // SONG_SELECT_MARKER is at WORD * 6 = 24 bytes before PlaySettings
    // So if PlaySettings is at offset 0x18, song_select_marker is at 0x18 - 0x18 = 0
    let marker_offset = settings::SONG_SELECT_MARKER as usize; // 24

    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        // PlaySettings at offset 0x18 (marker_offset)
        // song_select_marker at offset 0
        .write_i32(0, 1) // song_select_marker
        .write_i32(marker_offset, 2) // style = R-RANDOM
        .write_i32(marker_offset + 4, 3) // gauge = HARD
        .write_i32(marker_offset + 8, 0) // assist = OFF
        .write_i32(marker_offset + 12, 0) // flip = OFF
        .write_i32(marker_offset + 16, 2) // range = HIDDEN+
        .build();

    let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
    assert!(result.is_some());
}

#[test]
fn test_validate_play_settings_invalid_style() {
    let marker_offset = settings::SONG_SELECT_MARKER as usize;

    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 1) // song_select_marker
        .write_i32(marker_offset, 10) // style = INVALID (> 6)
        .write_i32(marker_offset + 4, 2)
        .write_i32(marker_offset + 8, 0)
        .write_i32(marker_offset + 12, 0)
        .write_i32(marker_offset + 16, 1)
        .build();

    let result = reader.validate_play_settings_at(0x1000 + marker_offset as u64);
    assert!(result.is_none());
}

#[test]
fn test_validate_play_data_valid() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 1500) // song_id in range
        .write_i32(4, 3) // difficulty (SPA)
        .write_i32(8, 2000) // ex_score
        .write_i32(12, 25) // miss_count
        .build();

    let result = reader.validate_play_data_address(0x1000);
    assert!(result);
}

#[test]
fn test_validate_play_data_all_zeros_is_rejected() {
    // Initial state (all zeros) should be rejected during offset search
    // to avoid false positives at wrong addresses
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 0) // song_id
        .write_i32(4, 0) // difficulty
        .write_i32(8, 0) // ex_score
        .write_i32(12, 0) // miss_count
        .build();

    let result = reader.validate_play_data_address(0x1000);
    assert!(!result);
}

#[test]
fn test_validate_play_data_invalid_song_id() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 500) // song_id below 1000
        .write_i32(4, 3)
        .write_i32(8, 2000)
        .write_i32(12, 25)
        .build();

    let result = reader.validate_play_data_address(0x1000);
    assert!(!result);
}

#[test]
fn test_validate_current_song_valid() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 2500) // song_id
        .write_i32(4, 5) // difficulty (DPB)
        .write_i32(8, 500) // field3
        .build();

    let result = reader.validate_current_song_address(0x1000);
    assert!(result);
}

#[test]
fn test_validate_current_song_power_of_two_rejected() {
    // Powers of 2 are likely memory artifacts
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 2048) // song_id = 2^11 (power of 2)
        .write_i32(4, 3)
        .write_i32(8, 500)
        .build();
    let result = reader.validate_current_song_address(0x1000);
    assert!(!result);
}

#[test]
fn test_validate_data_map_valid() {
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
fn test_validate_data_map_invalid_size() {
    let base = 0x1000u64;
    let table_start = base + 0x100;
    let table_end = table_start + 0x100; // Only 256 bytes - too small

    let reader = MockMemoryBuilder::new()
        .base(base)
        .with_size(0x1000)
        .write_u64(0, table_start)
        .write_u64(8, table_end)
        .build();

    assert!(!reader.validate_data_map_address(base));
}

#[test]
fn test_validate_unlock_data_valid() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 1500) // song_id in range [1000, 50000]
        .write_i32(4, 2) // unlock_type = Bits (0-3 valid)
        .build();

    assert!(reader.validate_unlock_data_address(0x1000));
}

#[test]
fn test_validate_unlock_data_invalid_song_id() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 500) // song_id too low
        .write_i32(4, 1)
        .build();

    assert!(!reader.validate_unlock_data_address(0x1000));
}

#[test]
fn test_validate_unlock_data_invalid_type() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x100)
        .write_i32(0, 1500)
        .write_i32(4, 10) // unlock_type out of range
        .build();

    assert!(!reader.validate_unlock_data_address(0x1000));
}

#[test]
fn test_merge_byte_representations() {
    let bytes = merge_byte_representations(&[1000, 42]);
    // 1000 = 0x000003E8, 42 = 0x0000002A in little-endian
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[0..4], &[0xE8, 0x03, 0x00, 0x00]);
    assert_eq!(&bytes[4..8], &[0x2A, 0x00, 0x00, 0x00]);
}

#[test]
fn test_find_all_matches() {
    let reader = MockMemoryBuilder::new()
        .base(0x1000)
        .with_size(0x200) // Large enough for the search
        .write_bytes(0x10, &[0xAB, 0xCD])
        .write_bytes(0x30, &[0xAB, 0xCD])
        .write_bytes(0x50, &[0xAB, 0xCD])
        .build();

    let mut searcher = OffsetSearcher::new(&reader);
    // Load buffer around the center with smaller distance
    searcher.load_buffer_around(0x1080, 0x80).unwrap();

    let matches = searcher.find_all_matches(&[0xAB, 0xCD]);
    assert_eq!(matches.len(), 3);
}

#[test]
fn test_offsets_collection_is_valid() {
    let valid = OffsetsCollection {
        version: "test".to_string(),
        song_list: 0x1000,
        judge_data: 0x2000,
        play_settings: 0x3000,
        play_data: 0x4000,
        current_song: 0x5000,
        data_map: 0x6000,
        unlock_data: 0x7000,
    };
    assert!(valid.is_valid());

    let invalid = OffsetsCollection {
        version: "test".to_string(),
        song_list: 0, // Zero = invalid
        judge_data: 0x2000,
        play_settings: 0x3000,
        play_data: 0x4000,
        current_song: 0x5000,
        data_map: 0x6000,
        unlock_data: 0x7000,
    };
    assert!(!invalid.is_valid());
}
