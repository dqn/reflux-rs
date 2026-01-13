use crate::error::Result;
use crate::game::{GameState, PlayData, SongInfo, Difficulty};
use crate::storage::format_post_form;
use std::fs;
use std::path::Path;

pub struct StreamOutput {
    enabled: bool,
    base_dir: String,
}

impl StreamOutput {
    pub fn new(enabled: bool, base_dir: String) -> Self {
        Self { enabled, base_dir }
    }

    pub fn write_play_state(&self, state: GameState) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let state_str = match state {
            GameState::SongSelect => "SELECT",
            GameState::Playing => "PLAYING",
            GameState::ResultScreen => "RESULT",
            GameState::Unknown => "UNKNOWN",
        };

        self.write_file("playstate.txt", state_str)
    }

    pub fn write_current_song(&self, title: &str, difficulty: &str, level: u8) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let content = format!("{} [{}{}]", title, difficulty, level);
        self.write_file("currentsong.txt", &content)
    }

    pub fn write_latest_result(
        &self,
        title: &str,
        difficulty: &str,
        level: u8,
        grade: &str,
        lamp: &str,
        ex_score: u32,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let content = format!(
            "{} [{}{}] {} {} {}",
            title, difficulty, level, grade, lamp, ex_score
        );
        self.write_file("latest.txt", &content)?;
        self.write_file("latest-grade.txt", grade)?;
        self.write_file("latest-lamp.txt", lamp)?;

        Ok(())
    }

    pub fn write_marquee(&self, text: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        self.write_file("marquee.txt", text)
    }

    fn write_file(&self, filename: &str, content: &str) -> Result<()> {
        let path = Path::new(&self.base_dir).join(filename);
        fs::write(path, content)?;
        Ok(())
    }

    /// Write all latest play files (for OBS and external tools)
    pub fn write_latest_files(&self, play_data: &PlayData, api_key: &str) -> Result<()> {
        // latest.json - post form format
        let form = format_post_form(play_data, api_key);
        let json = serde_json::to_string_pretty(&form)?;
        self.write_file("latest.json", &json)?;

        // latest-grade.txt
        self.write_file("latest-grade.txt", play_data.grade.short_name())?;

        // latest-lamp.txt - expanded form
        self.write_file("latest-lamp.txt", play_data.lamp.expand_name())?;

        // latest-difficulty.txt
        self.write_file(
            "latest-difficulty.txt",
            play_data.chart.difficulty.short_name(),
        )?;

        // latest-difficulty-color.txt
        self.write_file(
            "latest-difficulty-color.txt",
            play_data.chart.difficulty.color_code(),
        )?;

        // latest-titleenglish.txt
        self.write_file("latest-titleenglish.txt", &play_data.chart.title_english)?;

        // latest.txt - combined format (title\ngrade\nlamp)
        let combined = format!(
            "{}\n{}\n{}",
            play_data.chart.title_english,
            play_data.grade.short_name(),
            play_data.lamp.short_name()
        );
        self.write_file("latest.txt", &combined)?;

        Ok(())
    }

    /// Write full song info files (for OBS display)
    pub fn write_full_song_info(&self, song: &SongInfo, difficulty: Difficulty) -> Result<()> {
        self.write_file("title.txt", &song.title)?;
        self.write_file("artist.txt", &song.artist)?;
        self.write_file("englishtitle.txt", &song.title_english)?;
        self.write_file("genre.txt", &song.genre)?;
        self.write_file("folder.txt", &song.folder.to_string())?;

        let level = song.get_level(difficulty as usize);
        self.write_file("level.txt", &level.to_string())?;

        Ok(())
    }

    /// Clear full song info files
    pub fn clear_full_song_info(&self) -> Result<()> {
        self.write_file("title.txt", "")?;
        self.write_file("artist.txt", "")?;
        self.write_file("englishtitle.txt", "")?;
        self.write_file("genre.txt", "")?;
        self.write_file("level.txt", "")?;
        self.write_file("folder.txt", "")?;
        Ok(())
    }
}
