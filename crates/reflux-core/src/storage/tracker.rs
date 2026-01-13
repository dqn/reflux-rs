use crate::error::Result;
use crate::game::{Difficulty, Grade, Lamp};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
        let content = fs::read_to_string(path)?;
        let mut tracker = Self::new();

        for line in content.lines() {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 6 {
                let song_id: u32 = parts[0].parse().unwrap_or(0);
                let difficulty =
                    Difficulty::from_u8(parts[1].parse().unwrap_or(0)).unwrap_or(Difficulty::SpN);
                let grade = Grade::from_u8(parts[2].parse().unwrap_or(0)).unwrap_or(Grade::NoPlay);
                let lamp = Lamp::from_u8(parts[3].parse().unwrap_or(0)).unwrap_or(Lamp::NoPlay);
                let ex_score: u32 = parts[4].parse().unwrap_or(0);
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
            }
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
