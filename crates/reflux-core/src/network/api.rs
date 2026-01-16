use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::error::{Error, Result};
use crate::network::HttpClient;

/// RAII wrapper for temporary files that ensures cleanup on drop
struct TempFile {
    path: PathBuf,
    persisted: bool,
}

impl TempFile {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            persisted: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    /// Rename the temp file to the target path. Consumes self and prevents cleanup.
    fn persist(mut self, target: &Path) -> std::io::Result<()> {
        fs::rename(&self.path, target)?;
        self.persisted = true;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.persisted {
            // Best-effort cleanup; ignore errors
            let _ = fs::remove_file(&self.path);
        }
    }
}

const GITHUB_RELEASES_URL: &str = "https://github.com/dqn/reflux-rs/releases/latest";

/// Parameters for adding a new song
pub struct AddSongParams<'a> {
    pub song_id: &'a str,
    pub title: &'a str,
    pub title_english: &'a str,
    pub artist: &'a str,
    pub genre: &'a str,
    pub bpm: &'a str,
    pub unlock_type: u8,
}

#[derive(Clone)]
pub struct RefluxApi {
    client: HttpClient,
    update_server: String,
}

impl RefluxApi {
    pub fn new(server_address: String, api_key: String) -> Result<Self> {
        Ok(Self {
            client: HttpClient::new(server_address.clone(), api_key)?,
            update_server: server_address,
        })
    }

    /// Set the update server URL (for fetching support files)
    pub fn set_update_server(&mut self, url: String) {
        self.update_server = url;
    }

