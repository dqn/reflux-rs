use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::game::{Difficulty, Lamp, SongInfo};
use crate::memory::MemoryReader;

/// Score data for a single song (all difficulties)
#[derive(Debug, Clone, Default)]
pub struct ScoreData {
    pub song_id: u32,
    /// Lamp for each difficulty: SPB, SPN, SPH, SPA, SPL, DPB, DPN, DPH, DPA, DPL
    pub lamp: [Lamp; 10],
    /// EX Score for each difficulty
    pub score: [u32; 10],
    /// Miss count for each difficulty
    pub miss_count: [Option<u32>; 10],
    /// DJ Points for each difficulty
    pub dj_points: [f64; 10],
}

impl ScoreData {
    pub fn new(song_id: u32) -> Self {
        Self {
            song_id,
            ..Default::default()
        }
    }

    pub fn get_lamp(&self, difficulty: Difficulty) -> Lamp {
        self.lamp
            .get(difficulty as usize)
            .copied()
            .unwrap_or(Lamp::NoPlay)
    }

    pub fn get_score(&self, difficulty: Difficulty) -> u32 {
        self.score.get(difficulty as usize).copied().unwrap_or(0)
    }

    pub fn set_lamp(&mut self, difficulty: Difficulty, lamp: Lamp) {
        if let Some(slot) = self.lamp.get_mut(difficulty as usize) {
            *slot = lamp;
        }
    }

    pub fn set_score(&mut self, difficulty: Difficulty, score: u32) {
        if let Some(slot) = self.score.get_mut(difficulty as usize) {
            *slot = score;
        }
    }
}

/// A node in the INFINITAS score hashmap linked list
#[derive(Debug, Clone, Default)]
struct ListNode {
    next: u64,
    #[allow(dead_code)]
    prev: u64,
    diff: i32,
    song: i32,
    playtype: i32,
    score: u32,
    miss_count: u32,
    lamp: i32,
}

impl ListNode {
    const SIZE: usize = 64;

    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            next: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            prev: u64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            diff: i32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            song: i32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            playtype: i32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            // uk2 at 28-31
            score: u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]),
            miss_count: u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]),
            // uk3 at 40-43, uk4 at 44-47
            lamp: i32::from_le_bytes([bytes[48], bytes[49], bytes[50], bytes[51]]),
        }
    }

    fn key(&self) -> (u32, i32, i32) {
        (self.song as u32, self.diff, self.playtype)
    }
}

/// Map of song scores loaded from INFINITAS memory
#[derive(Debug, Clone, Default)]
pub struct ScoreMap {
    scores: HashMap<u32, ScoreData>,
}

impl ScoreMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load score map from INFINITAS memory
    pub fn load_from_memory(
        reader: &MemoryReader,
        data_map_addr: u64,
        song_db: &HashMap<u32, SongInfo>,
    ) -> Result<Self> {
        let mut nodes: HashMap<(u32, i32, i32), ListNode> = HashMap::new();

        // Read null object address (used to skip empty entries)
        let null_obj = reader.read_u64(data_map_addr.wrapping_sub(16))?;

        // Read start and end addresses of the hash table
        let start_address = reader.read_u64(data_map_addr)?;
        let end_address = reader.read_u64(data_map_addr + 8)?;

        if end_address <= start_address {
            return Ok(Self::new());
        }

        let buffer_size = (end_address - start_address) as usize;
        let buffer = reader.read_bytes(start_address, buffer_size)?;

        // Collect entry points from the hash table
        let mut entry_points = Vec::new();
        for i in 0..(buffer_size / 8) {
            let addr = u64::from_le_bytes([
                buffer[i * 8],
                buffer[i * 8 + 1],
                buffer[i * 8 + 2],
                buffer[i * 8 + 3],
                buffer[i * 8 + 4],
                buffer[i * 8 + 5],
                buffer[i * 8 + 6],
                buffer[i * 8 + 7],
            ]);

            // Skip null entries and magic number entries
            if addr != 0 && addr != null_obj && addr != 0x494fdce0 {
                entry_points.push(addr);
            }
        }

        // Follow linked lists from each entry point
        for entry_point in entry_points {
            Self::follow_linked_list(reader, entry_point, song_db, &mut nodes)?;
        }

        // Convert nodes to ScoreData
        let mut result = Self::new();
        for ((song_id, diff, playtype), node) in nodes {
            // Calculate difficulty index: diff + playtype * 5
            let difficulty_index = (diff + playtype * 5) as usize;
            if difficulty_index >= 10 {
                continue;
            }

            let score_data = result.get_or_insert(song_id);
            score_data.lamp[difficulty_index] =
                Lamp::from_u8(node.lamp as u8).unwrap_or(Lamp::NoPlay);
            score_data.score[difficulty_index] = node.score;
            // INFINITAS uses u32::MAX as sentinel value to indicate miss_count data is unavailable
            // (e.g., for legacy scores or when the game doesn't track this information)
            score_data.miss_count[difficulty_index] =
                if node.miss_count == u32::MAX { None } else { Some(node.miss_count) };
        }

        Ok(result)
    }

    fn follow_linked_list(
        reader: &MemoryReader,
        entry_point: u64,
        song_db: &HashMap<u32, SongInfo>,
        nodes: &mut HashMap<(u32, i32, i32), ListNode>,
    ) -> Result<()> {
        let mut visited: HashSet<u64> = HashSet::new();
        let mut current_addr = entry_point;

        loop {
            // Prevent infinite loops
            if visited.contains(&current_addr) {
                break;
            }
            visited.insert(current_addr);

            // Read node
            let buffer = reader.read_bytes(current_addr, ListNode::SIZE)?;
            let node = ListNode::from_bytes(&buffer);

            // Check if song exists in database
            let song_id = node.song as u32;
            if !song_db.contains_key(&song_id) {
                break;
            }

            let key = node.key();
            if let std::collections::hash_map::Entry::Vacant(e) = nodes.entry(key) {
                let next_addr = node.next;
                e.insert(node);
                current_addr = next_addr;
            } else {
                break;
            }
        }

        Ok(())
    }

    pub fn get(&self, song_id: u32) -> Option<&ScoreData> {
        self.scores.get(&song_id)
    }

    pub fn get_mut(&mut self, song_id: u32) -> Option<&mut ScoreData> {
        self.scores.get_mut(&song_id)
    }

    pub fn insert(&mut self, song_id: u32, data: ScoreData) {
        self.scores.insert(song_id, data);
    }

    pub fn get_or_insert(&mut self, song_id: u32) -> &mut ScoreData {
        self.scores.entry(song_id).or_insert_with(|| ScoreData::new(song_id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (&u32, &ScoreData)> {
        self.scores.iter()
    }

    pub fn len(&self) -> usize {
        self.scores.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scores.is_empty()
    }
}
