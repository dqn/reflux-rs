use crate::process::ReadMemory;
use crate::offset::OffsetsCollection;
use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// Offset dump for diagnostic purposes
#[derive(Debug, Clone, Serialize)]
pub struct OffsetDump {
    pub version: String,
    pub base_address: String,
    pub offsets: OffsetValues,
    pub relations: OffsetRelations,
    pub memory_samples: MemorySamples,
    pub data_map_diagnostics: Option<DataMapDiagnostics>,
}

/// Offset values in hex string format
#[derive(Debug, Clone, Serialize)]
pub struct OffsetValues {
    pub song_list: String,
    pub unlock_data: String,
    pub data_map: String,
    pub judge_data: String,
    pub play_settings: String,
    pub play_data: String,
    pub current_song: String,
}

/// Relative distances between offsets (signed, in bytes)
#[derive(Debug, Clone, Serialize)]
pub struct OffsetRelations {
    pub song_list_from_base: i64,
    pub unlock_data_from_song_list: i64,
    pub data_map_from_song_list: i64,
    pub judge_data_from_data_map: i64,
    pub play_settings_from_judge_data: i64,
    pub play_data_from_play_settings: i64,
    pub current_song_from_play_settings: i64,
}

/// Memory samples at each offset location
#[derive(Debug, Clone, Serialize)]
pub struct MemorySamples {
    pub play_data_32bytes: String,
    pub current_song_32bytes: String,
    pub judge_data_32bytes: String,
    pub play_settings_32bytes: String,
}

/// DataMap diagnostics for verification
#[derive(Debug, Clone, Serialize)]
pub struct DataMapDiagnostics {
    pub status: String,
    pub null_obj: String,
    pub table_start: String,
    pub table_end: String,
    pub table_size: usize,
    pub scanned_entries: usize,
    pub non_null_entries: usize,
    pub valid_node_samples: usize,
}

impl OffsetDump {
    /// Create a dump from offsets and memory reader
    pub fn from_offsets<R: ReadMemory>(offsets: &OffsetsCollection, base: u64, reader: &R) -> Self {
        let offset_values = OffsetValues {
            song_list: format!("0x{:X}", offsets.song_list),
            unlock_data: format!("0x{:X}", offsets.unlock_data),
            data_map: format!("0x{:X}", offsets.data_map),
            judge_data: format!("0x{:X}", offsets.judge_data),
            play_settings: format!("0x{:X}", offsets.play_settings),
            play_data: format!("0x{:X}", offsets.play_data),
            current_song: format!("0x{:X}", offsets.current_song),
        };

        let relations = OffsetRelations {
            song_list_from_base: offsets.song_list as i64 - base as i64,
            unlock_data_from_song_list: offsets.unlock_data as i64 - offsets.song_list as i64,
            data_map_from_song_list: offsets.data_map as i64 - offsets.song_list as i64,
            judge_data_from_data_map: offsets.judge_data as i64 - offsets.data_map as i64,
            play_settings_from_judge_data: offsets.play_settings as i64 - offsets.judge_data as i64,
            play_data_from_play_settings: offsets.play_data as i64 - offsets.play_settings as i64,
            current_song_from_play_settings: offsets.current_song as i64
                - offsets.play_settings as i64,
        };

        let memory_samples = MemorySamples {
            play_data_32bytes: Self::read_memory_hex(reader, offsets.play_data, 32),
            current_song_32bytes: Self::read_memory_hex(reader, offsets.current_song, 32),
            judge_data_32bytes: Self::read_memory_hex(reader, offsets.judge_data, 32),
            play_settings_32bytes: Self::read_memory_hex(reader, offsets.play_settings, 32),
        };

        Self {
            version: offsets.version.clone(),
            base_address: format!("0x{:X}", base),
            offsets: offset_values,
            relations,
            memory_samples,
            data_map_diagnostics: Self::data_map_diagnostics(reader, offsets.data_map, base),
        }
    }

