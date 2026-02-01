use crate::error::Result;
use crate::export::{format_full_tsv_header, format_full_tsv_row, format_json_entry};
use crate::play::PlayData;
use chrono::{DateTime, Local};
use serde_json::Value as JsonValue;
use std::fs::{self};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct SessionManager {
    base_dir: PathBuf,
    current_tsv_session: Option<PathBuf>,
    current_json_session: Option<PathBuf>,
    json_data: Vec<JsonValue>,
}

impl SessionManager {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            current_tsv_session: None,
            current_json_session: None,
            json_data: Vec::new(),
        }
    }

    /// Start a new session with TSV header written
    pub fn start_session(&mut self) -> Result<PathBuf> {
        let now: DateTime<Local> = Local::now();
        let session_dir = self.base_dir.join(now.format("%Y-%m-%d").to_string());
        fs::create_dir_all(&session_dir)?;

        let session_file = session_dir.join(format!("session_{}.tsv", now.format("%H%M%S")));
        self.current_tsv_session = Some(session_file.clone());

        Ok(session_file)
    }

    /// Start a session with TSV header
    pub fn start_tsv_session(&mut self) -> Result<PathBuf> {
        let now: DateTime<Local> = Local::now();
        fs::create_dir_all(&self.base_dir)?;

        // TSV session file (C# compatible naming)
        let tsv_file = self
            .base_dir
            .join(format!("Session_{}.tsv", now.format("%Y_%m_%d_%H_%M_%S")));

        // Write header
        let header = format_full_tsv_header();
        fs::write(&tsv_file, format!("{}\n", header))?;

        self.current_tsv_session = Some(tsv_file.clone());

        Ok(tsv_file)
    }

    /// Start a JSON session file
    pub fn start_json_session(&mut self) -> Result<PathBuf> {
        let now: DateTime<Local> = Local::now();
        fs::create_dir_all(&self.base_dir)?;

        let json_file = self
            .base_dir
            .join(format!("Session_{}.json", now.format("%Y_%m_%d_%H_%M_%S")));

        // Initialize as empty array
        self.json_data = Vec::new();
        fs::write(&json_file, "[]")?;

        self.current_json_session = Some(json_file.clone());

        Ok(json_file)
    }

    /// Append a TSV row to the session file
    pub fn append_tsv_row(&self, play_data: &PlayData) -> Result<()> {
        if let Some(ref path) = self.current_tsv_session {
            let row = format_full_tsv_row(play_data);
            let mut file = fs::OpenOptions::new().append(true).open(path)?;
            writeln!(file, "{}", row)?;
        }
        Ok(())
    }

    /// Append a JSON entry to the session file
    pub fn append_json_entry(&mut self, play_data: &PlayData) -> Result<()> {
        if let Some(path) = &self.current_json_session {
            let entry = format_json_entry(play_data);
            self.json_data.push(entry);
            fs::write(path, serde_json::to_string_pretty(&self.json_data)?)?;
        }
        Ok(())
    }

    /// Append a line to the TSV session (legacy method)
    pub fn append_line(&self, line: &str) -> Result<()> {
        if let Some(ref path) = self.current_tsv_session {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(file, "{}", line)?;
        }
        Ok(())
    }

    pub fn current_session_path(&self) -> Option<&Path> {
        self.current_tsv_session.as_deref()
    }

    pub fn current_json_session_path(&self) -> Option<&Path> {
        self.current_json_session.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_session_manager() -> (SessionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = SessionManager::new(temp_dir.path());
        (manager, temp_dir)
    }

    #[test]
    fn test_new_session_manager() {
        let (manager, _temp) = create_temp_session_manager();
        assert!(manager.current_session_path().is_none());
        assert!(manager.current_json_session_path().is_none());
    }

    #[test]
    fn test_start_session() {
        let (mut manager, _temp) = create_temp_session_manager();
        let path = manager.start_session().unwrap();

        assert!(path.exists() || path.parent().unwrap().exists());
        assert!(manager.current_session_path().is_some());
        assert!(path.extension().unwrap() == "tsv");
    }

    #[test]
    fn test_start_json_session() {
        let (mut manager, _temp) = create_temp_session_manager();
        let path = manager.start_json_session().unwrap();

        assert!(path.exists());
        assert!(manager.current_json_session_path().is_some());
        assert!(path.extension().unwrap() == "json");

        // Verify JSON structure is an empty array
        let content = fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(json.is_array());
        assert!(json.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_append_line() {
        let (mut manager, _temp) = create_temp_session_manager();
        manager.start_session().unwrap();

        manager.append_line("test line 1").unwrap();
        manager.append_line("test line 2").unwrap();

        let path = manager.current_session_path().unwrap();
        let content = fs::read_to_string(path).unwrap();
        assert!(content.contains("test line 1"));
        assert!(content.contains("test line 2"));
    }

    #[test]
    fn test_append_line_without_session() {
        let (manager, _temp) = create_temp_session_manager();
        // Should not error even without active session
        let result = manager.append_line("test");
        assert!(result.is_ok());
    }
}
