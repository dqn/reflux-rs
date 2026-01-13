use crate::config::LocalRecordConfig;
use crate::error::Result;
use crate::game::PlayData;
use crate::storage::format::{
    format_dynamic_tsv_header, format_dynamic_tsv_row, format_json_entry,
};
use chrono::{DateTime, Local};
use serde_json::{Value as JsonValue, json};
use std::fs::{self};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct SessionManager {
    base_dir: PathBuf,
    current_tsv_session: Option<PathBuf>,
    current_json_session: Option<PathBuf>,
    json_data: Option<JsonValue>,
}

impl SessionManager {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            current_tsv_session: None,
            current_json_session: None,
            json_data: None,
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

    /// Start a session with dynamic TSV header based on config
    pub fn start_session_with_header(&mut self, config: &LocalRecordConfig) -> Result<PathBuf> {
        let now: DateTime<Local> = Local::now();
        fs::create_dir_all(&self.base_dir)?;

        // TSV session file (C# compatible naming)
        let tsv_file = self
            .base_dir
            .join(format!("Session_{}.tsv", now.format("%Y_%m_%d_%H_%M_%S")));

        // Write header
        let header = format_dynamic_tsv_header(config);
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

        // Initialize JSON structure
        let json_data = json!({
            "head": {
                "service": "Infinitas",
                "game": "iidx"
            },
            "body": []
        });

        fs::write(&json_file, serde_json::to_string_pretty(&json_data)?)?;

        self.current_json_session = Some(json_file.clone());
        self.json_data = Some(json_data);

        Ok(json_file)
    }

    /// Append a TSV row to the session file
    pub fn append_tsv_row(&self, play_data: &PlayData, config: &LocalRecordConfig) -> Result<()> {
        if let Some(ref path) = self.current_tsv_session {
            let row = format_dynamic_tsv_row(play_data, config);
            let mut file = fs::OpenOptions::new().append(true).open(path)?;
            writeln!(file, "{}", row)?;
        }
        Ok(())
    }

    /// Append a JSON entry to the session file
    pub fn append_json_entry(&mut self, play_data: &PlayData) -> Result<()> {
        if let (Some(path), Some(json_data)) = (&self.current_json_session, &mut self.json_data) {
            let entry = format_json_entry(play_data);

            if let Some(body) = json_data.get_mut("body")
                && let Some(arr) = body.as_array_mut()
            {
                arr.push(entry);
            }

            fs::write(path, serde_json::to_string_pretty(json_data)?)?;
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
