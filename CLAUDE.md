# CLAUDE.md

Architecture and agent contracts live in @AGENTS.md. This file covers how the code is actually laid out and how to work in it.

## Commands

```bash
cargo build                 # compile
cargo check                 # fast type-check (use after every edit)
cargo clippy                # lints
cargo run                   # start the scheduler loop (runs forever, calls paid APIs)
APP_DEBUG=True cargo run    # debug: auto-revive sessions that hit max_retry instead of waiting
```

- Requires `ffmpeg` and `ffprobe` on PATH, and a `.env` copied from `.env.example`.
- Uses `Duration::from_mins`, so it needs a recent stable Rust (1.91+).
- No test suite yet. Verify with `cargo check`. Don't `cargo run` without asking: it spends API credits and can upload to YouTube/TikTok.

## Code map

| Pipeline agent | Where it lives |
|---|---|
| Scheduler (outer loop, retry/backoff, session resume) | `src/scheduler.rs` |
| Planner / Writer / Builder / Renderer / Publisher (state machine) | `src/workflow.rs` → `run_agent_workflow`, one `match` arm per `AgentNode` |
| LLM, image, TTS provider selection + FFmpeg rendering | `src/renderer.rs` (`build_content`, `generate_scene_visual`, `generate_scene_audio`, `ffmpeg_render`) |
| Uploads | `src/uploader.rs` |
| Provider HTTP clients | `src/api/*.rs` (one file per provider, registered in `src/api/mod.rs`) |
| State, characters, artifact schema | `src/models.rs` (`VideoState`, `EDU_CHARACTERS`, `VideoArtifact`, `Scene`, `VideoTimeline`) |
| Enums (`AgentNode`, `Motion`, `Transition`, `VoiceMode`) | `src/enums.rs` |
| Static config (language, voice, TTS, feature flags) | `src/config.rs` → `CONFIG` |
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
- New fields on serialized structs should be `Option<T>` or `#[serde(default)]` so old sessions still load.
- Assets are idempotent: the Builder skips existing `images/` and `audios/` files, and the Renderer skips existing `videos/scene_N.mp4` and `final_video.mp4`. Delete them to force regeneration.
- Output path: `./output/{YYYYMMDD}_{age:03}/{session_id}/{images,audios,videos}/scene_N.*`.

## Conventions

- Fallible functions return `Result<T, String>` (provider/workflow) or `anyhow::Result` (db). Prefix errors with `helper::ctx(node, err)`.
- Each `src/api/*.rs` reads its own env var and returns `Err("Missing X")` if it isn't set. Add new keys to `.env.example`.
- Validate provider bytes with `helper::detect_media` before writing.
- LLM-facing enums (`Motion`, `Transition`) need matching `ALL`, `Display` and `FromStr` entries. The Writer prompt is built from `ALL`.
- Logs use emoji-prefixed `println!` per node (`🧠[Planner]`, `✍️ [Writer]`, …). Match that style.
- Never `unwrap()` LLM output. Use the `json_str` / `json_str_array` helpers in `workflow.rs` and validate character names against `EDU_CHARACTERS`.
- Every workflow arm ends with `db.save_state(&state.reset())` after moving `current_node` forward.

## Skills

Project skills in `.claude/skills/`, one per agent plus cross-cutting tasks:
`planner-agent`, `writer-agent`, `builder-agent`, `renderer-agent`, `uploader-agent`, `add-provider`, `debug-session`.
