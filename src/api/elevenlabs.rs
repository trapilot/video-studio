use std::env;
use reqwest::Client;
use serde_json::json;

use crate::config::CONFIG;
use super::{read_bytes, send_err};

pub async fn generate_tts(
    client: &Client,
    text: &str,
    voice: &str,
) -> Result<Vec<u8>, String> {
    let api_key = env::var("ELEVENLABS_API_KEY")
        .map_err(|_| "Missing ELEVENLABS_API_KEY")?;
    
    let url = format!(
        "https://api.elevenlabs.io/v1/text-to-speech/{}",
        voice
    );

    let payload = json!({
        "text": text,
        "model_id": "eleven_multilingual_v2",
        "voice_settings": {
            "speed": &CONFIG.tts.speed,
            "stability": &CONFIG.tts.stability,
            "similarity_boost": &CONFIG.tts.similarity_boost,
        }
    });

    let res = client
        .post(&url)
        .header("xi-api-key", api_key)
        .header("Accept", "audio/mpeg")
        .json(&payload)
        .send()
        .await
        .map_err(|e| send_err("ElevenLabs", e))?;

    read_bytes(res, "ElevenLabs").await
}