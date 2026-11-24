---
name: add-provider
description: Add or swap an AI provider (LLM, image, or TTS) under src/api/. Use when integrating a new API (OpenAI, OpenRouter, fal, etc.) or switching which provider a pipeline stage uses.
---

# Add / swap a provider

Providers are separate from workflow logic (AGENTS.md principle 3). Workflow arms call `renderer::*`, and only `renderer.rs` calls `api::*`.

## Steps
1. Create `src/api/<name>.rs` and add `pub mod <name>;` to `src/api/mod.rs`.
2. Match the signature of the slot you're filling:
   - LLM: `pub async fn generate_script(client: &Client, system: &str, user: &str, is_json: bool) -> Result<String, String>`
   - Image: `pub async fn generate_image(client: &Client, prompt: &str) -> Result<Vec<u8>, String>`
   - TTS: `pub async fn generate_tts(client: &Client, text: &str, voice: &str) -> Result<Vec<u8>, String>`
3. Read the key with `env::var("X_API_KEY").map_err(|_| "Missing X_API_KEY")?` and add it to `.env.example`.
4. Check `response.status()` and put the response body in the error string. Never `unwrap()`.
5. Switch the call site in `src/renderer.rs`: `build_content` (LLM), `generate_scene_visual` (image), or `generate_scene_audio` (TTS).
6. Callers run `detect_media` on the bytes. For a new format, extend `helper::detect_media`.
7. `cargo check`.

## Reference implementations
- `src/api/gemini.rs`: LLM with JSON mode
- `src/api/cloudflare.rs`: image returned as base64 JSON
- `src/api/pollinations.rs`: image returned as raw bytes via GET
- `src/api/elevenlabs.rs`: TTS that uses `CONFIG.tts`

Don't log API keys or put them in URLs that get printed in errors.
