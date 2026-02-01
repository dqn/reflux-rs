//! Interactive offset search functionality
//!
//! This module provides the interactive offset discovery process that guides
//! users through finding game data structures in memory.

use crate::error::Result;
use crate::offset::OffsetsCollection;
use crate::play::PlayType;
use crate::process::ReadMemory;

use super::OffsetSearcher;
use super::constants::*;
use super::types::{InteractiveSearchResult, JudgeInput, SearchPrompter};
use super::utils::merge_byte_representations;

impl<'a, R: ReadMemory> OffsetSearcher<'a, R> {
    /// Run interactive offset search with user prompts
    ///
    /// This method guides the user through the offset discovery process:
    /// 1. Search SongList, UnlockData, DataMap
    /// 2. User plays "Sleepless Days SPA" and enters judge data
    /// 3. Search JudgeData, PlayData, CurrentSong
    /// 4. User sets specific options and searches PlaySettings
    pub fn interactive_search<P: SearchPrompter>(
        &mut self,
        prompter: &P,
        old_offsets: &OffsetsCollection,
        new_version: &str,
    ) -> Result<InteractiveSearchResult> {
        prompter.prompt_continue("Starting offset search mode, press ENTER to continue");

        let mut new_offsets = OffsetsCollection {
            version: new_version.to_string(),
            ..Default::default()
        };

        // Use base address as default hint if old offsets are invalid
        let base = self.reader.base_address();
        let hint = |offset: u64| if offset == 0 { base } else { offset };

        // Phase 1: Static patterns
        prompter.display_message("Searching for SongList...");
        new_offsets.song_list = self.search_song_list_offset(hint(old_offsets.song_list))?;
        prompter.display_message(&format!("Found SongList at 0x{:X}", new_offsets.song_list));

        prompter.display_message("Searching for UnlockData...");
        new_offsets.unlock_data = self.search_unlock_data_offset(hint(old_offsets.unlock_data))?;
        prompter.display_message(&format!(
            "Found UnlockData at 0x{:X}",
            new_offsets.unlock_data
        ));

        prompter.display_message("Searching for DataMap...");
        // Use SongList as hint for DataMap since they are in similar memory region
        let data_map_hint = if old_offsets.data_map != 0 {
            old_offsets.data_map
        } else {
            new_offsets.song_list
        };
        new_offsets.data_map = self.search_data_map_offset(data_map_hint)?;
        prompter.display_message(&format!("Found DataMap at 0x{:X}", new_offsets.data_map));

        // Phase 2: Judge data (requires playing a song)
        prompter.prompt_continue(
            "Play Sleepless Days SPA, either fully or exit after hitting 50-ish notes or more, then press ENTER"
        );

        prompter.display_message("Enter your judge data:");
        let judge = JudgeInput {
            pgreat: prompter.prompt_number("Enter pgreat count: "),
            great: prompter.prompt_number("Enter great count: "),
            good: prompter.prompt_number("Enter good count: "),
            bad: prompter.prompt_number("Enter bad count: "),
            poor: prompter.prompt_number("Enter poor count: "),
            combo_break: prompter.prompt_number("Enter combobreak count: "),
            fast: prompter.prompt_number("Enter fast count: "),
            slow: prompter.prompt_number("Enter slow count: "),
        };

        // Try P1 pattern first, then P2
        prompter.display_message("Searching for JudgeData...");
        let (judge_address, play_type) =
            self.search_judge_data_with_playtype(hint(old_offsets.judge_data), &judge)?;
        new_offsets.judge_data = judge_address;
        prompter.display_message(&format!(
            "Found JudgeData at 0x{:X} ({})",
            new_offsets.judge_data,
            play_type.short_name()
        ));

        // Phase 3: Play data and current song (Sleepless Days SPA = 25094, difficulty 3)
        let ex_score = judge.pgreat * 2 + judge.great;
        prompter.display_message("Searching for PlayData...");
        new_offsets.play_data =
            self.search_play_data_offset(hint(old_offsets.play_data), 25094, 3, ex_score)?;
        prompter.display_message(&format!("Found PlayData at 0x{:X}", new_offsets.play_data));

        prompter.display_message("Searching for CurrentSong...");
        let current_song_addr =
            self.search_current_song_offset(hint(old_offsets.current_song), 25094, 3)?;
        // Verify it's different from PlayData
        new_offsets.current_song = if current_song_addr == new_offsets.play_data {
            self.search_current_song_offset_excluding(
                hint(old_offsets.current_song),
                25094,
                3,
                Some(new_offsets.play_data),
            )?
        } else {
            current_song_addr
        };
        prompter.display_message(&format!(
            "Found CurrentSong at 0x{:X}",
            new_offsets.current_song
        ));

        // Phase 4: Play settings (requires user to set specific options)
        // C# prompts: "RANDOM EXHARD OFF SUDDEN+" and "MIRROR EASY AUTO-SCRATCH HIDDEN+"
        prompter.prompt_continue(
            "Set the following settings and then press ENTER: RANDOM EXHARD OFF SUDDEN+",
        );

        prompter.display_message("Searching for PlaySettings...");
        // RANDOM=1, EXHARD=4, OFF=0, SUDDEN+=1 (C# values)
        let settings_addr1 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            1, // RANDOM (style)
            4, // EXHARD (gauge) - C# uses 4 for EXHARD
            0, // OFF (assist)
            1, // SUDDEN+ (range)
        )?;

