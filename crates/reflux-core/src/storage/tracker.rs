use crate::error::Result;
use crate::game::{Difficulty, Grade, Lamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::warn;

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

        // Error tracking for summary
        let mut total_lines = 0usize;
        let mut parsed_lines = 0usize;
        let mut skipped_lines = 0usize;
        let mut field_errors = 0usize;
        let mut logged_errors = 0usize;

        // Helper to log errors with limit
        let mut log_error = |msg: String| {
            if logged_errors < MAX_DETAILED_ERRORS {
                warn!("{}", msg);
                logged_errors += 1;
            }
        };

        for (line_num, line) in content.lines().enumerate() {
            total_lines += 1;
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 6 {
                let song_id: u32 = match parts[0].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        log_error(format!(
                            "tracker.txt line {}: failed to parse song_id '{}'",
                            line_num + 1,
                            parts[0]
                        ));
                        skipped_lines += 1;
                        continue;
                    }
                };
                let difficulty = match parts[1].parse::<u8>() {
                    Ok(v) => Difficulty::from_u8(v).unwrap_or_else(|| {
                        log_error(format!(
                            "tracker.txt line {}: invalid difficulty value {}",
                            line_num + 1,
                            v
                        ));
                        field_errors += 1;
                        Difficulty::SpN
                    }),
                    Err(_) => {
                        log_error(format!(
                            "tracker.txt line {}: failed to parse difficulty '{}'",
                            line_num + 1,
                            parts[1]
                        ));
                        skipped_lines += 1;
                        continue;
                    }
                };
                let grade = match parts[2].parse::<u8>() {
                    Ok(v) => Grade::from_u8(v).unwrap_or_else(|| {
                        log_error(format!(
                            "tracker.txt line {}: invalid grade value {}",
                            line_num + 1,
                            v
                        ));
                        field_errors += 1;
                        Grade::NoPlay
                    }),
                    Err(_) => {
                        log_error(format!(
                            "tracker.txt line {}: failed to parse grade '{}'",
                            line_num + 1,
                            parts[2]
                        ));
                        field_errors += 1;
                        Grade::NoPlay
                    }
                };
                let lamp = match parts[3].parse::<u8>() {
                    Ok(v) => Lamp::from_u8(v).unwrap_or_else(|| {
                        log_error(format!(
                            "tracker.txt line {}: invalid lamp value {}",
                            line_num + 1,
                            v
                        ));
                        field_errors += 1;
                        Lamp::NoPlay
                    }),
                    Err(_) => {
                        log_error(format!(
                            "tracker.txt line {}: failed to parse lamp '{}'",
                            line_num + 1,
                            parts[3]
                        ));
                        field_errors += 1;
                        Lamp::NoPlay
                    }
                };
                let ex_score: u32 = match parts[4].parse() {
                    Ok(v) => v,
                    Err(_) => {
                        log_error(format!(
                            "tracker.txt line {}: failed to parse ex_score '{}', using 0",
                            line_num + 1,
                            parts[4]
                        ));
                        field_errors += 1;
                        0
                    }
                };
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
                tracker.db.insert(key, info);
                parsed_lines += 1;
            } else {
                skipped_lines += 1;
            }
        }

        // Report parse summary if there were any issues
        let total_errors = skipped_lines + field_errors;
        if total_errors > 0 {
            warn!(
                "Tracker load summary: {} total lines, {} parsed, {} skipped, {} field errors{}",
                total_lines,
                parsed_lines,
                skipped_lines,
                field_errors,
                if total_errors > MAX_DETAILED_ERRORS {
                    format!(" ({} errors not shown)", total_errors - logged_errors)
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

        fs::write(path, lines.join("\n"))?;
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
