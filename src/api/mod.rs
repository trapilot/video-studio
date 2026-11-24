pub mod gemini;
pub mod hugging_face;
pub mod microsoft_edge;
pub mod googleapis;
pub mod elevenlabs;
pub mod fal;
pub mod cloudflare;
pub mod pollinations;
use serde_json::Value;

/// Parse a provider's JSON response, turning a non-2xx status into an error that
/// keeps the status and body. Never includes the request URL (it may carry secrets).
pub(crate) async fn read_json(res: reqwest::Response, provider: &str) -> Result<Value, String> {
    let status = res.status();
    let body = res.text().await.map_err(|e| format!("{} read error: {}", provider, e.without_url()))?;
    if !status.is_success() {
        return Err(format!("{} HTTP {}: {}", provider, status, body));
    }
    serde_json::from_str(&body).map_err(|e| format!("{} invalid JSON ({}): {}", provider, e, body))
}

/// Like `read_json`, for endpoints that return raw bytes.
pub(crate) async fn read_bytes(res: reqwest::Response, provider: &str) -> Result<Vec<u8>, String> {
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        return Err(format!("{} HTTP {}: {}", provider, status, body));
    }
    res.bytes().await.map(|b| b.to_vec()).map_err(|e| format!("{} read error: {}", provider, e.without_url()))
}

/// Format a transport error without its URL.
pub(crate) fn send_err(provider: &str, e: reqwest::Error) -> String {
    format!("{} request failed: {}", provider, e.without_url())
}
