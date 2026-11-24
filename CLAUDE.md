# CLAUDE.md

Architecture and agent contracts live in @AGENTS.md. This file covers how the code is actually laid out and how to work in it.

## Commands

```bash
cargo build                 # compile
cargo check                 # fast type-check (use after every edit)
cargo clippy                # lints
cargo test                  # unit tests (profiles, characters, workflow, DB, media sniffing, backoff, fades)
cargo test -- --ignored     # real FFmpeg render test on generated assets (needs ffmpeg, no API calls)
cargo run                   # start the scheduler loop (runs forever, calls paid APIs)
STUDIO_PROFILE=<id> cargo run  # pick a profile from src/profiles.rs (default: kids_edu)
APP_DEBUG=True cargo run    # debug: auto-revive sessions that hit max_retry instead of waiting
```

- Requires `ffmpeg` and `ffprobe` on PATH, and a `.env` copied from `.env.example`.
- Uses `Duration::from_mins`, so it needs a recent stable Rust (1.91+).
- Tests live next to the code (`#[cfg(test)]` modules in `profiles.rs`, `characters.rs`, `workflow.rs`, `db.rs`, `helper.rs`, `renderer.rs`). Verify with `cargo check` and `cargo test`; run `cargo test -- --ignored` after touching FFmpeg code. Don't `cargo run` without asking: it spends API credits and can upload to YouTube/TikTok.

## Code map

| Pipeline agent | Where it lives |
|---|---|
| Scheduler (outer loop, retry/backoff, session resume) | `src/scheduler.rs` |
| Planner / Writer / Builder / Renderer / Publisher (state machine) | `src/workflow.rs` → `run_agent_workflow`, one `match` arm per `AgentNode` |
| LLM, image, TTS provider selection + FFmpeg rendering | `src/renderer.rs` (`build_content`, `generate_scene_visual`, `generate_scene_audio`, `ffmpeg_render`) |
| Uploads | `src/uploader.rs` |
| Provider HTTP clients | `src/api/*.rs` (one file per provider, registered in `src/api/mod.rs`) |
| Studio profiles (video types, selection) | `src/profiles.rs` (`StudioProfile`, `PROFILES`, `active_profile`, `cast()`) |
| Characters: `Character` struct, loader, validation | `src/characters.rs` (`init`, `cast_for`) |
| Character data, one TOML per character | `data/characters/<profile_id>/*.toml` (reference: `data/characters/README.md`) |
| State, artifact schema | `src/models.rs` (`VideoState`, `VideoArtifact`, `Scene`, `VideoTimeline`) |
| Enums (`AgentNode`, `Motion`, `Transition`, `VoiceMode`) | `src/enums.rs` |
| Static config (output video size/fps, base voice, TTS, feature flags) | `src/config.rs` → `CONFIG` |
| Persistence (SQLite `data_history.db`) | `src/db.rs` |

## Current provider wiring

- Script LLM: `gemini::generate_script` (gemini-2.5-flash), called via `renderer::build_content`
- Image: `cloudflare::generate_image` (flux-1-schnell), called via `renderer::generate_scene_visual`
- TTS: `elevenlabs::generate_tts`, called via `renderer::generate_scene_audio`. Max 2 concurrent requests (semaphore, ElevenLabs free tier)
- Unused but present: `fal`, `hugging_face`, `pollinations`, `microsoft_edge`, `googleapis`

Swapping a provider means changing one call in `src/renderer.rs`. Workflow code never calls `api::*` directly. Keep it that way.

## State & resume semantics

- `VideoState` is serialized as JSON into `workflow_state` after each node (`db.save_state(&state.reset())`). On restart the scheduler resumes the most recent unfinished session for `APP_VERSION`.
- Bump `APP_VERSION` in `src/db.rs` when changing `VideoState` or artifact fields in an incompatible way. Otherwise old rows fail to deserialize and get silently ignored.
- Not deployed yet: there is no legacy data to stay compatible with. Still bump `APP_VERSION` on incompatible `VideoState` changes so local in-flight sessions are ignored instead of failing to load.
- A session keeps the profile it started with; a changed `STUDIO_PROFILE` only affects the next new session.
- Topic dedup lives in `topic_history (id, profile, topic, created_at)`: last 20 topics per profile, saved only after a successful End.
- Assets are idempotent: the Builder skips existing `images/` and `audios/` files, and the Renderer skips existing `videos/scene_N.mp4` and `final_video.mp4`. Delete them to force regeneration.
- Output path: `./output/{YYYYMMDD}_{profile_id}/{session_id}/{images,audios,videos}/scene_N.*` plus `final_video.mp4`. Frame size and fps come from `CONFIG.video` (1080x1920, 25 fps).
- Asset and render outputs are written to `<path>.part` and renamed (`helper::write_atomic`), so a crash never leaves a truncated file that the skip-if-exists logic would reuse.

## Conventions

- Fallible functions return `Result<T, String>` (provider/workflow) or `anyhow::Result` (db). Prefix errors with `helper::ctx(node, err)`.
- Each `src/api/*.rs` reads its own env var and returns `Err("Missing X")` if it isn't set. Add new keys to `.env.example`.
- Validate provider bytes with `helper::expect_media(bytes, MediaType::X, "what")` before writing. Parse provider HTTP responses with `api::read_json` / `api::read_bytes` (status check, body in the error) and map transport errors with `api::send_err` (strips the URL). Never put API keys in URLs.
- LLM-facing enums (`Motion`, `Transition`) need matching `ALL`, `Display` and `FromStr` entries. The Writer prompt is built from `ALL`.
- Logs use emoji-prefixed `println!` per node (`🧠[Planner]`, `✍️ [Writer]`, …). Match that style.
- Never `unwrap()` LLM output. Use the `json_str` / `json_str_array` helpers in `workflow.rs` and validate character names against `profile.character()`.
- Characters are data: never hardcode a cast in Rust; add a TOML file under `data/characters/<profile_id>/` (loaded at startup, no rebuild; see `data/characters/README.md`).
- Never hardcode audience, format, language or publish metadata in workflow/uploader code. Put it in the profile.
- Profile ids are persisted (output folders, `topic_history`, saved sessions): don't rename them.
- Every workflow arm ends with `db.save_state(&state.reset())` after moving `current_node` forward.

## Skills

Project skills in `.claude/skills/`, one per agent plus cross-cutting tasks:
`planner-agent`, `writer-agent`, `builder-agent`, `renderer-agent`, `uploader-agent`, `add-profile`, `add-provider`, `debug-session`.
