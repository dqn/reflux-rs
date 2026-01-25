use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSignature {
    pub pattern: String,
    pub instr_offset: usize,
    pub disp_offset: usize,
    pub instr_len: usize,
    #[serde(default)]
    pub deref: bool,
    #[serde(default)]
    pub addend: i64,
}

impl CodeSignature {
    pub fn pattern_bytes(&self) -> Result<Vec<Option<u8>>> {
        parse_pattern(&self.pattern)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetSignatureEntry {
    pub name: String,
    pub signatures: Vec<CodeSignature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetSignatureSet {
    pub version: String,
    pub entries: Vec<OffsetSignatureEntry>,
}

impl OffsetSignatureSet {
    pub fn entry(&self, name: &str) -> Option<&OffsetSignatureEntry> {
        self.entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }
}

pub fn load_signatures<P: AsRef<Path>>(path: P) -> Result<OffsetSignatureSet> {
    let content = fs::read_to_string(&path)?;
    let data = serde_json::from_str(&content)?;
    Ok(data)
}

pub fn save_signatures<P: AsRef<Path>>(path: P, signatures: &OffsetSignatureSet) -> Result<()> {
    let content = serde_json::to_string_pretty(signatures)?;
    fs::write(path, content)?;
    Ok(())
}

pub fn parse_pattern(pattern: &str) -> Result<Vec<Option<u8>>> {
    let mut bytes = Vec::new();
    for token in pattern.split_whitespace() {
        if token == "??" || token == "?" {
            bytes.push(None);
            continue;
        }

        let value = u8::from_str_radix(token, 16).map_err(|e| {
            Error::InvalidOffset(format!("Invalid signature token '{}': {}", token, e))
        })?;
        bytes.push(Some(value));
    }

    if bytes.is_empty() {
        return Err(Error::InvalidOffset(
            "Signature pattern is empty".to_string(),
        ));
    }

    Ok(bytes)
}

pub fn format_pattern(bytes: &[Option<u8>]) -> String {
    bytes
        .iter()
        .map(|b| match b {
            Some(value) => format!("{:02X}", value),
            None => "??".to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn builtin_signatures() -> OffsetSignatureSet {
    OffsetSignatureSet {
        version: "*".to_string(),
        entries: vec![
            OffsetSignatureEntry {
                name: "songList".to_string(),
                signatures: vec![
                    CodeSignature {
                        pattern: "48 8D 15 ?? ?? ?? ?? 48 8D 64 24 F8 48 89 D7".to_string(),
                        instr_offset: 0,
                        disp_offset: 3,
                        instr_len: 7,
                        deref: false,
                        addend: 0x1A34,
                    },
                    CodeSignature {
                        pattern: "48 8D 1D ?? ?? ?? ?? 45 19 CB".to_string(),
                        instr_offset: 0,
                        disp_offset: 3,
                        instr_len: 7,
                        deref: false,
                        addend: -0x32F9,
                    },
                    CodeSignature {
                        pattern: "8D 3D ?? ?? ?? ?? 45 89".to_string(),
                        instr_offset: 0,
                        disp_offset: 2,
                        instr_len: 6,
                        deref: false,
                        addend: -0xD5BC,
                    },
                ],
            },
            OffsetSignatureEntry {
                name: "judgeData".to_string(),
                signatures: vec![CodeSignature {
                    pattern: "48 89 05 ?? ?? ?? ?? 48 89 05 9F 23 80 02 48 89".to_string(),
                    instr_offset: 0,
                    disp_offset: 3,
                    instr_len: 7,
                    deref: false,
                    addend: 0,
                }],
            },
            OffsetSignatureEntry {
                name: "playSettings".to_string(),
                signatures: vec![CodeSignature {
                    pattern: "89 2D ?? ?? ?? ?? EB 0C 48 8D 0D 0F ED 45 02 E8".to_string(),
                    instr_offset: 0,
                    disp_offset: 2,
                    instr_len: 6,
                    deref: false,
                    addend: 0x4,
                }],
            },
            OffsetSignatureEntry {
                name: "playData".to_string(),
                signatures: vec![CodeSignature {
                    pattern: "44 89 25 ?? ?? ?? ?? 8B 00 89 05 64 D5 3D 02 8B".to_string(),
                    instr_offset: 0,
                    disp_offset: 3,
                    instr_len: 7,
                    deref: false,
                    addend: 0xC00,
                }],
            },
            OffsetSignatureEntry {
                name: "currentSong".to_string(),
                signatures: vec![
                    CodeSignature {
                        pattern: "48 89 05 ?? ?? ?? ?? 48 89 05 A7 12".to_string(),
                        instr_offset: 0,
                        disp_offset: 3,
                        instr_len: 7,
                        deref: false,
                        addend: -0x44,
                    },
                    CodeSignature {
                        pattern: "48 89 05 ?? ?? ?? ?? 48 89 05 A8 12".to_string(),
                        instr_offset: 0,
                        disp_offset: 3,
                        instr_len: 7,
                        deref: false,
                        addend: -0x4C,
                    },
                    CodeSignature {
                        pattern: "89 05 ?? ?? ?? ?? 88 05 92".to_string(),
                        instr_offset: 0,
                        disp_offset: 2,
                        instr_len: 6,
                        deref: false,
                        addend: -0x54,
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pattern_with_wildcards() {
        let bytes = parse_pattern("48 8D 0D ?? ?? ?? ??").unwrap();
        assert_eq!(bytes.len(), 7);
        assert_eq!(bytes[0], Some(0x48));
        assert_eq!(bytes[1], Some(0x8D));
        assert_eq!(bytes[2], Some(0x0D));
        assert_eq!(bytes[3], None);
    }

    #[test]
    fn test_format_pattern_roundtrip() {
        let pattern = vec![Some(0x48), Some(0x8D), Some(0x0D), None, Some(0xFF)];
        let formatted = format_pattern(&pattern);
        assert_eq!(formatted, "48 8D 0D ?? FF");
        let parsed = parse_pattern(&formatted).unwrap();
        assert_eq!(parsed, pattern);
    }
}
