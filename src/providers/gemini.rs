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
                self.script(req, credential).await
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
            || body.contains("UNAVAILABLE")
            || body.contains("quota")
            || body.contains("experiencing high demand")
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

impl GeminiClient {
    async fn script(
        &self,
        req: &ScriptRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {

        // The key goes in a header, never the URL: reqwest errors include the URL,
        // and errors end up in logs and in the jobs table.
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
            model_name()
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
