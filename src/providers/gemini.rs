use async_trait::async_trait;

use crate::provider::*;

pub struct GeminiClient;

#[async_trait]
impl ProviderClient for GeminiClient {
    async fn call(
        &self,
        req: &ProviderRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {
        match req {
            ProviderRequest::Script(req) => {
                let primary = model_name();
                match self.script(req, credential, &primary).await {
                    // Main model overloaded: try the fallback model once, if configured.
                    Err(ProviderError::Unavailable { .. }) if fallback_model_name().is_some_and(|f| f != primary) => {
                        let fallback = fallback_model_name().unwrap_or_default();
                        println!("⏳ [gemini] {} overloaded, trying fallback model {}", primary, fallback);
                        self.script(req, credential, &fallback).await
                    }
                    other => other,
                }
            },

            _ => Err(ProviderError::NotSupported(Self.provider().to_string())),
        }
    }
    
    fn is_quota_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || body.contains("RESOURCE_EXHAUSTED")
            || body.contains("quota")
    }

    /// "This model is currently experiencing high demand" (503 UNAVAILABLE) is a
    /// model-wide overload, not a per-key limit.
    fn is_transient_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        matches!(status, 500 | 502 | 503 | 504)
            || body.contains("UNAVAILABLE")
            || body.contains("high demand")
            || body.contains("overloaded")
    }
    
    fn is_auth_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        status == reqwest::StatusCode::UNAUTHORIZED
    }

    
    fn provider(&self) -> Provider {
        Provider::Gemini
    }

    fn config(&self) -> ProviderConfig {
        ProviderConfig {
            reset_daily: true,
            reset_monthly: false,
            reset_time: Some(60),
            daily_limit: None,
            retry_after: 5,
        }
    }
}

/// Model used when `GEMINI_MODEL` is not set. Google retires models over time;
/// when a call fails with NOT_FOUND "no longer available", update this or set
/// `GEMINI_MODEL`. Every key uses the same model, so output doesn't depend on
/// which key answers (older keys may still reach retired models, newer ones not).
pub const DEFAULT_GEMINI_MODEL: &str = "gemini-3.8-flash";

/// `GEMINI_MODEL` if set (e.g. "gemini-3.8-flash"), else `DEFAULT_GEMINI_MODEL`.
pub fn model_name() -> String {
    std::env::var("GEMINI_MODEL")
        .ok()
        .map(|m| m.trim().trim_start_matches("models/").to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_string())
}

/// Optional `GEMINI_FALLBACK_MODEL`, used when the main model is overloaded.
pub fn fallback_model_name() -> Option<String> {
    std::env::var("GEMINI_FALLBACK_MODEL")
        .ok()
        .map(|m| m.trim().trim_start_matches("models/").to_string())
        .filter(|m| !m.is_empty())
}

impl GeminiClient {
    async fn script(
        &self,
        req: &ScriptRequest,
        credential: &ApiCredential,
        model: &str,
    ) -> Result<ProviderResponse, ProviderError> {

        // The key goes in a header, never the URL: reqwest errors include the URL,
        // and errors end up in logs and in the jobs table.
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model
        );

        let mut payload = serde_json::json!({
            "system_instruction": {
                "parts": [{
                    "text": req.system
                }]
            },
            "contents": [{
                "parts": [{
                    "text": req.prompt
                }]
            }],
            "generationConfig": {
                "temperature": req.temperature.unwrap_or(
                    if req.json_mode { 0.1 } else { 0.8 }
                )
            }
        });

        if let Some(max_tokens) = req.max_tokens {
            payload["generationConfig"]["maxOutputTokens"] =
                serde_json::json!(max_tokens);
        }

        if req.json_mode {
            payload["generationConfig"]["responseMimeType"] =
                serde_json::json!("application/json");
        }

        let res = reqwest::Client::new()
            .post(url)
            .header("x-goog-api-key", &credential.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                ProviderError::ClientResponse {
                    client: self.provider().to_string(),
                    error: e.without_url().to_string(),
                }
            })?;

        let data: serde_json::Value =
            self.read_json(res).await?;

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| {
                ProviderError::Http {
                    status: 500,
                    body: format!("Failed to parse response: {}", data),
                    provider: self.provider().to_string(),
                }
            })?;

        Ok(ProviderResponse::Text(text.trim().to_string()))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_demand_is_transient_not_quota() {
        let body = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;
        let err = GeminiClient.ensure_success(reqwest::StatusCode::SERVICE_UNAVAILABLE, body).unwrap_err();
        assert!(matches!(err, ProviderError::Unavailable { .. }), "{:?}", err);

        let quota = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED"}}"#;
        let err = GeminiClient.ensure_success(reqwest::StatusCode::TOO_MANY_REQUESTS, quota).unwrap_err();
        assert!(matches!(err, ProviderError::RequestExceeded { .. }), "{:?}", err);
    }

    #[test]
    fn model_name_defaults_and_accepts_override() {
        std::env::remove_var("GEMINI_MODEL");
        assert_eq!(model_name(), DEFAULT_GEMINI_MODEL);
        std::env::set_var("GEMINI_MODEL", " models/gemini-9-pro ");
        assert_eq!(model_name(), "gemini-9-pro");
        std::env::set_var("GEMINI_MODEL", "");
        assert_eq!(model_name(), DEFAULT_GEMINI_MODEL);
        std::env::remove_var("GEMINI_MODEL");
    }
}
