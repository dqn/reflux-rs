use crate::error::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub update: UpdateConfig,
    pub record: RecordConfig,
    pub remote_record: RemoteRecordConfig,
    pub local_record: LocalRecordConfig,
    pub livestream: LivestreamConfig,
    pub debug: DebugConfig,
}

#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub update_files: bool,
    pub update_server: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            update_files: true,
            update_server: String::from(
                "https://raw.githubusercontent.com/olji/Reflux/master/Reflux",
            ),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RecordConfig {
    pub save_remote: bool,
    pub save_local: bool,
    pub save_json: bool,
    pub save_latest_json: bool,
    pub save_latest_txt: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RemoteRecordConfig {
    pub server_address: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct LocalRecordConfig {
    pub song_info: bool,
    pub chart_details: bool,
    pub result_details: bool,
    pub judge: bool,
    pub settings: bool,
}

#[derive(Debug, Clone)]
pub struct LivestreamConfig {
    pub show_play_state: bool,
    pub enable_marquee: bool,
    pub enable_full_song_info: bool,
    pub marquee_idle_text: String,
}

impl Default for LivestreamConfig {
    fn default() -> Self {
        Self {
            show_play_state: false,
            enable_marquee: false,
            enable_full_song_info: false,
            marquee_idle_text: String::from("INFINITAS"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DebugConfig {
    pub output_db: bool,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::parse(&content)
    }

    pub fn parse(content: &str) -> Result<Self> {
        let ini = IniParser::parse(content)?;
        let mut config = Config::default();

        // Update section
        config.update.update_files = ini.get_bool("update", "updatefiles");
        config.update.update_server = ini.get_string("update", "updateserver");

        // Record section
        config.record.save_remote = ini.get_bool("record", "saveremote");
        config.record.save_local = ini.get_bool("record", "savelocal");
        config.record.save_json = ini.get_bool("record", "savejson");
        config.record.save_latest_json = ini.get_bool("record", "savelatestjson");
        config.record.save_latest_txt = ini.get_bool("record", "savelatesttxt");

        // Remote record section
        config.remote_record.server_address = ini.get_string("remoterecord", "serveraddress");
        config.remote_record.api_key = ini.get_string("remoterecord", "apikey");

        // Local record section
        config.local_record.song_info = ini.get_bool("localrecord", "songinfo");
        config.local_record.chart_details = ini.get_bool("localrecord", "chartdetails");
        config.local_record.result_details = ini.get_bool("localrecord", "resultdetails");
        config.local_record.judge = ini.get_bool("localrecord", "judge");
        config.local_record.settings = ini.get_bool("localrecord", "settings");

        // Livestream section
        config.livestream.show_play_state = ini.get_bool("livestream", "playstate");
        config.livestream.enable_marquee = ini.get_bool("livestream", "marquee");
        config.livestream.enable_full_song_info = ini.get_bool("livestream", "fullsonginfo");
        let marquee_idle = ini.get_string("livestream", "marqueeidletext");
        if !marquee_idle.is_empty() {
            config.livestream.marquee_idle_text = marquee_idle;
        }

        // Debug section
        config.debug.output_db = ini.get_bool("debug", "outputdb");

        Ok(config)
    }
}

struct IniParser {
    sections: HashMap<String, HashMap<String, String>>,
}

impl IniParser {
    fn parse(content: &str) -> Result<Self> {
        let mut sections: HashMap<String, HashMap<String, String>> = HashMap::new();
        let mut current_section = String::new();

        for line in content.lines() {
            let line = line.trim();

            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].to_lowercase();
                sections.entry(current_section.clone()).or_default();
            } else if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_lowercase();
                let value = value.trim().to_string();
                if !current_section.is_empty() {
                    sections
                        .entry(current_section.clone())
                        .or_default()
                        .insert(key, value);
                }
            }
        }

        Ok(Self { sections })
    }

    fn get_string(&self, section: &str, key: &str) -> String {
        self.sections
            .get(section)
            .and_then(|s| s.get(key))
            .cloned()
            .unwrap_or_default()
    }

    fn get_bool(&self, section: &str, key: &str) -> bool {
        self.get_string(section, key)
            .to_lowercase()
            .parse::<bool>()
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let ini = r#"
[Record]
saveremote = true
savelocal = false

[RemoteRecord]
serveraddress = https://example.com
apikey = test123

[Livestream]
playstate = true
marqueeidletext = Custom Text
"#;
        let config = Config::parse(ini).unwrap();

        assert!(config.record.save_remote);
        assert!(!config.record.save_local);
        assert_eq!(config.remote_record.server_address, "https://example.com");
        assert_eq!(config.remote_record.api_key, "test123");
        assert!(config.livestream.show_play_state);
        assert_eq!(config.livestream.marquee_idle_text, "Custom Text");
    }

    #[test]
    fn test_empty_config() {
        let config = Config::parse("").unwrap();
        assert!(!config.record.save_remote);
        assert_eq!(config.livestream.marquee_idle_text, "INFINITAS");
    }
}
