use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use thiserror::Error;
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};


#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Gemini,
    CFWorker,
    ElevenLabs,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderConfig {
    pub daily_limit: Option<u32>,
    pub retry_after: u64,
    
    // requests_per_minute: u64,
    // tokens_per_minute: u64,
    // requests_per_day: u64,

    pub reset_daily: bool,
    pub reset_monthly: bool,

    pub reset_time: Option<u32>, // hour * 3600 + minutes * 60 + seconds,
}

pub struct ScriptRequest {
    pub system: String,
    pub prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub json_mode: bool,
}

pub struct ImageRequest {
    pub prompt: String,
    /// Fixed seed so the same prompt gives the same image on every key/account.
    pub seed: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub num_steps: Option<u32>,
    pub guidance: Option<f32>,
}

pub struct AudioRequest {
    pub text: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
    pub speed: Option<f32>,
    pub stability: Option<f32>,
    pub similarity_boost: Option<f32>,
    pub format: Option<AudioFormat>,
}

pub enum AudioFormat {
    Mp3,
    /// 16-bit mono WAV at the given sample rate.
    Wav { sample_rate: u32 },
    Ogg,
}

pub enum ProviderRequest {
    Script(ScriptRequest),
    Image(ImageRequest),
    Audio(AudioRequest),
    // Video(VideoRequest),
    // Embedding(EmbeddingRequest),
}

pub enum ProviderResponse {
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("[{provider}] Provider http error {status} --> {body}")]
    Http { provider: String, status: u16, body: String },

    #[error("[{provider}] Provider request exceeded {retry_after} --> {body}")]
    RequestExceeded { provider: String, retry_after: u64, body: String },

    #[error("[{provider}] Provider invalid api keys --> {body}")]
    RequestUnauthorized { provider: String, body: String },

    /// The request is fine but this key's account can't serve it (e.g. an
    /// ElevenLabs voice that exists only on another account). Try another key.
    #[error("[{provider}] Key cannot serve this request --> {body}")]
    KeyRejected { provider: String, body: String },

    /// The service is temporarily overloaded or down (5xx, "UNAVAILABLE").
    /// Not tied to a key: every key gets the same answer, so retry later instead.
    #[error("[{provider}] Provider temporarily unavailable --> {body}")]
    Unavailable { provider: String, body: String },

    #[error("[{0}] Provider all keys exhausted")]
    AllKeysExhausted(String),

    #[error("[{provider}] Provider response error {error}")]
    InvalidResponse { provider: String, error: String },

    #[error("Provider reponse not match")]
    UnexpectedResponse,

    #[error("[{0}] Provider does not support")]
    NotSupported(String),

    #[error("[{client}] Provider client invalid api keys")]
    ClientUnauthorized { client: String },

    #[error("[{client}] Provider client response error {error}")]
    ClientResponse { client: String, error: String },
}

/// `acquire` waits at most this long for a blocked key; longer = give up.
const MAX_ACQUIRE_WAIT_SECS: u64 = 60;

/// In-call retries for `Unavailable` before the error goes back to the agent
/// (whose job is then retried with the queue's longer backoff).
const TRANSIENT_RETRIES: u32 = 3;

/// 2 s, 4 s, 8 s (milliseconds in tests).
fn transient_backoff(attempt: u32) -> std::time::Duration {
    let base_ms: u64 = if cfg!(test) { 5 } else { 2_000 };
    std::time::Duration::from_millis(base_ms << attempt.min(5))
}

/// How long a key that can't serve a request (`KeyRejected`) is skipped.
const KEY_REJECTED_BLOCK_SECS: u64 = 3600;

#[derive(Debug, Clone)]
pub struct ApiCredential {
    pub api_key: String,
    pub account_id: Option<String>,
}

#[derive(Debug)]
pub struct ApiKeyState {
    pub credential: ApiCredential,

    pub daily_used: u32,
    pub daily_limit: Option<u32>,

    pub blocked_until: u64,
    pub last_reset_day: u64,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    pub failure_count: u32,
    pub last_failure_time: u64,
    pub is_open: bool,
}

