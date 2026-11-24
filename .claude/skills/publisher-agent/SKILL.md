---
name: publisher-agent
description: Work on the Publisher agent and uploaders (src/agents/publisher.rs, src/uploader.rs, src/uploaders/, src/oauth.rs): YouTube upload, YouTube OAuth, TikTok, profile publish metadata. Use for upload failures, OAuth problems or adding a channel.
---

# Publisher agent and uploaders

**Code**: `src/agents/publisher.rs`, `src/uploader.rs` (`Uploader` trait, `UploaderManager`, `DistributionReport`), `src/uploaders/youtube.rs`, `src/uploaders/tiktok.rs`, `src/oauth.rs`. **Pool**: 2.

## Contract
- Input: `FinalCut`. Output (handoff to Cleaner): `DistributionReport { all_uploaded, any_uploaded, errors: { youtube, tiktok } }`.
- If no uploader succeeded the agent returns an error (retry with backoff, then the production fails).
- Metadata comes from the production's profile: `publish.youtube_category`, `tags`, `description`, `made_for_kids`. Never hardcode them.

## YouTube
- Title: `youtube_title` removes `<` `>`, trims, max 100 chars, `Untitled` if empty. Privacy is always `public`.
- OAuth (`src/oauth.rs`): on startup, if `get_youtube_token` finds no row, `start_oauth_server` saves the client (`YOUTUBE_CLIENT_ID`/`YOUTUBE_CLIENT_SECRET`), prints the Google URL, opens the browser and serves `127.0.0.1:8080/auth/callback`. The code is exchanged for tokens stored in `oauth_tokens` (provider `youtube`). `get_youtube_token` refreshes when the token expires within 5 minutes.
- Redirect URI `http://localhost:8080/auth/callback` must be registered in the Google OAuth client.

## TikTok
- `TiktokUploader` (`TIKTOK_ACCESS_TOKEN`, single-chunk upload) exists but is **not registered** in `AppState::build`, so the Publisher skips it. To enable: `uploaders.register(Arc::new(uploaders::TiktokUploader::new()))`.

## Adding a channel
Add a `Channel` variant and `ChannelConfig` variant, an `Uploader` impl, a field in `UploaderError`, registration in `main.rs`, and a config entry in the Publisher.

## Gotchas
- Never log tokens; strip URLs from reqwest errors (`e.without_url()`).
- Authorization counts as done only when a refresh token is stored (`oauth::has_auth_token`). Until then every startup reruns the OAuth flow, and uploads fail with "YouTube is not authorized yet".
- `YOUTUBE_REFRESH_TOKEN` (optional) is stored as the login at startup when no login exists yet (`oauth::seed_from_env`), skipping the browser flow.

## Verify
`cargo check`, `cargo test` (`youtube_title_is_sanitized`). Never upload for real or run the OAuth flow without the user's OK: uploads are public.