        prompter.prompt_continue(
            "Now set the following settings and then press ENTER: MIRROR EASY AUTO-SCRATCH HIDDEN+",
        );

        // MIRROR=4, EASY=2, AUTO-SCRATCH=1, HIDDEN+=2
        let settings_addr2 = self.search_play_settings_offset(
            hint(old_offsets.play_settings),
            4, // MIRROR (style)
            2, // EASY (gauge)
            1, // AUTO-SCRATCH (assist)
            2, // HIDDEN+ (range)
        )?;

        if settings_addr1 != settings_addr2 {
            prompter
                .display_warning("Warning: Settings addresses don't match between two searches!");
        }

        // Adjust for P2 offset if needed
        new_offsets.play_settings = if play_type == PlayType::P2 {
            use crate::play::Settings;
            settings_addr1 - Settings::P2_OFFSET
        } else {
            settings_addr1
        };
        prompter.display_message(&format!(
            "Found PlaySettings at 0x{:X}",
            new_offsets.play_settings
        ));

        prompter.display_message("Offset search complete!");

        Ok(InteractiveSearchResult {
            offsets: new_offsets,
            play_type,
        })
    }

    /// Search for judge data and determine play type
    pub(crate) fn search_judge_data_with_playtype(
        &mut self,
        base_hint: u64,
        judge: &JudgeInput,
    ) -> Result<(u64, PlayType)> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let (pattern_p1, pattern_p2) = self.build_judge_patterns(judge);
        let patterns = vec![pattern_p1, pattern_p2];

        let result = self.fetch_and_search_alternating(base_hint, &patterns, 0, None)?;

        let play_type = if result.pattern_index == 0 {
            PlayType::P1
        } else {
            PlayType::P2
        };

        Ok((result.address, play_type))
    }

    /// Search for current song offset, excluding a specific address
    pub(crate) fn search_current_song_offset_excluding(
        &mut self,
        base_hint: u64,
        song_id: u32,
        difficulty: u32,
        exclude: Option<u64>,
    ) -> Result<u64> {
        self.load_buffer_around(base_hint, INITIAL_SEARCH_SIZE)?;

        let pattern = merge_byte_representations(&[song_id as i32, difficulty as i32]);
        self.fetch_and_search(base_hint, &pattern, 0, exclude)
    }
}