    fn read_memory_hex<R: ReadMemory>(reader: &R, address: u64, size: usize) -> String {
        if address == 0 {
            return "(address is 0)".to_string();
        }

        match reader.read_bytes(address, size) {
            Ok(bytes) => bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" "),
            Err(_) => "(read failed)".to_string(),
        }
    }

    fn data_map_diagnostics<R: ReadMemory>(
        reader: &R,
        data_map_addr: u64,
        base: u64,
    ) -> Option<DataMapDiagnostics> {
        if data_map_addr == 0 {
            return None;
        }

        let null_obj = match reader.read_u64(data_map_addr.wrapping_sub(16)) {
            Ok(value) => value,
            Err(e) => {
                return Some(DataMapDiagnostics {
                    status: format!("read failed: {}", e),
                    null_obj: "(read failed)".to_string(),
                    table_start: "(read failed)".to_string(),
                    table_end: "(read failed)".to_string(),
                    table_size: 0,
                    scanned_entries: 0,
                    non_null_entries: 0,
                    valid_node_samples: 0,
                });
            }
        };

        let table_start = match reader.read_u64(data_map_addr) {
            Ok(value) => value,
            Err(e) => {
                return Some(DataMapDiagnostics {
                    status: format!("read failed: {}", e),
                    null_obj: format!("0x{:X}", null_obj),
                    table_start: "(read failed)".to_string(),
                    table_end: "(read failed)".to_string(),
                    table_size: 0,
                    scanned_entries: 0,
                    non_null_entries: 0,
                    valid_node_samples: 0,
                });
            }
        };

        let table_end = match reader.read_u64(data_map_addr + 8) {
            Ok(value) => value,
            Err(e) => {
                return Some(DataMapDiagnostics {
                    status: format!("read failed: {}", e),
                    null_obj: format!("0x{:X}", null_obj),
                    table_start: format!("0x{:X}", table_start),
                    table_end: "(read failed)".to_string(),
                    table_size: 0,
                    scanned_entries: 0,
                    non_null_entries: 0,
                    valid_node_samples: 0,
                });
            }
        };

        if table_end <= table_start {
            return Some(DataMapDiagnostics {
                status: "invalid range (end <= start)".to_string(),
                null_obj: format!("0x{:X}", null_obj),
                table_start: format!("0x{:X}", table_start),
                table_end: format!("0x{:X}", table_end),
                table_size: 0,
                scanned_entries: 0,
                non_null_entries: 0,
                valid_node_samples: 0,
            });
        }

        let below_base = table_start < base || table_end < base;

        let table_size = (table_end - table_start) as usize;
        let scan_size = table_size.min(0x4000);

        let buffer = match reader.read_bytes(table_start, scan_size) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Some(DataMapDiagnostics {
                    status: format!("read failed: {}", e),
                    null_obj: format!("0x{:X}", null_obj),
                    table_start: format!("0x{:X}", table_start),
                    table_end: format!("0x{:X}", table_end),
                    table_size,
                    scanned_entries: 0,
                    non_null_entries: 0,
                    valid_node_samples: 0,
                });
            }
        };

        let scanned_entries = buffer.len() / 8;
        let mut non_null_entries = 0usize;
        let mut entry_points = Vec::new();

        for i in 0..scanned_entries {
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

            if addr != 0 && addr != null_obj && addr != 0x494fdce0 {
                non_null_entries += 1;
                entry_points.push(addr);
            }
        }

        let mut valid_node_samples = 0usize;
        for entry in entry_points.iter().take(32) {
            if Self::validate_data_map_node(reader, *entry) {
                valid_node_samples += 1;
            }
        }

        let status = if below_base {
            "ok (below base)".to_string()
        } else {
            "ok".to_string()
        };

        Some(DataMapDiagnostics {
            status,
            null_obj: format!("0x{:X}", null_obj),
            table_start: format!("0x{:X}", table_start),
            table_end: format!("0x{:X}", table_end),
            table_size,
            scanned_entries,
            non_null_entries,
            valid_node_samples,
        })
    }

    fn validate_data_map_node<R: ReadMemory>(reader: &R, addr: u64) -> bool {
        let buffer = match reader.read_bytes(addr, 64) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        if buffer.len() < 52 {
            return false;
        }

        let diff = i32::from_le_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
        let song_id = i32::from_le_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
        let playtype = i32::from_le_bytes([buffer[24], buffer[25], buffer[26], buffer[27]]);
        let score = u32::from_le_bytes([buffer[32], buffer[33], buffer[34], buffer[35]]);
        let miss_count = u32::from_le_bytes([buffer[36], buffer[37], buffer[38], buffer[39]]);
        let lamp = i32::from_le_bytes([buffer[48], buffer[49], buffer[50], buffer[51]]);

        if !(0..=4).contains(&diff) {
            return false;
        }
        if !(0..=1).contains(&playtype) {
            return false;
        }
        if !(1000..=50000).contains(&song_id) {
            return false;
        }
        if score > 200_000 {
            return false;
        }
        if miss_count > 10_000 && miss_count != u32::MAX {
            return false;
        }
        if !(0..=7).contains(&lamp) {
            return false;
        }

        true
    }

    /// Save dump to JSON file
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
    }
}
