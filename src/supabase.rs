use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Supabase client for persisting signals and config
#[derive(Clone)]
pub struct SupabaseClient {
    client: Client,
    url: String,
    api_key: String,
}

/// Session record for database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub mode: String,
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_high: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_low: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_volume: Option<i64>,
}

/// Signal record for database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalInsert {
    pub session_id: Uuid,
    pub timestamp: i64,
    pub signal_type: String,
    pub direction: String,
    pub price: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_after_1m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_after_5m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

/// User configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserConfig {
    #[serde(default = "default_min_size")]
    pub min_size: u32,
    #[serde(default = "default_sound_enabled")]
    pub sound_enabled: bool,
    #[serde(default = "default_symbols")]
    pub symbols: Vec<String>,
}

fn default_min_size() -> u32 {
    1
}
fn default_sound_enabled() -> bool {
    true
}
fn default_symbols() -> Vec<String> {
    vec!["NQ.c.0".to_string(), "ES.c.0".to_string()]
}

/// Response from Supabase insert with returning
#[derive(Debug, Deserialize)]
struct InsertResponse {
    id: Uuid,
}

impl SupabaseClient {
    /// Create a new Supabase client from environment variables
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("SUPABASE_URL").ok()?;
        let api_key = std::env::var("SUPABASE_ANON_KEY").ok()?;

        if url.is_empty() || api_key.is_empty() {
            return None;
        }

        Some(Self {
            client: Client::new(),
            url,
            api_key,
        })
    }

    /// Create a new Supabase client with explicit credentials
    pub fn new(url: String, api_key: String) -> Self {
        Self {
            client: Client::new(),
            url,
            api_key,
        }
    }

    /// Build request with auth headers
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, format!("{}/rest/v1/{}", self.url, path))
            .header("apikey", &self.api_key)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
    }

    /// Insert a new session and return its ID
    pub async fn insert_session(&self, session: &SessionRecord) -> Result<Uuid> {
        let response = self
            .request(reqwest::Method::POST, "sessions")
            .header("Prefer", "return=representation")
            .json(session)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to insert session: {} - {}", status, body));
        }

        let records: Vec<InsertResponse> = response.json().await?;
        let id = records
            .first()
            .ok_or_else(|| anyhow!("No session ID returned"))?
            .id;

        info!("Created session in Supabase: {}", id);
        Ok(id)
    }

    /// Update session with final stats
    pub async fn update_session(
        &self,
        session_id: Uuid,
        session_high: f64,
        session_low: f64,
        total_volume: u64,
    ) -> Result<()> {
        let response = self
            .request(reqwest::Method::PATCH, &format!("sessions?id=eq.{}", session_id))
            .json(&json!({
                "session_high": session_high,
                "session_low": session_low,
                "total_volume": total_volume as i64,
                "ended_at": chrono::Utc::now().to_rfc3339(),
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Failed to update session: {} - {}", status, body);
        }

        Ok(())
    }

    /// Insert a signal record (fire-and-forget style, logs errors)
    pub async fn insert_signal(&self, signal: SignalInsert) {
        match self.insert_signal_inner(signal).await {
            Ok(_) => {}
            Err(e) => error!("Failed to insert signal to Supabase: {}", e),
        }
    }

    async fn insert_signal_inner(&self, signal: SignalInsert) -> Result<()> {
        let response = self
            .request(reqwest::Method::POST, "signals")
            .json(&signal)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to insert signal: {} - {}", status, body));
        }

        Ok(())
    }

    /// Update signal outcomes in batch
    pub async fn update_signal_outcomes(&self, updates: Vec<SignalOutcomeUpdate>) {
        for update in updates {
            if let Err(e) = self.update_signal_outcome_inner(&update).await {
                warn!("Failed to update signal outcome: {}", e);
            }
        }
    }

    async fn update_signal_outcome_inner(&self, update: &SignalOutcomeUpdate) -> Result<()> {
        let response = self
            .request(
                reqwest::Method::PATCH,
                &format!("signals?timestamp=eq.{}&session_id=eq.{}", update.timestamp, update.session_id),
            )
            .json(&json!({
                "price_after_1m": update.price_after_1m,
                "price_after_5m": update.price_after_5m,
                "outcome": update.outcome,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to update signal: {} - {}", status, body));
        }

        Ok(())
    }

    /// Get user configuration
    pub async fn get_config(&self) -> Result<UserConfig> {
        let response = self
            .request(reqwest::Method::GET, "config?key=eq.user_settings")
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Ok(UserConfig::default());
        }

        #[derive(Deserialize)]
        struct ConfigRow {
            value: UserConfig,
        }

        let rows: Vec<ConfigRow> = response.json().await?;
        Ok(rows.into_iter().next().map(|r| r.value).unwrap_or_default())
    }

    /// Save user configuration
    pub async fn set_config(&self, config: &UserConfig) -> Result<()> {
        let response = self
            .request(reqwest::Method::PATCH, "config?key=eq.user_settings")
            .json(&json!({
                "value": config,
                "updated_at": chrono::Utc::now().to_rfc3339(),
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to save config: {} - {}", status, body));
        }

        info!("Saved config to Supabase");
        Ok(())
    }
}

/// Batch update for signal outcomes
#[derive(Debug, Clone)]
pub struct SignalOutcomeUpdate {
    pub session_id: Uuid,
    pub timestamp: i64,
    pub price_after_1m: Option<f64>,
    pub price_after_5m: Option<f64>,
    pub outcome: Option<String>,
}