#[derive(Debug)]
pub struct ProviderState {
    pub running: u32,
    pub maximum: u32,

    pub next_index: usize,
    pub keys: Vec<ApiKeyState>,

    pub circuit: CircuitBreaker,
}

pub struct ProviderRuntime {
    pub state: ProviderState,
    pub config: ProviderConfig,
    pub client: Arc<dyn ProviderClient>,
}

#[derive(Clone)]
pub struct ProviderManager {
    inner: Arc<RwLock<HashMap<Provider, Arc<Mutex<ProviderRuntime>>>>>,
}

#[derive(Clone)]
pub struct ProviderGuard {
    provider: Provider,
    credential: ApiCredential,
    client: Arc<dyn ProviderClient>,
    manager: ProviderManager,
}

#[async_trait::async_trait]
pub trait ProviderClient: Send + Sync {
    fn provider(&self) -> Provider;
    fn config(&self) -> ProviderConfig;

    async fn call(
        &self,
        req: &ProviderRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError>;

    fn is_auth_error(&self, status: u16, body: &str) -> bool;
    fn is_quota_error(&self, status: u16, body: &str) -> bool;

    /// Errors tied to one key's account rather than to the request
    /// (see `ProviderError::KeyRejected`). Default: none.
    fn is_key_specific_error(&self, _status: u16, _body: &str) -> bool {
        false
    }

    /// True if every key also needs `{PREFIX}_ACCOUNT_n` (e.g. Cloudflare account id).
    fn requires_account(&self) -> bool {
        false
    }

    /// Temporary service-side failures (see `ProviderError::Unavailable`).
    fn is_transient_error(&self, status: u16, _body: &str) -> bool {
        matches!(status, 500 | 502 | 503 | 504)
    }

    async fn read_json<T>(
        &self,
        res: reqwest::Response,
    ) -> Result<T, ProviderError>
    where
        T: serde::de::DeserializeOwned,
        Self: Sized,
    {
        let status = res.status();

        let body = res.text().await.unwrap_or_default();

        self.ensure_success(status, &body)?;

        serde_json::from_str(&body).map_err(|e| {
            ProviderError::InvalidResponse {
                provider: self.provider().to_string(),
                error: e.to_string(),
            }
        })
    }

    async fn read_bytes(
        &self,
        res: reqwest::Response,
    ) -> Result<Vec<u8>, ProviderError>
    {
        let status = res.status();

        let bytes = res.bytes().await.map_err(|e| {
            ProviderError::InvalidResponse {
                provider: self.provider().to_string(),
                error: e.to_string(),
            }
        })?;

        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes);

            self.ensure_success(status, &body)?;
        }

        Ok(bytes.to_vec())
    }

    fn ensure_success(
        &self,
        status: reqwest::StatusCode,
        body: &str,
    ) -> Result<(), ProviderError> {
        let code = status.as_u16();

        if self.is_auth_error(code, body) {
            return Err(ProviderError::RequestUnauthorized {
                provider: self.provider().to_string(),
                body: body.to_string(),
            });
        }

        if self.is_transient_error(code, body) {
            return Err(ProviderError::Unavailable {
                provider: self.provider().to_string(),
                body: body.to_string(),
            });
        }

        if self.is_key_specific_error(code, body) {
            return Err(ProviderError::KeyRejected {
                provider: self.provider().to_string(),
                body: body.to_string(),
            });
        }

        if self.is_quota_error(code, body) {
            let config = self.config();
            return Err(ProviderError::RequestExceeded {
                provider: self.provider().to_string(),
                retry_after: config.retry_after,
                body: body.to_string(),
            });
        }

        if !status.is_success() {
            return Err(ProviderError::Http {
                provider: self.provider().to_string(),
                status: code,
                body: body.to_string(),
            });
        }

        Ok(())
    }
}

