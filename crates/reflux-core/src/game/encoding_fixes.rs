use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::Result;

/// Database of encoding fixes for song titles and artists
///
/// Some Shift-JIS strings don't decode properly due to special characters.
/// This database maps broken strings to their correct representations.
#[derive(Debug, Clone, Default)]
pub struct EncodingFixes {
    fixes: HashMap<String, String>,
    version: String,
}

impl EncodingFixes {
    /// Create a new empty encoding fixes database
    pub fn new() -> Self {
        Self::default()
    }

    /// Load encoding fixes from a file
    ///
    /// File format:
    /// - First line: version (YYYYMMDD)
    /// - Other lines: broken_string\tcorrect_string
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut fixes = HashMap::new();
        let mut version = String::new();
        let mut first_line = true;

        for line in content.lines() {
            if first_line {
                // First line is version string
                version = line.trim().to_string();
                first_line = false;
                continue;
            }

            // Skip lines without tab separator
            if !line.contains('\t') {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                fixes.insert(parts[0].to_string(), parts[1].trim().to_string());
            }
        }

        Ok(Self { fixes, version })
    }

    /// Apply encoding fixes to a string
    ///
    /// Returns the fixed string if a fix exists, otherwise returns the original.
    pub fn apply(&self, text: &str) -> String {
        self.fixes
            .get(text)
            .cloned()
            .unwrap_or_else(|| text.to_string())
    }

    /// Check if a fix exists for the given text
    pub fn has_fix(&self, text: &str) -> bool {
        self.fixes.contains_key(text)
    }

    /// Get the version of the encoding fixes database
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get the number of fixes in the database
    pub fn len(&self) -> usize {
        self.fixes.len()
    }

    /// Check if the database is empty
    pub fn is_empty(&self) -> bool {
        self.fixes.is_empty()
    }

    /// Add a fix to the database
    pub fn add_fix(&mut self, broken: String, correct: String) {
        self.fixes.insert(broken, correct);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_fix() {
        let mut fixes = EncodingFixes::new();
        fixes.add_fix("?Viva!".to_string(), "¡Viva!".to_string());
        fixes.add_fix("fffff".to_string(), "ƒƒƒƒƒ".to_string());

        assert_eq!(fixes.apply("?Viva!"), "¡Viva!");
        assert_eq!(fixes.apply("fffff"), "ƒƒƒƒƒ");
        assert_eq!(fixes.apply("normal"), "normal");
    }

    #[test]
    fn test_has_fix() {
        let mut fixes = EncodingFixes::new();
        fixes.add_fix("broken".to_string(), "fixed".to_string());

        assert!(fixes.has_fix("broken"));
        assert!(!fixes.has_fix("normal"));
    }
}
