use std::env;

use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::{read_json, send_err};

pub async fn generate_script(
    client: &Client,
    system: &str,
    user: &str,
    is_json: bool,
) -> Result<String, String> {

    let api_key =
        env::var("GEMINI_API_KEY")
            .map_err(|_| "Missing GEMINI_API_KEY")?;

    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent";

    let mut payload = json!({
        "system_instruction": {
            "parts": [{
                "text": system
            }]
        },
        "contents": [{
            "parts": [{
                "text": user
            }]
        }],
        "generationConfig": {
            "temperature": if is_json { 0.1 } else { 0.8 }
        }
    });

    if is_json {
        payload["generationConfig"]["responseMimeType"] =
            json!("application/json");
    }

    // The key goes in a header, not the URL, so it never shows up in error messages.
    let res = client
        .post(url)
        .header("x-goog-api-key", &api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| send_err("Gemini", e))?;
    let res: Value = read_json(res, "Gemini").await?;

    res["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("Failed to parse Gemini response: {}", res))
}

pub async fn generate_tts(
    client: &Client,
    text: &str,
    _voice: &str,
) -> Result<Vec<u8>, String> {
    let api_key =
        env::var("GEMINI_API_KEY")
            .map_err(|_| "Missing GEMINI_API_KEY")?;

    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-preview-tts:generateContent";

    let payload = json!({
        "contents": [{
            "parts": [{
                "text": text
            }]
        }],
        "generationConfig": {
            "responseModalities": ["AUDIO"]
        }
    });

    // The key goes in a header, not the URL, so it never shows up in error messages.
    let res = client
        .post(url)
        .header("x-goog-api-key", &api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| send_err("Gemini", e))?;
    let res: Value = read_json(res, "Gemini").await?;

    let audio_b64 =
        res["candidates"][0]["content"]["parts"][0]["inlineData"]["data"]
            .as_str()
            .ok_or_else(|| format!("Invalid Gemini TTS response: {}", res))?;

    let audio =
        base64::engine::general_purpose::STANDARD
            .decode(audio_b64)
            .map_err(|e| e.to_string())?;

    Ok(audio)
}

pub async fn generate_image(
    client: &Client,
    prompt: &str,
) -> Result<Vec<u8>, String> {

    let api_key =
        env::var("GEMINI_API_KEY")
            .map_err(|_| "Missing GEMINI_API_KEY")?;

    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent";

    let payload = json!({
        "contents": [{
            "parts": [{
                "text": prompt
            }]
        }],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"]
        }
    });

    // The key goes in a header, not the URL, so it never shows up in error messages.
    let res = client
        .post(url)
        .header("x-goog-api-key", &api_key)
        .json(&payload)
        .send()
        .await
        .map_err(|e| send_err("Gemini", e))?;
    let res: Value = read_json(res, "Gemini").await?;

    let image_b64 = res["candidates"][0]["content"]["parts"]
        .as_array()
        .and_then(|parts| {
            parts.iter().find_map(|p| {
                p["inlineData"]["data"].as_str()
            })
        })
        .ok_or_else(|| format!("No image found in Gemini response: {}", res))?;

    let image_bytes =
        base64::engine::general_purpose::STANDARD
            .decode(image_b64)
            .map_err(|e| e.to_string())?;
    
    Ok(image_bytes)
}