use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::Result;
use crate::game::UnlockData;

/// Persistent storage for unlock states
/// Used to track changes between sessions and sync with remote server
#[derive(Debug, Clone, Default)]
pub struct UnlockDb {
    /// Map of song_id -> (unlock_type as i32, unlocks bitmask)
    entries: HashMap<u32, (i32, i32)>,
}

impl UnlockDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load unlock database from file
    /// Format: songid,unlock_type,unlocks (CSV, one entry per line)
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    /// Parse unlock database from string content
    pub fn parse(content: &str) -> Result<Self> {
        let mut entries = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                let song_id: u32 = parts[0].trim().parse().unwrap_or(0);
                let unlock_type: i32 = parts[1].trim().parse().unwrap_or(0);
                let unlocks: i32 = parts[2].trim().parse().unwrap_or(0);
                entries.insert(song_id, (unlock_type, unlocks));
            }
        }

        Ok(Self { entries })
    }

    /// Save unlock database to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut lines = Vec::new();

        for (&song_id, (unlock_type, unlocks)) in &self.entries {
            lines.push(format!("{:05},{},{}", song_id, unlock_type, unlocks));
        }

        fs::write(path, lines.join("\n"))?;
        Ok(())
    }

    /// Get entry for a song
    pub fn get(&self, song_id: u32) -> Option<(i32, i32)> {
        self.entries.get(&song_id).copied()
    }

    /// Check if song exists in database
    pub fn contains(&self, song_id: u32) -> bool {
        self.entries.contains_key(&song_id)
    }

    /// Update or insert an entry
    pub fn update(&mut self, song_id: u32, unlock_type: i32, unlocks: i32) {
        self.entries.insert(song_id, (unlock_type, unlocks));
    }

    /// Update from UnlockData
    pub fn update_from_data(&mut self, song_id: u32, data: &UnlockData) {
        let unlock_type = match data.unlock_type {
            crate::game::UnlockType::Base => 1,
            crate::game::UnlockType::Bits => 2,
            crate::game::UnlockType::Sub => 3,
        };
        self.update(song_id, unlock_type, data.unlocks);
    }

    /// Check if unlock type changed
    pub fn has_unlock_type_changed(&self, song_id: u32, new_type: i32) -> bool {
        if let Some((old_type, _)) = self.get(song_id) {
            old_type != new_type
        } else {
            false
        }
    }

    /// Check if unlock state changed
    pub fn has_unlocks_changed(&self, song_id: u32, new_unlocks: i32) -> bool {
        if let Some((_, old_unlocks)) = self.get(song_id) {
            old_unlocks != new_unlocks
        } else {
            false
        }
    }

    /// Get all entries
    pub fn iter(&self) -> impl Iterator<Item = (&u32, &(i32, i32))> {
        self.entries.iter()
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unlock_db() {
        let content = r#"01000,1,31
02000,2,15
03000,3,255"#;

        let db = UnlockDb::parse(content).unwrap();

        assert_eq!(db.get(1000), Some((1, 31)));
        assert_eq!(db.get(2000), Some((2, 15)));
        assert_eq!(db.get(3000), Some((3, 255)));
        assert_eq!(db.get(99999), None);
    }

    #[test]
    fn test_update_and_save() {
        let mut db = UnlockDb::new();
        db.update(1000, 1, 31);
        db.update(2000, 2, 15);

        assert_eq!(db.len(), 2);
        assert!(db.contains(1000));
        assert!(!db.contains(99999));
    }

    #[test]
    fn test_change_detection() {
        let mut db = UnlockDb::new();
        db.update(1000, 1, 31);

        assert!(db.has_unlock_type_changed(1000, 2));
        assert!(!db.has_unlock_type_changed(1000, 1));
        assert!(db.has_unlocks_changed(1000, 63));
        assert!(!db.has_unlocks_changed(1000, 31));
    }
}
