---
name: add-provider
description: Add or swap an AI provider (LLM, image, TTS) behind ProviderManager: implement ProviderClient in src/providers/, register it in AppState::build, add the Provider enum variant and KEY_n env vars. Use when integrating a new API or changing which provider an agent uses.
---

# Add / swap a provider

Agents never call HTTP APIs directly; they go through `ProviderManager` (`src/provider.rs`).

## Steps
1. Add a variant to `Provider` in `src/provider.rs` (strum lowercase).
2. Create `src/providers/<name>.rs` with `pub struct XClient;` implementing `ProviderClient`: `provider()`, `config()` (`daily_limit`, `retry_after`, ...), `call(&ProviderRequest, &ApiCredential)`, `is_auth_error`, `is_quota_error`. Handle only the request kinds you support (`Script`, `Image`, `Audio`); return `ProviderError::NotSupported` otherwise.
3. Export it in `src/providers/mod.rs`.
4. Register in `AppState::build` (`src/main.rs`): `providers.register_with_keys(Arc::new(providers::XClient), <concurrency>, "X").await`. Keys are read from `X_KEY_1`, `X_KEY_2`, ... (numbered from 1, stops at the first gap) and optional `X_ACCOUNT_n` (becomes `credential.account_id`). Add the names to `.env.example`.
5. Switch the call site in the agent: `state.services.providers.acquire(&provider::Provider::X)` then `guard.call(ProviderRequest::...)` and `into_text()` / `into_bytes()`.
6. Validate bytes before saving (`helper::is_image` for images) and write atomically (`helper::write_atomic`).

## Rules
- Use `self.read_json` / `self.read_bytes` (they map non-2xx to errors and call `is_auth_error`/`is_quota_error`). A quota error blocks that key for `retry_after` and the manager rotates.
- Send keys in headers, never in the URL; map reqwest errors with `e.without_url()`. Never log keys.
- Never `unwrap()` a response.

## Reference implementations
`src/providers/gemini.rs` (JSON script), `src/providers/cf_worker.rs` (base64 image, needs account id), `src/providers/eleven_labs.rs` (audio bytes).

## Verify
`cargo check`, `cargo test`. Real API calls spend credits: ask first.
