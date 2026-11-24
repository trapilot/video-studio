use async_trait::async_trait;
use base64::Engine;

use crate::provider::*;

pub struct CFWorkerClient;

/// Cloudflare API error codes in a response body: `{"errors":[{"code":10000,...}]}`.
fn cf_error_codes(body: &str) -> Vec<i64> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["errors"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["code"].as_i64())
        .collect()
}

#[async_trait]
impl ProviderClient for CFWorkerClient {
    async fn call(
        &self,
        req: &ProviderRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {
        match req {
            ProviderRequest::Image(req) => {
                self.image(req, credential).await
            },

            _ => Err(ProviderError::NotSupported(format!("{}", Self.provider().clone()))),
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
            || body.contains("rate limit")
    }
    
    fn is_auth_error(
        &self,
        status: u16,
        body: &str,
    ) -> bool {
        status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
            || cf_error_codes(body).contains(&10000) // "Authentication error"
    }

    /// Workers AI needs an account id per key (`CF_WORKER_ACCOUNT_n`).
    fn requires_account(&self) -> bool {
        true
    }

    /// 7003 "Could not route ... object identifier is invalid": the account id
    /// paired with this key is wrong. Another key/account may work.
    fn is_key_specific_error(
        &self,
        _status: u16,
        body: &str,
    ) -> bool {
        cf_error_codes(body).contains(&7003)
    }

    fn provider(&self) -> Provider {
        Provider::CFWorker
    }

    fn config(&self) -> ProviderConfig {
        ProviderConfig {
            reset_daily: true,
            reset_monthly: false,
            reset_time: Some(60),
            daily_limit: Some(100000),
            retry_after: 5,
        }
    }
}

impl CFWorkerClient {
    async fn image(
        &self,
        req: &ImageRequest,
        credential: &ApiCredential,
    ) -> Result<ProviderResponse, ProviderError> {

        match req {
            ImageRequest { prompt, num_steps, guidance, seed, .. } => {
                let mut payload = serde_json::json!({
                    "prompt": prompt,
                    "num_steps": num_steps,
                    "guidance": guidance,
                    "negative_prompt": "low quality, blurry, distorted face",
                });
                if let Some(seed) = seed {
                    payload["seed"] = serde_json::json!(seed);
                }


                let Some(account_id) = &credential.account_id else {
                    return Err(ProviderError::ClientUnauthorized {
                        client: "CFWorkerClient".to_string(),
                    });
                };

                let url = format!(
                    "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/@cf/black-forest-labs/flux-1-schnell",
                    account_id
                );

                let res = reqwest::Client::new()
                    .post(url)
                    .bearer_auth(&credential.api_key)
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| {
                        ProviderError::ClientResponse {
                            client: "CFWorker".to_string(),
                            error: e.without_url().to_string(),
                        }
                    })?;

                let data: serde_json::Value =
                    self.read_json(res).await?;

                let b64: &str = data["result"]["image"]
                    .as_str()
                    .ok_or_else(|| {
                        ProviderError::ClientResponse {
                            client: "CFWorker".to_string(),
                            error: "Invalid JSON".to_string(),
                        }
                    })?;

                let decode = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| {
                        ProviderError::ClientResponse {
                            client: "CFWorker".to_string(),
                            error: e.to_string(),
                        }
                    });

                match decode {
                    Ok(bytes) => Ok(ProviderResponse::Bytes(bytes)),
                    _ => Err(
                        ProviderError::ClientResponse {
                            client: "CFWorker".to_string(),
                            error: "Invalid reponse type".to_string(),
                        }
                    )
                }
            }

            _ => Err(ProviderError::NotSupported(format!("{}", Self.provider().clone()))),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cloudflare_errors_by_code() {
        let auth = r#"{"success":false,"errors":[{"code":10000,"message":"Authentication error"}]}"#;
        assert!(matches!(
            CFWorkerClient.ensure_success(reqwest::StatusCode::BAD_REQUEST, auth),
            Err(ProviderError::RequestUnauthorized { .. })
        ));

        let route = r#"{"result":null,"success":false,"errors":[{"code":7003,"message":"Could not route to /client/v4/accounts/ai/run/..."}]}"#;
        assert!(matches!(
            CFWorkerClient.ensure_success(reqwest::StatusCode::NOT_FOUND, route),
            Err(ProviderError::KeyRejected { .. })
        ));

        // A number that merely contains "10000" is not an auth error.
        let other = r#"{"success":false,"errors":[{"code":5006,"message":"prompt longer than 100000 chars"}]}"#;
        assert!(matches!(
            CFWorkerClient.ensure_success(reqwest::StatusCode::BAD_REQUEST, other),
            Err(ProviderError::Http { .. })
        ));
    }
}
