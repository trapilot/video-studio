---
name: add-profile
description: Add a new studio profile (video type) in src/entities/profile.rs, optionally with a cast in data/characters/<id>/. Use when the user wants a new kind of video.
---

# Add a studio profile

A profile is one kind of video. Agent code is format-agnostic: do not edit agents for a new type.

## Steps
1. In `src/entities/profile.rs`, append a `StudioProfile` to `PROFILES`. Fields: `id` (lowercase slug, `[a-z0-9_]`, unique, never renamed later), `name`, `format`, `audience`, `tone`, `topic_brief`, `language`, `visual_style`, `duration_secs: (u16, u16)`, `shot_count: (u8, u8)`, `content_rules: &[&str]`, `publish: PublishProfile { description, tags, youtube_category, made_for_kids }`.
2. Cast (optional): create `data/characters/<id>/` with `NN_name.toml` files, at least one `spotlight = true`. See `data/characters/README.md`. Narrator-only: create nothing.
3. `made_for_kids: true` for anything aimed at children.
4. `cargo check` and `cargo test` (a test validates ids, durations and shot counts; the character loader test checks the repo data).
5. Select with `STUDIO_PROFILE=<id>`. Unknown ids stop startup. Only new workflows use it; running workflows keep their stored profile.

## Guidance
- `format` completes "producing ...". `topic_brief` completes "Create ...". Both are pasted into prompts.
- Shots are about 3-5 s of speech: choose `shot_count` consistent with `duration_secs`.
- `visual_style` prefixes every image prompt; the prompt already ends with "no text, no captions, no logos".
- Character folder names must equal a profile id or startup fails.

## Verify
`cargo test`. Do not `cargo run` (paid APIs, public uploads) without the user's OK.
