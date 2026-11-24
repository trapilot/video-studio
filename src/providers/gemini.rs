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

impl GeminiClient {
    async fn script(
        &self,
        req: &ScriptRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {

        // The key goes in a header, never the URL: reqwest errors include the URL,
        // and errors end up in logs and in the jobs table.
        let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent";

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