impl ProviderManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register_with_keys(
        &self,
        client: Arc<dyn ProviderClient>,
        concurrency: u32,
        env_prefix: &str,
    ) {
        let provider = client.provider();
        let config = client.config();
        let requires_account = client.requires_account();

        let runtime = Arc::new(Mutex::new(ProviderRuntime {
            config,
            client,
            state: ProviderState {
                running: 0,
                maximum: concurrency,
                next_index: 0,
                keys: vec![],
                circuit: CircuitBreaker {
                    failure_count: 0,
                    last_failure_time: 0,
                    is_open: false,
                },
            },
        }));

        {
            // Only lock write operations when hardcoded at app startup.
            let mut map = self.inner.write().await;
            map.insert(provider.clone(), runtime);
        }

        self.load_keys_from_env(&provider, &config, env_prefix, requires_account).await;
    }

    pub async fn acquire(&self, provider: &Provider) -> Option<ProviderGuard> {
        loop {
            // Get the mutex specific to that provider using Read lock (without blocking others)
            let runtime_mutex = {
                let map = self.inner.read().await;
                map.get(provider).cloned()?
            };
            
            // Lock only the current provider's runtime
            let mut runtime: tokio::sync::MutexGuard<'_, ProviderRuntime> = runtime_mutex.lock().await;

            let client = runtime.client.clone();
            let state = &mut runtime.state;
            
            // #[cfg(debug_assertions)]
            // println!("{:#?}", state);

            if state.circuit.is_open {
                let elapsed = Self::now_day() - state.circuit.last_failure_time;

                if elapsed < 60 {
                    return None;
                }

                state.circuit.is_open = false;
                state.circuit.failure_count = 0;
            }

            if state.running >= state.maximum {
                drop(runtime);

                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            let now = Self::now_day();
            let today = Self::current_day();

            let max_attempts = state.keys.len();
            if max_attempts == 0 {
                return None;
            }

            for _ in 0..max_attempts {
                let idx = state.next_index % max_attempts;
                
                state.next_index += 1;
                if state.next_index >= max_attempts {
                    state.next_index = 0;
                }

                let key = &mut state.keys[idx];

                if key.last_reset_day != today {
                    key.daily_used = 0;
                    key.last_reset_day = today;
                    key.blocked_until = 0;
                }

                if key.blocked_until > now {
                    continue;
                }

                if let Some(daily_limit) = key.daily_limit {
                    if key.daily_used >= daily_limit {
                        continue;
                    }
                }
                
                let credential = key.credential.clone();

                key.daily_used += 1;
                state.running += 1;

                // Never log the key itself.
                println!(
                    "[{}] acquired key #{} | used today: {} | running: {}/{}",
                    provider, idx + 1, key.daily_used, state.running, state.maximum,
                );
                return Some(ProviderGuard {
                    credential,
                    provider: provider.clone(),
                    client: client.clone(),
                    manager: self.clone(),
                });
            }

            // No usable key right now. If the soonest one frees up later than
            // MAX_ACQUIRE_WAIT_SECS (disabled until tomorrow, daily limit), return
            // None so the job fails and the queue retries it with its backoff,
            // instead of blocking this agent for hours.
            let soonest = state.keys
                .iter()
                .map(|k| {
                    let over_limit = k.daily_limit.is_some_and(|l| k.daily_used >= l);
                    if over_limit { Self::next_day_timestamp() } else { k.blocked_until }
                })
                .min()
                .unwrap_or(now);
            if soonest.saturating_sub(now) > MAX_ACQUIRE_WAIT_SECS {
                eprintln!("🔴 [{}] all keys are blocked or over their daily limit", provider);
                return None;
            }

            // Release the lock before repeating/waiting
            drop(runtime);

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        };
    }

    async fn release(&self, provider: &Provider) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;

            // Only free the slot. The circuit is reset by `record_success`, so
            // failures accumulate across calls and can actually open it.
            runtime.state.running = runtime.state.running.saturating_sub(1);
        }
    }

    async fn record_success(&self, provider: &Provider) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;
            runtime.state.circuit.failure_count = 0;
            runtime.state.circuit.is_open = false;
        }
    }
    
    async fn block_key(
        &self,
        provider: &Provider,
        api_key: &str,
        seconds: u64
    ) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;

            let now = Self::now_day();
            
            if let Some(key) = runtime.state.keys.iter_mut().find(|k| k.credential.api_key == api_key) {
                key.blocked_until = now + seconds;
            }
        }
    }

    async fn block_until_tomorrow(
        &self,
        provider: Provider,
        api_key: &str,
    ) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(&provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;
            
            if let Some(k) = runtime.state.keys.iter_mut().find(|k| k.credential.api_key == api_key) {
                k.blocked_until = Self::next_day_timestamp();
            }
        }
    }

    pub async fn add_key(
        &self,
        provider: &Provider,
        config: &ProviderConfig,
        credential: ApiCredential,
    ) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;
            runtime.state.keys.push(ApiKeyState {
                credential,
                daily_used: 0,
                daily_limit: config.daily_limit,
                blocked_until: 0,
                last_reset_day: Self::current_day(),
            });
        }
    }
    
    /// 1-based position of a key, for logs (never log the key itself).
    async fn key_number(&self, provider: &Provider, api_key: &str) -> usize {
        let map = self.inner.read().await;
        let Some(mutex) = map.get(provider) else { return 0 };
        let runtime = mutex.lock().await;
        runtime.state.keys.iter().position(|k| k.credential.api_key == api_key).map(|i| i + 1).unwrap_or(0)
    }

    async fn count_key(
        &self,
        provider: &Provider,
    ) -> usize {
        let map = self.inner.read().await;
        if let Some(mutex) = map.get(provider) {
            let runtime = mutex.lock().await;
            runtime.state.keys.len()
        } else {
            0
        }
    }

    async fn next_key(
        &self,
        provider: &Provider,
    ) -> Option<ApiCredential> {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()?
        };

        let mut runtime = runtime_mutex.lock().await;
        let now = Self::now_day();
        let today = Self::current_day();
        let total = runtime.state.keys.len();

        for _ in 0..total {
            let idx = runtime.state.next_index % total;

            runtime.state.next_index += 1;
            if runtime.state.next_index >= total {
                runtime.state.next_index = 0;
            }

            let key = &mut runtime.state.keys[idx];

            if key.last_reset_day != today {
                key.daily_used = 0;
                key.blocked_until = 0;
                key.last_reset_day = today;
            }

            if key.blocked_until > now {
                continue;
            }

            if let Some(daily_limit) = key.daily_limit {
                if key.daily_used >= daily_limit {
                    continue;
                }
            }

            key.daily_used += 1;
            
            return Some(key.credential.clone());
        }

        None
    }

    async fn record_failure(&self, provider: &Provider) {
        let runtime_mutex = {
            let map = self.inner.read().await;
            map.get(provider).cloned()
        };

        if let Some(mutex) = runtime_mutex {
            let mut runtime = mutex.lock().await;
            
            let circuit = &mut runtime.state.circuit;
            
            circuit.failure_count += 1;
            circuit.last_failure_time = Self::now_day();

            if circuit.failure_count >= 5 {
                circuit.is_open = true;
            }
        }
    }

    async fn load_keys_from_env(
        &self,
        provider: &Provider,
        config: &ProviderConfig,
        prefix: &str,
        requires_account: bool,
    ) {
        let mut i = 1;
        let mut loaded = 0;
        loop {
            let key_name = format!("{}_KEY_{}", prefix, i);
            let Ok(api_key) = std::env::var(&key_name) else {
                break;
            };
            i += 1;

            // An empty `KEY_n=` line in .env is a placeholder, not a key.
            let api_key = api_key.trim().to_string();
            if api_key.is_empty() {
                println!("⚠️  [{}] {} is empty, skipped", provider, key_name);
                continue;
            }

            let account_name = format!("{}_ACCOUNT_{}", prefix, i - 1);
            let account_id = std::env::var(&account_name)
                .ok()
                .map(|a| a.trim().to_string())
                .filter(|a| !a.is_empty());

            if requires_account && account_id.is_none() {
                eprintln!("🔴 [{}] {} is set but {} is missing or empty, key skipped", provider, key_name, account_name);
                continue;
            }

            self.add_key(
                provider,
                config,
                ApiCredential { api_key, account_id },
            ).await;
            loaded += 1;
        }

        if loaded == 0 {
            println!("⚠️  [{}] no usable keys ({}_KEY_1, ...): this provider is disabled", provider, prefix);
        }
    }

    fn now_day() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn current_day() -> u64 {
        Self::now_day() / 86400
    }
    
    fn next_day_timestamp() -> u64 {
        let now = Self::now_day();

        ((now / 86400) + 1) * 86400
    }
}

