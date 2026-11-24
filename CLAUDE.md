# CLAUDE.md

Architecture and agent contracts live in @AGENTS.md. This file covers how the code is laid out and how to work in it.

## Commands

```bash
cargo build                  # compile
cargo check                  # fast type-check (use after every edit)
cargo clippy                 # lints
cargo test                   # unit tests
cargo test -- --ignored      # real FFmpeg render on generated assets (no API calls)
STUDIO_PROFILE=<id> cargo run   # scheduler + agents, runs forever
```

- Do not `cargo run` without asking: it calls paid APIs, may open an OAuth browser flow on :8080 and uploads videos publicly to YouTube.
- Requires `ffmpeg` and `ffprobe` on PATH and a `.env` copied from `.env.example`.
- Speech: local Piper in `tools/` (gitignored) if present, otherwise ElevenLabs (`ELEVEN_LABS_KEY_n`).
- No minimum Rust version is pinned; use a recent stable toolchain.
- Verify with `cargo check` and `cargo test`.

## Code map

| Concern | Where |
|---|---|
| Startup, `AppState`, provider and uploader registration | `src/main.rs` |
| Engine: scheduler task + dispatch loop + pool sizes | `src/studio.rs` (`Studio`) |
| `Agent` trait, `AgentType`, `AgentPool` (retry/fail), `AgentError`, `job_profile` | `src/agent.rs` |
| Agents | `src/agents/{producer,planner,writer,builder,renderer,publisher,cleaner}.rs` |
| Payloads, `Motion`, `Transition`, `Position`, `Storyboard::from_llm_json`, `Shot::image_prompt` | `src/entities/storyboard.rs` |
| Profiles (`StudioProfile`, `PROFILES`) | `src/entities/profile.rs` |
| Characters (TOML loading, validation, `describe`) | `src/entities/character.rs`, data in `data/characters/` |
| Queue, productions, topics, OAuth tokens | `src/db.rs`, `migrations/schema_01.sql` |
| `Job`, `Production`, statuses, `OAuthToken` | `src/models/` |
| Provider manager (keys, round-robin, limits) | `src/provider.rs` |
| Provider clients | `src/providers/{gemini,cf_worker,eleven_labs}.rs` |
| Uploaders and manager | `src/uploader.rs`, `src/uploaders/{youtube,tiktok}.rs` |
| YouTube OAuth (callback server on :8080, refresh) | `src/oauth.rs` |
| Static settings (`video`, `productions_per_day`, `image`, `tts`, `voice`) | `src/config.rs` |
| Atomic writes, `is_image`, `truncate_chars` | `src/helper.rs` |

## Provider wiring

- Script LLM: Gemini `gemini-3.8-flash` (`Provider::Gemini`, `providers::gemini::DEFAULT_GEMINI_MODEL`, override with `GEMINI_MODEL`), used by Planner and Writer. All keys use the same model.
- Image: Cloudflare Workers AI `flux-1-schnell` (`Provider::CFWorker`), used by Builder.
- Speech: Builder checks at runtime for `./tools/piper/piper` and `./tools/models/en_US-lessac-medium.onnx`. Both present: local Piper. Otherwise: ElevenLabs (`Provider::ElevenLabs`, concurrency 2). `voice_id` applies in ElevenLabs mode only.
- Registered in `AppState::build`: Gemini (2), CFWorker (10), ElevenLabs (2). Uploaders: YouTube only; `TiktokUploader` exists but is not registered.

Agents never call HTTP APIs directly. They use `state.services.providers.acquire(&Provider::X)` then `guard.call(request)`. Keep it that way.

## State and resume semantics

- State is the `jobs` and `productions` tables in `data_history.db`. A job payload is the JSON of the previous agent's output; handoff is `db.handoff_job`.
- Failure: `AgentPool` calls `retry_job` (backoff via `jobs.run_after`, 60 s doubling to 1 h, `db::retry_delay_secs`) while `agent::should_retry` allows it (`max_retry` = 3 total attempts), then `fail_job` (job and production `failed`).
- Startup: `recover_interrupted_jobs` requeues jobs left `processing`.
- The Cleaner marks the production `completed`.
- `APP_VERSION` (`src/db.rs`) filters which jobs are claimed and counted busy. Bump it after an incompatible payload change so old jobs are ignored.
- The schema (`migrations/schema_01.sql`) is applied only when the DB file is created. The project is not deployed, so change the schema by editing `schema_01.sql` and deleting your local `data_history.db`.
- New fields on payload structs should be `Option<T>` or `#[serde(default)]` so queued jobs still decode.
- Assets are idempotent: Builder skips existing `shot_N/image.png` and `audio.wav`; Renderer skips existing `shot_N/video.mp4` and `final_video.mp4`. Delete them to force regeneration.
- Output path: `./output/{YYYYMMDD}/{production_id}/` (`db.production_output_path`, dated by the production's creation, local time). The Cleaner deletes intermediates after publishing unless `cleanup.keep_intermediates`.
- Several keys per provider: requests must stay self-contained (full context in each prompt, fixed image seed per production) so any key gives the same result. Bad keys rotate (`RequestUnauthorized`, `KeyRejected`).

## Conventions

- Errors: agents return `AgentError` (`Decode`, `Encode`, `Execute`, `Handoff`); internal helpers return `Result<T, String>`; providers return `ProviderError`. Never panic on external data.
- Parse LLM output leniently and never `unwrap()` it. Unknown enum values fall back (see `Storyboard::from_llm_json`).
- Validate character names with `profile.character(name)` (full name or nickname). Store full names.
- Never hardcode audience, format, language, visual style or publish data. Read them from the profile (`job_profile`). Characters are data files in `data/characters/`, never Rust.
- Profile ids are persisted in `productions.profile` and used as folder names: do not rename them.
- Never log secrets or put keys in URLs (Gemini uses a header). Use `e.without_url()` on reqwest errors.
- Write generated files atomically with `helper::write_atomic`, or `helper::part_path` plus `helper::commit_part`.
- Check provider bytes before saving (`helper::is_image`).
- Logs are emoji-prefixed `println!` per agent (`🧠 [Planner]`, `✍️  [Writer]`, `🔧 [Builder]`, `🎥 [Renderer]`, `📤 [Publisher]`, `📤 [Cleaner]`). Match that.
- LLM-facing enums (`Motion`, `Transition`, `Position`) need an `ALL` entry, `strum` lowercase `Display`/`FromStr`, and serde lowercase. The Writer prompt is built from `ALL`.
- Do not run real uploads or `cargo run` unless the user asks.

## Skills

Project skills in `.claude/skills/`: `planner-agent`, `writer-agent`, `builder-agent`, `renderer-agent`, `publisher-agent`, `add-profile`, `add-provider`, `debug-production`.
