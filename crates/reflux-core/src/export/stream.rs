use crate::error::Result;
use crate::chart::{Difficulty, SongInfo};
use crate::play::GameState;
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
            GameState::SongSelect => "menu",
            GameState::Playing => "play",
            GameState::ResultScreen => "menu",
            GameState::Unknown => "off",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::UnlockType;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn test_stream_output_disabled() {
        let output = StreamOutput::new(false, "/nonexistent".to_string());

        // When disabled, all writes should succeed without actually writing
        assert!(output.write_play_state(GameState::Playing).is_ok());
        assert!(output.write_current_song("Test", "SPA", 12).is_ok());
        assert!(
            output
                .write_latest_result("Test", "SPA", 12, "AAA", "HARD CLEAR", 2000)
                .is_ok()
        );
        assert!(output.write_marquee("Test marquee").is_ok());
    }

    #[test]
    fn test_stream_output_write_play_state() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_str().unwrap().to_string();
        let output = StreamOutput::new(true, base_dir.clone());

        output.write_play_state(GameState::SongSelect).unwrap();
        let content = fs::read_to_string(temp_dir.path().join("playstate.txt")).unwrap();
        assert_eq!(content, "menu");

        output.write_play_state(GameState::Playing).unwrap();
        let content = fs::read_to_string(temp_dir.path().join("playstate.txt")).unwrap();
        assert_eq!(content, "play");

        output.write_play_state(GameState::Unknown).unwrap();
        let content = fs::read_to_string(temp_dir.path().join("playstate.txt")).unwrap();
        assert_eq!(content, "off");
    }

    #[test]
    fn test_stream_output_write_current_song() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_str().unwrap().to_string();
        let output = StreamOutput::new(true, base_dir);

        output.write_current_song("Test Song", "SPA", 12).unwrap();
        let content = fs::read_to_string(temp_dir.path().join("currentsong.txt")).unwrap();
        assert_eq!(content, "Test Song [SPA12]");
    }

    #[test]
    fn test_stream_output_write_latest_result() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_str().unwrap().to_string();
        let output = StreamOutput::new(true, base_dir);

        output
            .write_latest_result("Test Song", "SPA", 12, "AAA", "HARD CLEAR", 2000)
            .unwrap();

        let content = fs::read_to_string(temp_dir.path().join("latest.txt")).unwrap();
        assert_eq!(content, "Test Song [SPA12] AAA HARD CLEAR 2000");

        let grade = fs::read_to_string(temp_dir.path().join("latest-grade.txt")).unwrap();
        assert_eq!(grade, "AAA");

        let lamp = fs::read_to_string(temp_dir.path().join("latest-lamp.txt")).unwrap();
        assert_eq!(lamp, "HARD CLEAR");
    }

    #[test]
    fn test_stream_output_write_full_song_info() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_str().unwrap().to_string();
        let output = StreamOutput::new(true, base_dir);

        let mut levels = [0u8; 10];
        levels[3] = 11; // SPA

        let song = SongInfo {
            id: 1001,
            title: Arc::from("Test Title"),
            title_english: Arc::from("Test Title EN"),
            artist: Arc::from("Test Artist"),
            genre: Arc::from("Test Genre"),
            bpm: Arc::from("150"),
            folder: 1,
            levels,
            total_notes: [0; 10],
            unlock_type: UnlockType::Base,
        };

        output.write_full_song_info(&song, Difficulty::SpA).unwrap();

        assert_eq!(
            fs::read_to_string(temp_dir.path().join("title.txt")).unwrap(),
            "Test Title"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("artist.txt")).unwrap(),
            "Test Artist"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("englishtitle.txt")).unwrap(),
            "Test Title EN"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("genre.txt")).unwrap(),
            "Test Genre"
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("level.txt")).unwrap(),
            "11"
        );
    }

    #[test]
    fn test_stream_output_clear_full_song_info() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path().to_str().unwrap().to_string();
        let output = StreamOutput::new(true, base_dir);

        // First write some content
        output.write_file("title.txt", "Test").unwrap();
        output.write_file("artist.txt", "Test").unwrap();

        // Then clear
        output.clear_full_song_info().unwrap();

        assert_eq!(
            fs::read_to_string(temp_dir.path().join("title.txt")).unwrap(),
            ""
        );
        assert_eq!(
            fs::read_to_string(temp_dir.path().join("artist.txt")).unwrap(),
            ""
        );
    }
}