impl Drop for ProviderGuard {
    fn drop(&mut self) {
        let provider = self.provider.clone();
        let manager = self.manager.clone();

        tokio::spawn(async move {
            manager.release(&provider).await;
        });
    }
}

impl ProviderGuard {
    pub async fn call(
        self,
        req: ProviderRequest,
    ) -> Result<ProviderResponse, ProviderError> {
        let max_keys = self.manager
            .count_key(&self.provider)
            .await;

        // Use the key `acquire` already picked (and counted). Rotate to another key
        // only for key-level problems; the request itself (prompt, seed, voice) is
        // identical on every key, so which key answers doesn't change the result.
        let mut next = Some(self.credential.clone());
        let mut last_error = None;
        let mut transient_retries = 0;

        for _ in 0..max_keys.max(1) {
            let Some(credential) = next.take() else {
                break;
            };

            // Overload/5xx: same answer on every key, so wait and retry the same
            // request a few times instead of burning keys or the job's attempts.
            let mut result = self.client.call(&req, &credential).await;
            while matches!(result, Err(ProviderError::Unavailable { .. })) && transient_retries < TRANSIENT_RETRIES {
                let wait = transient_backoff(transient_retries);
                transient_retries += 1;
                println!(
                    "⏳ [{}] temporarily unavailable, retry {}/{} in {:?}",
                    self.provider, transient_retries, TRANSIENT_RETRIES, wait
                );
                tokio::time::sleep(wait).await;
                result = self.client.call(&req, &credential).await;
            }

            match result {
                Ok(v) => {
                    self.manager.record_success(&self.provider).await;
                    return Ok(v)
                },
                Err(e) => match &e {
                    // Rate-limited: skip this key briefly.
                    ProviderError::RequestExceeded { retry_after, .. } => {
                        self.manager.block_key(&self.provider, &credential.api_key, *retry_after).await;
                        last_error = Some(e);
                    }
                    // Invalid/revoked key: park it until tomorrow so it stops being picked.
                    ProviderError::RequestUnauthorized { .. } => {
                        eprintln!(
                            "🔴 [{}] key #{} rejected as unauthorized, disabled until tomorrow: {}",
                            self.provider,
                            self.manager.key_number(&self.provider, &credential.api_key).await,
                            e,
                        );
                        self.manager.block_until_tomorrow(self.provider.clone(), &credential.api_key).await;
                        last_error = Some(e);
                    }
                    // This account can't serve the request; another key's account may.
                    ProviderError::KeyRejected { .. } => {
                        self.manager.block_key(&self.provider, &credential.api_key, KEY_REJECTED_BLOCK_SECS).await;
                        last_error = Some(e);
                    }
                    _ => {
                        self.manager.record_failure(&self.provider).await;
                        return Err(e);
                    }
                },
            }
            next = self.manager.next_key(&self.provider).await;
        }

        Err(last_error.unwrap_or_else(|| ProviderError::AllKeysExhausted(self.provider.to_string())))
    }


}

