use crate::error::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct HttpClient {
    client: Client,
    base_url: String,
    api_key: String,
}

impl HttpClient {
    pub fn new(base_url: String, api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url,
            api_key,
        }
    }

    pub async fn post_form(&self, endpoint: &str, form: HashMap<String, String>) -> Result<String> {
        let url = format!("{}/{}", self.base_url, endpoint);

        let mut form = form;
        form.insert("apikey".to_string(), self.api_key.clone());

        let response = self
            .client
            .post(&url)
            .form(&form)
            .send()
            .await?
            .error_for_status()?;

        let text = response.text().await?;
        Ok(text)
    }

    pub async fn get(&self, url: &str) -> Result<String> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let text = response.text().await?;
        Ok(text)
    }
}
