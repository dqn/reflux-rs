use crate::error::Result;
use crate::game::{Difficulty, Grade, Lamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::warn;

/// Statistics for tracker file parsing
#[derive(Debug, Default)]
struct ParseStats {
    total_lines: usize,
    parsed_lines: usize,
    skipped_lines: usize,
    field_errors: usize,
    logged_errors: usize,
}

impl ParseStats {
    fn total_errors(&self) -> usize {
        self.skipped_lines + self.field_errors
    }
}

/// Error type for tracker line parsing
#[derive(Debug)]
enum TrackerParseError {
    FieldCount,
    SongId(String),
    Difficulty(String),
    Grade(String),
    Lamp(String),
    ExScore(String),
}

/// Parse a single tracker line into ChartKey and TrackerInfo
fn parse_tracker_line(
    line: &str,
) -> std::result::Result<(ChartKey, TrackerInfo), TrackerParseError> {
    let parts: Vec<&str> = line.split(',').collect();
    if parts.len() < 6 {
        return Err(TrackerParseError::FieldCount);
    }

    let song_id: u32 = parts[0]
        .parse()
        .map_err(|_| TrackerParseError::SongId(parts[0].to_string()))?;

    let difficulty = parts[1]
        .parse::<u8>()
        .map_err(|_| TrackerParseError::Difficulty(parts[1].to_string()))
        .and_then(|v| {
            Difficulty::from_u8(v).ok_or_else(|| TrackerParseError::Difficulty(v.to_string()))
        })?;

    let grade = parts[2]
        .parse::<u8>()
        .map_err(|_| TrackerParseError::Grade(parts[2].to_string()))
        .and_then(|v| Grade::from_u8(v).ok_or_else(|| TrackerParseError::Grade(v.to_string())))?;

    let lamp = parts[3]
        .parse::<u8>()
        .map_err(|_| TrackerParseError::Lamp(parts[3].to_string()))
        .and_then(|v| Lamp::from_u8(v).ok_or_else(|| TrackerParseError::Lamp(v.to_string())))?;

    let ex_score: u32 = parts[4]
        .parse()
        .map_err(|_| TrackerParseError::ExScore(parts[4].to_string()))?;

    let miss_count: Option<u32> = parts[5].parse().ok();

    let key = ChartKey {
        song_id,
        difficulty,
    };
    let info = TrackerInfo {
        grade,
        lamp,
        ex_score,
        miss_count,
        dj_points: 0.0,
    };

    Ok((key, info))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackerInfo {
    pub grade: Grade,
    pub lamp: Lamp,
    pub ex_score: u32,
    pub miss_count: Option<u32>,
    pub dj_points: f64,
}

impl TrackerInfo {
    pub fn update(&mut self, other: &TrackerInfo) {
        if other.lamp > self.lamp {
            self.lamp = other.lamp;
        }
        if other.grade > self.grade {
            self.grade = other.grade;
        }
        if other.ex_score > self.ex_score {
            self.ex_score = other.ex_score;
        }
        if let Some(miss) = other.miss_count {
            match self.miss_count {
                Some(current) if miss < current => self.miss_count = Some(miss),
                None => self.miss_count = Some(miss),
                _ => {}
            }
        }
        if other.dj_points > self.dj_points {
            self.dj_points = other.dj_points;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChartKey {
    pub song_id: u32,
    pub difficulty: Difficulty,
}

#[derive(Debug, Clone, Default)]
pub struct Tracker {
    db: HashMap<ChartKey, TrackerInfo>,
}

impl Tracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        const MAX_DETAILED_ERRORS: usize = 10;

        let content = fs::read_to_string(path)?;
        let mut tracker = Self::new();
        let mut stats = ParseStats::default();

        for (line_num, line) in content.lines().enumerate() {
            stats.total_lines += 1;

            match parse_tracker_line(line) {
                Ok((key, info)) => {
                    tracker.db.insert(key, info);
                    stats.parsed_lines += 1;
                }
                Err(e) => {
                    stats.skipped_lines += 1;
                    if let TrackerParseError::FieldCount = e {
                        // Lines with insufficient fields are silently skipped
                        continue;
                    }

                    stats.field_errors += 1;
                    if stats.logged_errors < MAX_DETAILED_ERRORS {
                        let msg = match e {
                            TrackerParseError::FieldCount => unreachable!(),
                            TrackerParseError::SongId(v) => {
                                format!("failed to parse song_id '{}'", v)
                            }
                            TrackerParseError::Difficulty(v) => {
                                format!("invalid difficulty value '{}'", v)
                            }
                            TrackerParseError::Grade(v) => {
                                format!("invalid grade value '{}'", v)
                            }
                            TrackerParseError::Lamp(v) => {
                                format!("invalid lamp value '{}'", v)
                            }
                            TrackerParseError::ExScore(v) => {
                                format!("failed to parse ex_score '{}'", v)
                            }
                        };
                        warn!("tracker.txt line {}: {}", line_num + 1, msg);
                        stats.logged_errors += 1;
                    }
                }
            }
        }

        // Report parse summary if there were any issues
        if stats.total_errors() > 0 {
            let hidden = stats.total_errors().saturating_sub(stats.logged_errors);
            warn!(
                "Tracker load summary: {} total lines, {} parsed, {} skipped, {} field errors{}",
                stats.total_lines,
                stats.parsed_lines,
                stats.skipped_lines,
                stats.field_errors,
                if hidden > 0 {
                    format!(" ({} errors not shown)", hidden)
                } else {
                    String::new()
                }
            );
        }

        Ok(tracker)
    }

    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let mut lines = Vec::new();

        for (key, info) in &self.db {
            let miss_str = info
                .miss_count
                .map(|m| m.to_string())
                .unwrap_or_else(|| "-".to_string());
            lines.push(format!(
                "{},{},{},{},{},{}",
                key.song_id,
                key.difficulty as u8,
                info.grade as u8,
                info.lamp as u8,
                info.ex_score,
                miss_str
            ));
        }

        // Atomic write: write to temp file then rename
        let tmp_path = path.as_ref().with_extension("tmp");
        fs::write(&tmp_path, lines.join("\n"))?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn get(&self, key: &ChartKey) -> Option<&TrackerInfo> {
        self.db.get(key)
    }

    pub fn update(&mut self, key: ChartKey, info: TrackerInfo) {
        self.db
            .entry(key)
            .and_modify(|existing| existing.update(&info))
            .or_insert(info);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ChartKey, &TrackerInfo)> {
        self.db.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tracker_info_update_lamp() {
        let mut info = TrackerInfo {
            grade: Grade::B,
            lamp: Lamp::Clear,
            ex_score: 1000,
            miss_count: Some(5),
            dj_points: 50.0,
        };

        let other = TrackerInfo {
            grade: Grade::C,
            lamp: Lamp::HardClear,
            ex_score: 900,
            miss_count: Some(10),
            dj_points: 40.0,
        };

        info.update(&other);

        // Only lamp should be updated (HardClear > Clear)
        assert_eq!(info.lamp, Lamp::HardClear);
        assert_eq!(info.grade, Grade::B); // B > C, no change
        assert_eq!(info.ex_score, 1000); // 1000 > 900, no change
        assert_eq!(info.miss_count, Some(5)); // 5 < 10, no change
        assert_eq!(info.dj_points, 50.0); // 50 > 40, no change
    }

    #[test]
    fn test_tracker_info_update_all_better() {
        let mut info = TrackerInfo {
            grade: Grade::C,
            lamp: Lamp::Clear,
            ex_score: 500,
            miss_count: Some(10),
            dj_points: 30.0,
        };

        let other = TrackerInfo {
            grade: Grade::A,
            lamp: Lamp::ExHardClear,
            ex_score: 1500,
            miss_count: Some(2),
            dj_points: 80.0,
        };

        info.update(&other);

        assert_eq!(info.lamp, Lamp::ExHardClear);
        assert_eq!(info.grade, Grade::A);
        assert_eq!(info.ex_score, 1500);
        assert_eq!(info.miss_count, Some(2));
        assert_eq!(info.dj_points, 80.0);
    }

    #[test]
    fn test_tracker_info_update_miss_count_none_to_some() {
        let mut info = TrackerInfo {
            miss_count: None,
            ..Default::default()
        };

        let other = TrackerInfo {
            miss_count: Some(5),
            ..Default::default()
        };

        info.update(&other);
        assert_eq!(info.miss_count, Some(5));
    }

    #[test]
    fn test_tracker_new() {
        let tracker = Tracker::new();
        let key = ChartKey {
            song_id: 1000,
            difficulty: Difficulty::SpA,
        };
        assert!(tracker.get(&key).is_none());
    }

    #[test]
    fn test_tracker_update_and_get() {
        let mut tracker = Tracker::new();
        let key = ChartKey {
            song_id: 1000,
            difficulty: Difficulty::SpA,
        };
        let info = TrackerInfo {
            grade: Grade::A,
            lamp: Lamp::HardClear,
            ex_score: 2000,
            miss_count: Some(3),
            dj_points: 75.0,
        };

        tracker.update(key, info.clone());

        let retrieved = tracker.get(&key).unwrap();
        assert_eq!(retrieved.grade, Grade::A);
        assert_eq!(retrieved.lamp, Lamp::HardClear);
        assert_eq!(retrieved.ex_score, 2000);
    }

    #[test]
    fn test_tracker_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("tracker.txt");

        let mut tracker = Tracker::new();
        let key = ChartKey {
            song_id: 1000,
            difficulty: Difficulty::SpA,
        };
        let info = TrackerInfo {
            grade: Grade::A,
            lamp: Lamp::HardClear,
            ex_score: 2000,
            miss_count: Some(3),
            dj_points: 75.0,
        };
        tracker.update(key, info);

        tracker.save(&path).unwrap();

        let loaded = Tracker::load(&path).unwrap();
        let loaded_info = loaded.get(&key).unwrap();

        assert_eq!(loaded_info.grade, Grade::A);
        assert_eq!(loaded_info.lamp, Lamp::HardClear);
        assert_eq!(loaded_info.ex_score, 2000);
        assert_eq!(loaded_info.miss_count, Some(3));
    }

    #[test]
    fn test_tracker_iter() {
        let mut tracker = Tracker::new();

        for i in 0..3 {
            let key = ChartKey {
                song_id: 1000 + i,
                difficulty: Difficulty::SpN,
            };
            tracker.update(
                key,
                TrackerInfo {
                    ex_score: i * 100,
                    ..Default::default()
                },
            );
        }

        let count = tracker.iter().count();
        assert_eq!(count, 3);
    }
}