impl ProviderResponse {
    pub fn into_text(self) -> Result<String, ProviderError> {
        match self {
            ProviderResponse::Text(t) => Ok(t),
            ProviderResponse::Bytes(_) => Err(ProviderError::UnexpectedResponse),
        }
    }

    pub fn into_bytes(self) -> Result<Vec<u8>, ProviderError> {
        match self {
            ProviderResponse::Bytes(b) => Ok(b),
            ProviderResponse::Text(_) => Err(ProviderError::UnexpectedResponse),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysFails;

    #[async_trait::async_trait]
    impl ProviderClient for AlwaysFails {
        fn provider(&self) -> Provider { Provider::Gemini }
        fn config(&self) -> ProviderConfig {
            ProviderConfig { daily_limit: None, retry_after: 5, reset_daily: false, reset_monthly: false, reset_time: None }
        }
        async fn call(&self, _: &ProviderRequest, _: &ApiCredential) -> Result<ProviderResponse, ProviderError> {
            Err(ProviderError::Http { provider: "test".into(), status: 500, body: "boom".into() })
        }
        fn is_auth_error(&self, _: u16, _: &str) -> bool { false }
        fn is_quota_error(&self, _: u16, _: &str) -> bool { false }
    }

    fn req() -> ProviderRequest {
        ProviderRequest::Script(ScriptRequest {
            system: String::new(), prompt: String::new(), temperature: None, max_tokens: None, json_mode: false,
        })
    }

    /// Rejects the key "bad" as unauthorized and the key "nov" with an
    /// account-specific error; answers with the key it was called with otherwise.
    struct PicksGoodKey;

    #[async_trait::async_trait]
    impl ProviderClient for PicksGoodKey {
        fn provider(&self) -> Provider { Provider::ElevenLabs }
        fn config(&self) -> ProviderConfig {
            ProviderConfig { daily_limit: None, retry_after: 5, reset_daily: false, reset_monthly: false, reset_time: None }
        }
        async fn call(&self, _: &ProviderRequest, c: &ApiCredential) -> Result<ProviderResponse, ProviderError> {
            match c.api_key.as_str() {
                "bad" => Err(ProviderError::RequestUnauthorized { provider: "t".into(), body: "invalid key".into() }),
                "nov" => Err(ProviderError::KeyRejected { provider: "t".into(), body: "voice_not_found".into() }),
                k => Ok(ProviderResponse::Text(k.to_string())),
            }
        }
        fn is_auth_error(&self, _: u16, _: &str) -> bool { false }
        fn is_quota_error(&self, _: u16, _: &str) -> bool { false }
    }

    #[tokio::test]
    async fn rotates_past_bad_keys_and_stops_picking_them() {
        let manager = ProviderManager::new();
        manager.register_with_keys(Arc::new(PicksGoodKey), 10, "VIDEO_STUDIO_TEST_UNSET").await;
        for key in ["bad", "nov", "good"] {
            manager.add_key(&Provider::ElevenLabs, &PicksGoodKey.config(), ApiCredential { api_key: key.into(), account_id: None }).await;
        }

        // Whatever key acquire() starts with, the call ends on "good".
        for _ in 0..4 {
            let guard = manager.acquire(&Provider::ElevenLabs).await.unwrap();
            let answer = guard.call(req()).await.unwrap().into_text().unwrap();
            assert_eq!(answer, "good");
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // Both bad keys are now blocked, so only "good" is handed out.
        for _ in 0..3 {
            let guard = manager.acquire(&Provider::ElevenLabs).await.unwrap();
            assert_eq!(guard.credential.api_key, "good");
        }
    }

    /// Answers `Unavailable` for the first `fail_first` calls, then succeeds.
    struct Flaky {
        fail_first: u32,
        calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl ProviderClient for Flaky {
        fn provider(&self) -> Provider { Provider::CFWorker }
        fn config(&self) -> ProviderConfig {
            ProviderConfig { daily_limit: None, retry_after: 5, reset_daily: false, reset_monthly: false, reset_time: None }
        }
        async fn call(&self, _: &ProviderRequest, c: &ApiCredential) -> Result<ProviderResponse, ProviderError> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < self.fail_first {
                Err(ProviderError::Unavailable { provider: "t".into(), body: "high demand".into() })
            } else {
                Ok(ProviderResponse::Text(c.api_key.clone()))
            }
        }
        fn is_auth_error(&self, _: u16, _: &str) -> bool { false }
        fn is_quota_error(&self, _: u16, _: &str) -> bool { false }
    }

    async fn flaky_manager(fail_first: u32) -> (ProviderManager, Arc<Flaky>) {
        let client = Arc::new(Flaky { fail_first, calls: Default::default() });
        let manager = ProviderManager::new();
        manager.register_with_keys(client.clone(), 10, "VIDEO_STUDIO_TEST_UNSET").await;
        manager.add_key(&Provider::CFWorker, &client.config(), ApiCredential { api_key: "k".into(), account_id: None }).await;
        (manager, client)
    }

    #[tokio::test]
    async fn overload_is_retried_in_place() {
        let (manager, client) = flaky_manager(2).await;
        let guard = manager.acquire(&Provider::CFWorker).await.unwrap();
        assert_eq!(guard.call(req()).await.unwrap().into_text().unwrap(), "k");
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn lasting_overload_returns_unavailable_after_retries() {
        let (manager, client) = flaky_manager(u32::MAX).await;
        let guard = manager.acquire(&Provider::CFWorker).await.unwrap();
        assert!(matches!(guard.call(req()).await, Err(ProviderError::Unavailable { .. })));
        assert_eq!(client.calls.load(std::sync::atomic::Ordering::SeqCst), 1 + TRANSIENT_RETRIES);
    }

    struct NeedsAccount;

    #[async_trait::async_trait]
    impl ProviderClient for NeedsAccount {
        fn provider(&self) -> Provider { Provider::CFWorker }
        fn config(&self) -> ProviderConfig {
            ProviderConfig { daily_limit: None, retry_after: 5, reset_daily: false, reset_monthly: false, reset_time: None }
        }
        fn requires_account(&self) -> bool { true }
        async fn call(&self, _: &ProviderRequest, c: &ApiCredential) -> Result<ProviderResponse, ProviderError> {
            Ok(ProviderResponse::Text(c.api_key.clone()))
        }
        fn is_auth_error(&self, _: u16, _: &str) -> bool { false }
        fn is_quota_error(&self, _: u16, _: &str) -> bool { false }
    }

    #[tokio::test]
    async fn empty_and_incomplete_keys_are_skipped() {
        // Like a .env with KEY_2/ACCOUNT_2 left empty and KEY_3 without an account.
        std::env::set_var("VS_TEST_SKIP_KEY_1", "good");
        std::env::set_var("VS_TEST_SKIP_ACCOUNT_1", "acc");
        std::env::set_var("VS_TEST_SKIP_KEY_2", "");
        std::env::set_var("VS_TEST_SKIP_ACCOUNT_2", "");
        std::env::set_var("VS_TEST_SKIP_KEY_3", "no-account");

        let manager = ProviderManager::new();
        manager.register_with_keys(Arc::new(NeedsAccount), 10, "VS_TEST_SKIP").await;
        assert_eq!(manager.count_key(&Provider::CFWorker).await, 1);
        for _ in 0..3 {
            let guard = manager.acquire(&Provider::CFWorker).await.unwrap();
            assert_eq!(guard.credential.api_key, "good");
        }
    }

    #[tokio::test]
    async fn acquire_gives_up_when_all_keys_are_disabled() {
        let manager = ProviderManager::new();
        manager.register_with_keys(Arc::new(NeedsAccount), 10, "VS_TEST_UNSET").await;
        manager.add_key(&Provider::CFWorker, &NeedsAccount.config(), ApiCredential { api_key: "k".into(), account_id: Some("a".into()) }).await;
        manager.block_until_tomorrow(Provider::CFWorker, "k").await;

        let started = std::time::Instant::now();
        assert!(manager.acquire(&Provider::CFWorker).await.is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(2), "must not wait for tomorrow");
    }

    #[tokio::test]
    async fn circuit_opens_after_repeated_failures() {
        let manager = ProviderManager::new();
        manager.register_with_keys(Arc::new(AlwaysFails), 10, "VIDEO_STUDIO_TEST_UNSET").await;
        manager.add_key(&Provider::Gemini, &AlwaysFails.config(), ApiCredential { api_key: "k".into(), account_id: None }).await;

        for _ in 0..5 {
            let guard = manager.acquire(&Provider::Gemini).await.expect("closed circuit hands out the key");
            assert!(guard.call(req()).await.is_err());
            // Let the guard's Drop (which spawns `release`) run.
            tokio::task::yield_now().await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        assert!(manager.acquire(&Provider::Gemini).await.is_none(), "circuit should be open after 5 failures");
    }
}
