---
name: add-profile
description: Add or change a studio profile (a new type of video: format, audience, tone, visual style, length, rules, cast, publish metadata) in src/profiles.rs. Use when the user wants the studio to produce a new kind of video or tweak an existing one.
---

# Add or change a studio profile

A `StudioProfile` in `src/profiles.rs` defines one kind of video. Planner, Writer and Publisher read it; no workflow code changes are needed for a new type.

## Steps
1. Append a `StudioProfile { .. }` to `PROFILES` (copy `explainer` for narrator-only, `kids_edu` for a profile with a cast).
2. `id`: lowercase slug (`a-z`, `0-9`, `_`), unique. It names output folders, keys `topic_history` and is the `STUDIO_PROFILE` value.
3. `name`, `format`, `audience`, `tone`, `topic_brief`: prompt-ready phrases, pasted into the LLM prompts as written ("producing {format}", "Create {topic_brief}").
4. `language`: language of all content.
5. `visual_style`: image-style prefix for every scene (used when there is no cast; with a cast the main character's `visual_anchor` is used). Add "no text, no logos" if lettering is unwanted.
6. `duration_secs` and `scene_count`: `(min, max)` with min > 0 and min <= max.
7. `content_rules`: short imperative sentences appended to the Writer rules (safety, brand, factual constraints).
8. Cast (not a profile field): for narrator-only (speaker `"Narrator"`, voiced with `CONFIG.voice.base_voice`) create nothing. For a cast, create `data/characters/<id>/` (folder name = profile id) with one `.toml` per character: at least one enabled `spotlight = true`, unique `name`s, and `visual_anchor`s in the profile's art style. `"Narrator"` is reserved. See `data/characters/README.md`.
9. `publish`: `description`, `tags`, `youtube_category`, `made_for_kids`.
   - Categories: 1 Film & Animation, 10 Music, 17 Sports, 20 Gaming, 22 People & Blogs, 23 Comedy, 24 Entertainment, 25 News & Politics, 26 Howto & Style, 27 Education, 28 Science & Technology.
   - `made_for_kids` must be `true` for anything aimed at children (COPPA). Otherwise `false`.
10. Run `cargo test` (`profiles::tests::profiles_are_well_formed` validates every profile; `characters::tests` validate the cast data) and `cargo check`.
11. Try it with `STUDIO_PROFILE=<id> cargo run` only after the user OKs spending API credits and public uploads.

## Rules
- Never rename an existing `id`: folders, `topic_history` rows and saved sessions refer to it.
- Changing an existing profile's prompt text, rules or publish metadata does not need an `APP_VERSION` bump. Changing `VideoState` or artifact fields does.
- Don't put audience, format, language or publish metadata in workflow or uploader code. It belongs in the profile.
- Unfinished sessions keep the profile they started with; a new `STUDIO_PROFILE` applies to the next new session.
