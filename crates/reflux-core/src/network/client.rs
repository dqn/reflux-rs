use crate::error::{Error, Result};
use reqwest::{Client, StatusCode};
use std::collections::HashMap;
use std::time::Duration;
use tracing::warn;

#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl HttpClient {
    pub fn new(base_url: String, api_key: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::NetworkError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            base_url,
            api_key,
        })
    }

    pub async fn post_form(&self, endpoint: &str, form: HashMap<String, String>) -> Result<String> {
        const MAX_RETRIES: u32 = 3;

        let url = format!("{}/{}", self.base_url, endpoint);

        let mut form = form;
        form.insert("apikey".to_string(), self.api_key.clone());

        let mut backoff_ms = 100u64;

        for attempt in 0..MAX_RETRIES {
            let response = self.client.post(&url).form(&form).send().await?;

            if response.status() == StatusCode::TOO_MANY_REQUESTS
                && attempt < MAX_RETRIES - 1
            {
                let delay = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|secs| secs * 1000)
                    .unwrap_or(backoff_ms);

                warn!(
                    "Rate limited (attempt {}/{}), retrying in {}ms",
                    attempt + 1,
                    MAX_RETRIES,
                    delay
                );
                tokio::time::sleep(Duration::from_millis(delay)).await;
                backoff_ms = (backoff_ms * 2).min(5000);
                continue;
            }

            let response = response.error_for_status()?;
            return Ok(response.text().await?);
        }

        Err(Error::NetworkError(
            "Max retries exceeded due to rate limiting".to_string(),
        ))
    }

    pub async fn get(&self, url: &str) -> Result<String> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let text = response.text().await?;
        Ok(text)
    }
}
