---
name: uploader-agent
description: Work on the Publisher/Uploader stage — YouTube Shorts and TikTok uploads, metadata (title, description, tags, category), and upload-success tracking. Use when changing publishing behavior or adding a platform.
---

# Uploader (Publisher) Agent

**Code:** `AgentNode::Publisher` arm in `src/workflow.rs`. API calls are in `src/uploader.rs` (`upload_to_youtube`, `upload_to_tiktok`).

## Contract
- Input: `state.video_path`, `state.final_json.title` (fallback `state.target_topic`) as the title, `state.profile().publish`.
- Runs both uploads concurrently with `tokio::join!` and records `youtube_uploaded` / `tiktok_uploaded`.
- **Only a YouTube failure fails the stage.** A TikTok failure is just logged.
- On success, moves to `End`. The scheduler then marks the session `done` and saves the topic to `topic_history` for the session's profile.

## Auth / env
- YouTube: OAuth refresh-token flow, using `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN`.
- TikTok: `TIKTOK_ACCESS_TOKEN`.
- Metadata comes from `profile.publish` (`PublishProfile` in `src/profiles.rs`): `description`, `tags`, `youtube_category` (`categoryId`) and `made_for_kids` (sent as `madeForKids` and `selfDeclaredMadeForKids`). Category differs per profile (e.g. `27` Education for `kids_edu`, `28` Science & Technology for `explainer`).
- `made_for_kids` must be `true` for child-directed profiles (COPPA). Don't hardcode metadata in `uploader.rs`.

## Rules
- **Never trigger a real upload without the user's explicit OK.** Uploads are public and outward-facing.
- Platforms whose `*_uploaded` flag is already true are skipped, so a retry never re-posts. The scheduler saves the flags together with the retry. Keep that if you change the Publisher.
- Parse responses with `api::read_json` / `read_bytes` and map transport errors with `api::send_err`. No `unwrap()`: a missing env var is an `Err`. YouTube titles go through `youtube_title` (100 chars, no `<>`).
- To add a platform: add an `upload_to_x` fn in `uploader.rs` that returns `Result<(), String>`, add a `x_uploaded: bool` field with `#[serde(default)]` to `VideoState`, add it to the `join!`, and add its env var to `.env.example`.

Verify with `cargo check`.
