use crate::error::{Error, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

const KAMAITACHI_API_BASE: &str = "https://kamai.tachi.ac/api/v1";

#[derive(Debug, Deserialize)]
struct SearchResponse {
    success: bool,
    body: Option<SearchBody>,
}

#[derive(Debug, Deserialize)]
struct SearchBody {
    songs: Vec<KamaitachiSong>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KamaitachiSong {
    pub id: i32,
    pub title: String,
    pub artist: String,
}

pub struct KamaitachiClient {
    client: Client,
}

impl KamaitachiClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self { client })
    }

    pub async fn search_song(&self, title: &str) -> Result<Option<KamaitachiSong>> {
        let url = format!(
            "{}/games/iidx/SP/songs?search={}",
            KAMAITACHI_API_BASE,
            urlencoding::encode(title)
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::NetworkError(e.to_string()))?;

        let search_result: SearchResponse = response
            .json()
            .await
            .map_err(|e| Error::NetworkError(e.to_string()))?;

        if !search_result.success {
            return Ok(None);
        }

        Ok(search_result.body.and_then(|b| b.songs.into_iter().next()))
    }

    pub async fn search_song_with_retry(&self, title: &str) -> Result<Option<KamaitachiSong>> {
        // Try with full title first
        if let Some(song) = self.search_song(title).await? {
            return Ok(Some(song));
        }

        // Try with progressively fewer words
        let words: Vec<&str> = title.split_whitespace().collect();
        for i in (1..words.len()).rev() {
            let partial_title = words[..i].join(" ");
            if let Some(song) = self.search_song(&partial_title).await? {
                return Ok(Some(song));
            }
        }

        Ok(None)
    }

    /// Get the Kamaitachi song ID for a given title
    ///
    /// This is useful for matching INFINITAS songs to Kamaitachi entries.
    /// It uses progressive search to handle title differences.
    pub async fn get_song_id(&self, title: &str) -> Result<Option<i32>> {
        Ok(self.search_song_with_retry(title).await?.map(|s| s.id))
    }
}

