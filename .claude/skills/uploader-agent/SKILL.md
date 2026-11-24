---
name: uploader-agent
description: Work on the Publisher/Uploader stage — YouTube Shorts and TikTok uploads, metadata (title, description, tags, category), and upload-success tracking. Use when changing publishing behavior or adding a platform.
---

# Uploader (Publisher) Agent

**Code:** `AgentNode::Publisher` arm in `src/workflow.rs`. API calls are in `src/uploader.rs` (`upload_to_youtube`, `upload_to_tiktok`).

## Contract
- Input: `state.video_path`, `state.target_topic` (used as the title).
- Runs both uploads concurrently with `tokio::join!` and records `youtube_uploaded` / `tiktok_uploaded`.
- **Only a YouTube failure fails the stage.** A TikTok failure is just logged.
- On success, moves to `End`. The scheduler then marks the session `done` and saves the topic to `video_history`.

## Auth / env
- YouTube: OAuth refresh-token flow, using `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET`, `YOUTUBE_REFRESH_TOKEN`.
- TikTok: `TIKTOK_ACCESS_TOKEN`.
- Metadata defaults (description, tags, category `27` = Education) come from `CONFIG.movie` in `src/config.rs`.

## Rules
- **Never trigger a real upload without the user's explicit OK.** Uploads are public and outward-facing.
- A retry re-runs both uploads, so a YouTube retry can re-post to TikTok. If you change retry behavior, skip platforms whose `*_uploaded` flag is already true.
- To add a platform: add an `upload_to_x` fn in `uploader.rs` that returns `Result<(), String>`, add a `x_uploaded: bool` field with `#[serde(default)]` to `VideoState`, add it to the `join!`, and add its env var to `.env.example`.

Verify with `cargo check`.