    /// Get the latest version from GitHub releases
    pub async fn get_latest_version() -> Result<String> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| Error::NetworkError(e.to_string()))?;

        let response = client
            .head(GITHUB_RELEASES_URL)
            .send()
            .await
            .map_err(|e| Error::NetworkError(e.to_string()))?;

        // Get the redirect URL which contains the version
        if let Some(location) = response.headers().get("location") {
            let url = location
                .to_str()
                .map_err(|_| Error::NetworkError("Invalid redirect URL".to_string()))?;

            // Extract version from URL (e.g., "https://github.com/olji/Reflux/releases/tag/v1.2.3")
            if let Some(version) = url.rsplit('/').next() {
                return Ok(version.to_string());
            }
        }

        Err(Error::NetworkError(
            "Could not determine latest version".to_string(),
        ))
    }

    /// Fetch a support file from the update server
    pub async fn fetch_support_file(&self, filename: &str) -> Result<String> {
        let url = format!("{}/{}.txt", self.update_server, filename);
        self.client.get(&url).await
    }

    /// Update a local support file if a newer version is available
    pub async fn update_support_file<P: AsRef<Path>>(
        &self,
        filename: &str,
        path: P,
    ) -> Result<bool> {
        // Get current version from local file
        let current_version = if path.as_ref().exists() {
            fs::read_to_string(&path)?
                .lines()
                .next()
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Fetch remote file
        let content = self.fetch_support_file(filename).await?;
        let remote_version = content
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        // Compare versions (format: YYYYMMDD)
        if remote_version > current_version {
            // Save old content for archiving (before any file operations).
            // Note: old_content is read into memory here, so subsequent file operations
            // do not create a race condition - the content is safely captured at this point.
            let old_content = if path.as_ref().exists() {
                Some(fs::read_to_string(&path)?)
            } else {
                None
            };

            // Write to temp file first for atomic update.
            // TempFile ensures cleanup even if subsequent operations fail.
            let temp_file = TempFile::new(path.as_ref().with_extension("tmp"));
            fs::write(temp_file.path(), &content)?;

            // Persist new file first to ensure update succeeds.
            // This is done before archiving to prevent data loss if persist fails.
            temp_file.persist(path.as_ref())?;

            // Archive old content (best-effort, failure doesn't affect the update)
            if let Some(old_content) = old_content {
                let archive_dir = path
                    .as_ref()
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join("archive");

                if let Err(e) = fs::create_dir_all(&archive_dir) {
                    tracing::warn!("Failed to create archive directory: {}", e);
                } else {
                    let archive_name = format!("{}_{}.txt", filename, current_version);
                    let archive_path = archive_dir.join(archive_name);
                    if let Err(e) = fs::write(&archive_path, old_content) {
                        tracing::warn!("Failed to archive old file: {}", e);
                    }
                }
            }

            return Ok(true);
        }

        Ok(false)
    }

    /// Update offsets file if the remote version matches the required version
    pub async fn update_offsets<P: AsRef<Path>>(&self, version: &str, path: P) -> Result<bool> {
        let content = self.fetch_support_file("offsets").await?;
        let remote_version = content
            .lines()
            .next()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        if remote_version != version {
            return Ok(false);
        }

        // Save old content for archiving (before any file operations)
        let old_content = if path.as_ref().exists() {
            let content = fs::read_to_string(&path)?;
            let old_version = content
                .lines()
                .next()
                .map(|s| s.trim().replace(':', "_"))
                .unwrap_or_else(|| "unknown".to_string());
            Some((content, old_version))
        } else {
            None
        };

        // Write to temp file first for atomic update
        // TempFile ensures cleanup even if subsequent operations fail
        let temp_file = TempFile::new(path.as_ref().with_extension("tmp"));
        fs::write(temp_file.path(), &content)?;

        // Persist new file first to ensure update succeeds
        // This is done before archiving to prevent data loss if persist fails
        temp_file.persist(path.as_ref())?;

        // Archive old content (best-effort, failure doesn't affect the update)
        if let Some((old_content, old_version)) = old_content {
            let archive_dir = path
                .as_ref()
                .parent()
                .unwrap_or(Path::new("."))
                .join("archive");

            if let Err(e) = fs::create_dir_all(&archive_dir) {
                tracing::warn!("Failed to create archive directory: {}", e);
            } else {
                let archive_path = archive_dir.join(format!("{}.txt", old_version));
                if let Err(e) = fs::write(&archive_path, old_content) {
                    tracing::warn!("Failed to archive old offsets file: {}", e);
                }
            }
        }

        Ok(true)
    }

    /// Report play data to remote server
    pub async fn report_play(&self, form: HashMap<String, String>) -> Result<String> {
        self.client.post_form("api/songplayed", form).await
    }

    /// Report unlock state change
    pub async fn report_unlock(&self, song_id: &str, unlock_state: i32) -> Result<String> {
        let mut form = HashMap::new();
        form.insert("songid".to_string(), song_id.to_string());
        form.insert("state".to_string(), unlock_state.to_string());

        self.client.post_form("api/unlocksong", form).await
    }

    /// Update chart unlock type
    pub async fn update_chart_unlock_type(&self, song_id: &str, unlock_type: u8) -> Result<String> {
        let mut form = HashMap::new();
        form.insert("songid".to_string(), song_id.to_string());
        form.insert("unlockType".to_string(), unlock_type.to_string());

        self.client.post_form("api/updatesong", form).await
    }

    /// Add a new song to remote server
    pub async fn add_song(&self, params: AddSongParams<'_>) -> Result<String> {
        let mut form = HashMap::new();
        form.insert("songid".to_string(), params.song_id.to_string());
        form.insert("title".to_string(), params.title.to_string());
        form.insert("title2".to_string(), params.title_english.to_string());
        form.insert("artist".to_string(), params.artist.to_string());
        form.insert("genre".to_string(), params.genre.to_string());
        form.insert("bpm".to_string(), params.bpm.to_string());
        form.insert("unlockType".to_string(), params.unlock_type.to_string());

        self.client.post_form("api/addsong", form).await
    }

    /// Add a chart to remote server
    pub async fn add_chart(
        &self,
        song_id: &str,
        difficulty: u8,
        level: u8,
        note_count: u32,
        unlocked: bool,
    ) -> Result<String> {
        let mut form = HashMap::new();
        form.insert("songid".to_string(), song_id.to_string());
        form.insert("diff".to_string(), difficulty.to_string());
        form.insert("level".to_string(), level.to_string());
        form.insert("notecount".to_string(), note_count.to_string());
        form.insert("unlocked".to_string(), unlocked.to_string());

        self.client.post_form("api/addchart", form).await
    }

    /// Post score to remote server
    pub async fn post_score(
        &self,
        song_id: &str,
        difficulty: u8,
        ex_score: u32,
        miss_count: u32,
        grade: &str,
        lamp: &str,
    ) -> Result<String> {
        let mut form = HashMap::new();
        form.insert("songid".to_string(), song_id.to_string());
        form.insert("diff".to_string(), difficulty.to_string());
        form.insert("exscore".to_string(), ex_score.to_string());
        form.insert("misscount".to_string(), miss_count.to_string());
        form.insert("grade".to_string(), grade.to_string());
        form.insert("lamp".to_string(), lamp.to_string());

        self.client.post_form("api/postscore", form).await
    }
}
