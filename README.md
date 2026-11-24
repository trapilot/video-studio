# Video Studio

Video Studio is an autonomous studio that produces short vertical videos of any kind you define and publishes them to YouTube Shorts. Kids stories, explainers and product promos all run through the same pipeline. Only the **studio profile** changes.

It runs as a job queue. A **production** is one video from topic to upload, made of a chain of jobs. A scheduler creates a production, and a chain of agents (Planner, Writer, Builder, Renderer, Publisher, Cleaner) each pick up one job, do one piece of work and hand the result to the next agent. Everything is stored in SQLite, so a crash or an API failure resumes where it stopped.

> **Warning.** Running the studio calls paid or quota-limited APIs (Gemini, Cloudflare Workers AI) and **uploads videos publicly** to YouTube. Read [Quick start](#quick-start) before the first run.

---

## How it works

```text
                         ┌────────────────────────────────────────────────┐
                         │                Studio                  │
                         │                                                │
 scheduler task ─────────┼─► productions + first job  ──┐                   │
 (every 30 s, up to      │                            ▼                   │
  productions_per_day)      │              ┌──────────────────────┐          │
                         │              │  jobs table (SQLite) │◄───┐     │
                         │              └──────────┬───────────┘    │     │
                         │       dispatch loop:    │ claim_job()    │     │
                         │                         ▼                │     │
                         │   ┌─────────┐ ┌────────┐ ┌─────────┐     │     │
                         │   │ Planner │ │ Writer │ │ Builder │ ... │     │
                         │   │  (x10)  │ │  (x2)  │ │  (x1)   │     │     │
                         │   └────┬────┘ └───┬────┘ └────┬────┘     │     │
                         │        └──────────┴───────────┴── handoff_job() │
                         └────────────────────────────────────────────────┘

Producer -> Planner -> Writer -> Builder -> Renderer -> Publisher -> Cleaner
           Brief  Storyboard  Timeline  FinalCut  DistributionReport
```

- **Scheduler** (`Studio::start_scheduler`, `src/studio.rs`): every 30 seconds, if no Planner job is pending or processing and fewer than `config.productions_per_day` productions were created today (local day), it creates a production for the active profile and its first job.
- **Dispatch loop** (`Studio::dispatch`): claims the oldest runnable job with `db.claim_job()` and runs it in the agent's `AgentPool`. If the pool is full, the job is released with its retry count untouched.
- **Agent pools** (concurrency per agent): Planner 10, Writer 2, Builder 1, Renderer 1, Publisher 2, Cleaner 1, Producer 1.
- **Handoff**: an agent finishes by calling `db.handoff_job(job, next_agent, payload)`. That creates the next job with a JSON payload and marks the current job completed. Agents never call each other.

### The agents

| Agent | Input payload | What it does | Output payload | File |
|---|---|---|---|---|
| Producer | none | Placeholder. The scheduler creates the production and a job already addressed to the Planner (parent `producer`), so the Producer agent never receives a job today. | none | `src/agents/producer.rs` |
| Planner | any (ignored; the profile comes from the production) | Asks Gemini for one topic that fits the profile, avoiding the last 30 days of topics of the same profile. With a cast it also picks the main, spotlight and supporting characters and validates every name. Saves the topic on the production. | `Brief` | `src/agents/planner.rs` |
| Writer | `Brief` | Builds a prompt from the profile (format, audience, tone, visual style, rules, cast) and asks Gemini for a strict JSON storyboard. Parses it leniently. | `Storyboard` | `src/agents/writer.rs` |
| Builder | `Storyboard` | For every shot: generates an image (Cloudflare `flux-1-schnell`) and speech (local Piper, or ElevenLabs when Piper is not installed), measures the audio with `ffprobe` and computes clip and subtitle timing. | `Timeline` | `src/agents/builder.rs` |
| Renderer | `Timeline` | Renders one clip per shot, joins them with `xfade`/`acrossfade`, burns in ASS subtitles. | `FinalCut` | `src/agents/renderer.rs` |
| Publisher | `FinalCut` | Uploads `final_video.mp4` to every registered uploader (YouTube today) using the profile's publish data. | `DistributionReport` | `src/agents/publisher.rs` |
| Cleaner | `DistributionReport` | Logs upload errors, deletes the per-shot files and the un-subtitled `video.mp4` (unless `cleanup.keep_intermediates`), completes its job and marks the production `completed`. | none | `src/agents/cleaner.rs` |

Payload types live in `src/entities/storyboard.rs` (`Brief`, `Storyboard`, `Timeline`, `FinalCut`) and `src/uploader.rs` (`DistributionReport`). See [AGENTS.md](AGENTS.md) for JSON examples and the full contract of each agent.

---

## Studio profiles

A `StudioProfile` (`src/entities/profile.rs`) describes one kind of video: what it is, who it is for, how it looks and sounds, how long it is, which rules apply and how it is published. Profiles are static Rust data in the `PROFILES` array. The recurring cast is not part of the struct. It is loaded from data files (see [Characters](#characters)).

### Fields

| Field | Type | Meaning |
|---|---|---|
| `id` | `&str` | Stable slug (`kids_edu`). Used in `STUDIO_PROFILE`, `data/characters/<id>/` and topic dedup. Lowercase letters, digits and `_` only (checked by a test). Never rename it after producing videos. |
| `name` | `&str` | Human-readable show or channel name. Shown in the startup log and given to the LLM. |
| `format` | `&str` | What the video is, as a phrase that completes "producing ...". Also used as the production task text. |
| `audience` | `&str` | Who watches it. |
| `tone` | `&str` | Voice and mood. |
| `topic_brief` | `&str` | What the Planner comes up with each run ("Create {topic_brief}"). |
| `language` | `&str` | Language for all spoken and written content. |
| `visual_style` | `&str` | Art style placed first in every image prompt, so all shots look consistent. |
| `duration_secs` | `(u16, u16)` | Target total length in seconds (min, max), given to the Writer. |
| `shot_count` | `(u8, u8)` | Number of shots (min, max), given to the Writer. |
| `content_rules` | `&[&str]` | Extra rules appended to the Writer prompt (safety, brand, factual constraints). |
| `publish` | `PublishProfile` | `description`, `tags`, `youtube_category`, `made_for_kids`. |

### Built-in profiles

| id | Name | Format | Cast | YouTube category | Made for kids |
|---|---|---|---|---|---|
| `kids_edu` (default) | Bin's Family Adventures | Animated educational story for children aged 4-8, told by a family cast | `data/characters/kids_edu/` (4 files) | 27 (Education) | yes |
| `explainer` | Quick Explainers | Narrated explainer of one idea in under a minute, for curious adults and teenagers | none (narrator-only) | 28 (Science & Technology) | no |

### Selecting a profile

```bash
STUDIO_PROFILE=explainer cargo run
```

- Unset or empty: `kids_edu`.
- Unknown id: startup stops with an error that lists the known ids.
- `STUDIO_PROFILE` only decides the profile of **new** productions. Each production stores its profile id in `productions.profile`, and every agent reads it back with `agent::job_profile`, so in-flight work keeps the profile it started with.

The startup banner shows what is active:

```text
🚀 Video Studio v0.1.0 | profile: explainer (Quick Explainers) | cast: narrator only
```

---

## Characters

The cast is data, not code. One TOML file per character in `data/characters/<profile_id>/` (override the root with `STUDIO_DATA_DIR`). Files are loaded once at startup, so adding or editing a character needs a restart but no rebuild. Only `name` is required. Unknown fields, duplicate names or nicknames, a cast without a `spotlight = true` character and a folder that matches no profile id all stop startup with the file path and the reason. A profile without a folder, or without enabled characters, is narrator-only (the Writer uses the speaker `Narrator`).

```toml
# data/characters/study_buddies/01_mia.toml
name = "Mia"
nickname = "Mimi"
spotlight = true
birth_year = 2018
personality_prompt = "A curious girl who asks why before every experiment."
visual_template = "a 3D cartoon girl, age {age}, curly brown hair, green raincoat"
```

Full field reference, validation rules and where each field is used: [data/characters/README.md](data/characters/README.md).

---

## Create a new video type

1. Open `src/entities/profile.rs`.
2. Pick an `id`: a short lowercase slug such as `product_promo`. It becomes a folder name and a database value, so choose it once.
3. Write `format`, `audience`, `tone` and `topic_brief` as prompt-ready phrases. They are pasted into the LLM prompts as they are.
4. Write a `visual_style` that works as a prefix for every image prompt: medium, lighting, composition. Mention "no text" if you do not want garbled lettering (the image prompt already ends with "no text, no captions, no logos").
5. Set `duration_secs` and `shot_count`. Keep them consistent: a Writer shot is about 3-5 seconds of speech.
6. Add `content_rules`: short imperative sentences for what must always or never happen.
7. Decide on a cast. For a cast, create `data/characters/<id>/` with one `.toml` file per character (at least one with `spotlight = true`). For narrator-only, create nothing.
8. Fill in `publish`. Set `made_for_kids: true` for anything aimed at children.
9. Append the profile to `PROFILES`, then run `cargo check` and `cargo test` (a test validates ids, durations and shot counts of every profile).
10. Try it with `STUDIO_PROFILE=<id> cargo run`. This spends API credits and publishes publicly, see [Quick start](#quick-start).

### Example: a narrator-only profile

```rust
StudioProfile {
    id: "product_promo",
    name: "Gadget Spotlight",
    format: "a punchy 30 second vertical promo video for a consumer gadget",
    audience: "young adults who shop online",
    tone: "energetic, upbeat, persuasive but honest",
    topic_brief: "a single everyday gadget or accessory and the one problem it solves",
    language: "English",
    visual_style: "clean studio product photograph, soft gradient background, dramatic lighting",
    duration_secs: (25, 35),
    shot_count: (6, 9),
    content_rules: &[
        "Open with the problem in the first shot.",
        "Name one concrete benefit per shot.",
        "Do not invent prices, discounts or statistics.",
        "End with a short call to action.",
    ],
    publish: PublishProfile {
        description: "A quick look at a handy gadget",
        tags: &["gadgets", "product", "review", "shorts"],
        youtube_category: "26",
        made_for_kids: false,
    },
},
```

Run it with `STUDIO_PROFILE=product_promo cargo run`.

---

## Quick start

### Prerequisites

- **Rust**, a recent stable toolchain (edition 2021; no minimum version is pinned in `Cargo.toml`).
- **FFmpeg and ffprobe** on `PATH`.
- **Speech engine**, one of:
  - **Piper** (optional, free, local). Install it in `tools/`; if it is present the Builder uses it. Otherwise set `ELEVEN_LABS_KEY_1` and the Builder falls back to ElevenLabs.
  - **ElevenLabs** (paid/quota-limited): set `ELEVEN_LABS_KEY_1` (and more keys if you have them).

  The `tools/` folder is gitignored (everything except its `.gitignore`), so to use Piper every checkout must provide:
  - the Piper binary at `./tools/piper/piper`: download a Piper release for your platform from the Piper project's releases and extract it so the executable is `tools/piper/piper`;
  - the voice model at `./tools/models/en_US-lessac-medium.onnx`, downloaded from the Piper voice collection. Keep the model's `.onnx.json` config next to it, as Piper expects.
- **API keys**: Gemini (script), Cloudflare Workers AI (images), YouTube OAuth client (uploads), and ElevenLabs unless Piper is installed.

### Run

```bash
cp .env.example .env     # then fill in the keys, see Configuration
cargo check
cargo test
cargo run                # or: STUDIO_PROFILE=explainer cargo run
```

On the first run, if no YouTube token is stored in the database, the studio starts a local callback server on `127.0.0.1:8080`, prints a Google authorization URL and tries to open it in your browser. Approve the `youtube.upload` scope. The tokens are saved in the `oauth_tokens` table and refreshed automatically afterwards. Add `http://localhost:8080/auth/callback` as an authorized redirect URI of your Google OAuth client.

`cargo run` starts the scheduler loop and runs until you stop it. It will create up to `productions_per_day` (3) productions per day, each one spending Gemini and Cloudflare quota and ending in a **public** YouTube upload. Do not run it casually.

---

## Configuration

### Environment variables (`.env`)

| Variable | Used by | Meaning |
|---|---|---|
| `STUDIO_PROFILE` | `main.rs`, `entities::active_profile` | Profile id for new productions. Default `kids_edu`. |
| `STUDIO_DATA_DIR` | `entities::character::init` | Folder that holds `characters/<profile_id>/*.toml`. Default `./data`. |
| `GEMINI_FALLBACK_MODEL` | Gemini provider | Optional. Model tried when the main model answers "high demand" / 503 UNAVAILABLE. |
| `GEMINI_MODEL` | Gemini provider | Model for every Gemini key. Default `gemini-3.8-flash`. Change it when Google retires a model (error `NOT_FOUND ... no longer available`). |
| `GEMINI_KEY_1`, `GEMINI_KEY_2`, ... | Gemini provider | Script LLM keys. Numbered from 1 with no gaps: loading stops at the first missing number. |
| `CF_WORKER_KEY_n` and `CF_WORKER_ACCOUNT_n` | Cloudflare provider | Workers AI token and account id. Each key needs its own account id. |
| `ELEVEN_LABS_KEY_n` | ElevenLabs provider | Speech fallback used by the Builder when Piper is not installed. |
| `YOUTUBE_CLIENT_ID`, `YOUTUBE_CLIENT_SECRET` | `oauth.rs` | Google OAuth client, used on the first run to authorize and later to refresh tokens. |
| `YOUTUBE_REFRESH_TOKEN` | `oauth.rs` | Optional. If set and no login is stored, it is saved as the YouTube login, so the browser flow is skipped (handy on headless machines). Needs the client id and secret. |
| `TIKTOK_ACCESS_TOKEN` | TikTok uploader | Only read if the TikTok uploader is registered (it is not). |

`.env` is loaded with `dotenvy` and is gitignored.

### Static settings (`src/config.rs`, `build_config`)

| Setting | Value | Used by |
|---|---|---|
| `video` | 1080 x 1920 at 25 fps | Renderer (every shot is scaled and center-cropped to it), subtitle layout |
| `productions_per_day` | 3 | Scheduler |
| `image` | `num_steps` 4, `guidance` 3.5 | Builder, passed to Cloudflare flux-1-schnell |
| `tts` | `speed`, `stability`, `similarity_boost` | Builder, ElevenLabs mode only |
| `voice` | `base_voice` | ElevenLabs voice for the narrator and speakers without a `voice_id`. Use a premade voice: custom voices exist only on the account that created them. |
| `video` | `width`, `height`, `fps`, `burn_subtitles` | Output frame (1080x1920 @ 25) and whether subtitles are burned in. The `.ass` file is always written. |
| `cleanup` | `keep_intermediates` | `true` keeps per-shot files and `video.mp4` after publishing (debugging). Default `false`. |

### Constants in code

| Where | Value |
|---|---|
| `db.rs` `APP_VERSION` | `1`. Only jobs with this version are claimed. |
| `db.rs` `DbManager::new` | database file `data_history.db`, created next to where you run the binary |
| Job `max_retry` | 3 (schema default) |
| `Transition::DURATION` | 2.0 s cross-fade, shortened to half of the shorter clip |

---

## Providers

Provider calls go through `ProviderManager` (`src/provider.rs`). Agents ask it for a guard with `acquire(&Provider::X)` and call `guard.call(request)`.

| Provider | Purpose | Used by | Model / endpoint | Keys |
|---|---|---|---|---|
| `Gemini` | Script LLM, JSON mode | Planner, Writer | `gemini-3.8-flash` (override with `GEMINI_MODEL`), key sent in the `x-goog-api-key` header | `GEMINI_KEY_n`, concurrency 2 |
| `CFWorker` | Image generation | Builder | Cloudflare Workers AI `@cf/black-forest-labs/flux-1-schnell` | `CF_WORKER_KEY_n` + `CF_WORKER_ACCOUNT_n`, concurrency 10, 100000 requests per key per day |
| `ElevenLabs` | Text to speech (fallback) | Builder, only when Piper is missing | `eleven_multilingual_v2`, 22.05 kHz 16-bit mono PCM wrapped as WAV | `ELEVEN_LABS_KEY_n`, concurrency 2 |

Speech engine choice happens at runtime in the Builder (`src/agents/builder.rs`): if both `./tools/piper/piper` and `./tools/models/en_US-lessac-medium.onnx` exist **and** the profile's language is English, it runs Piper locally; otherwise it logs why and uses ElevenLabs through the provider manager. Both engines produce the same WAV format, so segments concatenate either way. In ElevenLabs mode each line uses the speaker's `voice_id` (matched by name or nickname in the production's profile), otherwise `config.voice.base_voice`.

How the producer behaves:

- Keys are loaded from the environment at startup (`PREFIX_KEY_1`, `PREFIX_KEY_2`, ...) and used round-robin.
- A key has an in-memory daily counter and an optional daily limit.
- A quota response (`RequestExceeded`) blocks that key for the provider's `retry_after` (5 s) and the call moves on to the next key.
- An invalid or revoked key (`RequestUnauthorized`) is disabled until tomorrow and the call moves on, so one bad key never fails a job.
- A temporary service failure (`Unavailable`: HTTP 500/502/503/504, Gemini "UNAVAILABLE" / "high demand") is the same on every key, so keys are not blocked: the same request is retried after 2 s, 4 s and 8 s (and Gemini tries `GEMINI_FALLBACK_MODEL` if set). If it still fails, the job fails and the queue retries it later with its own backoff.
- An account-specific refusal (`KeyRejected`, e.g. an ElevenLabs `voice_not_found` for a voice that lives on another account) skips that key for an hour and tries the next one.
- Other errors count as failures for a circuit breaker: 5 failures in a row open it for 60 seconds, during which `acquire` returns `None` (the job fails and is retried with backoff). Any successful call closes it.
- `acquire` waits while the provider is at its concurrency limit or every key is blocked or over its limit.
- Error messages strip URLs, and logs print the key number, never the key.

**Several keys, same result.** Every request carries its full context, so it doesn't matter which key answers: Gemini calls are single-turn (the Planner gets topic history from the database, the Writer gets the story and cast in its prompt), Cloudflare image requests use a fixed seed per production (same prompt, same image on any account), and every ElevenLabs line uses the same voice id and settings. Keep custom ElevenLabs voices on every account whose key you add, or use premade voices.

To add a provider, see `.claude/skills/add-provider/SKILL.md`.

---

## Output layout

Each production writes to `./output/{YYYYMMDD}/{production_id}/` (`db.production_output_path`; the date is the production's creation date in local time, so a production that runs past midnight stays in one folder). `output/` is gitignored.

```text
output/
└── 20260101/
    └── 6f1c0b9e-.../              # production id
        ├── shot_1/
        │   ├── image.png          # Builder: Cloudflare image (square)
        │   ├── audio.wav          # Builder: speech (Piper or ElevenLabs), segments concatenated
        │   └── video.mp4          # Renderer: one clip per shot
        ├── shot_2/ ...
        ├── video.mp4              # Renderer: clips joined with transitions
        ├── subtitle.ass           # Renderer: subtitles
        └── final_video.mp4        # Renderer: video + burned-in subtitles (uploaded)
```

---

## Reliability

- **Retries with backoff.** When an agent returns an error, `AgentPool` calls `db.retry_job`: the job goes back to `pending` with `retry_count + 1` and `jobs.run_after` set 60 s, 120 s, 240 s ... later (doubling, capped at 1 hour, `db::retry_delay_secs`). `claim_job` ignores jobs whose `run_after` is in the future.
- **Giving up.** `max_retry` (3) is the total number of attempts (`agent::should_retry`). After the third failed attempt `db.fail_job` marks the job and its production `failed`.
- **Crash recovery.** At startup `recover_interrupted_jobs` puts every job left in `processing` back to `pending`, so a crash never blocks the queue (a stuck Planner job would block new productions).
- **Pool full.** A job claimed while its agent pool is full is released with `release_job`, which keeps its retry count.
- **Completion.** A production becomes `completed` in the Cleaner, `failed` in `fail_job`.
- **Atomic files.** Images, audio, clips and videos are written to `<path>.part` and renamed, so a crash never leaves a truncated file that a later run would skip.
- **Idempotent skips.** The Builder skips shots whose `image.png` or `audio.wav` exists, the Renderer skips existing `shot_N/video.mp4` and an existing `final_video.mp4`. Delete a file to force regeneration.
- **Bad provider output.** The Builder rejects image responses that are not PNG, JPEG or WebP, and the storyboard parser falls back instead of failing on unknown motion, transition or position names.

---

## Project structure

```text
.
├── Cargo.toml
├── .env.example
├── migrations/
│   └── schema_01.sql            # productions, jobs, providers, oauth_tokens
├── data/characters/
│   ├── README.md                # character file reference
│   └── kids_edu/*.toml          # cast of the kids_edu profile
├── tools/                       # gitignored: Piper binary and voice model
├── output/                      # gitignored: generated videos
├── .claude/skills/              # skills for coding agents
└── src/
    ├── main.rs                  # AppState, provider/uploader registration, startup
    ├── studio.rs                  # Studio: scheduler + dispatch loop
    ├── agent.rs                 # Agent trait, AgentType, AgentPool, AgentError, job_profile
    ├── agents/                  # producer, planner, writer, builder, renderer, publisher, cleaner
    ├── entities/
    │   ├── profile.rs           # StudioProfile, PROFILES
    │   ├── character.rs         # Character, TOML loading and validation
    │   └── storyboard.rs             # Brief, Storyboard, Shot, Timeline, Motion, Transition
    ├── models/                  # Job, Production, statuses, OAuthToken
    ├── db.rs                    # DbManager: queue, productions, topics, tokens
    ├── provider.rs              # ProviderManager, ProviderClient, keys, circuit breaker
    ├── providers/               # gemini.rs, cf_worker.rs, eleven_labs.rs
    ├── uploader.rs              # Uploader trait, UploaderManager, DistributionReport
    ├── uploaders/               # youtube.rs, tiktok.rs
    ├── oauth.rs                 # YouTube OAuth flow and token refresh
    ├── config.rs                # static settings
    └── helper.rs                # write_atomic, is_image, truncate_chars
```

### The database

`data_history.db` (SQLite, gitignored). The schema is in `migrations/schema_01.sql` and is applied **only when the database file is created**. The project is not deployed yet, so to change the schema, edit `schema_01.sql` and delete your local `data_history.db`.

---

## Development

```bash
cargo check                # fast type-check, use after every edit
cargo clippy               # lints
cargo test                 # unit tests (profiles, characters, storyboard, planner, renderer ASS, db queue, helpers, YouTube title)
cargo test -- --ignored    # real FFmpeg render on generated assets, no API calls
```

Tests need `ffmpeg` and `ffprobe` on `PATH` only for the ignored render test. No test calls a paid API or uploads.

Do not use `cargo run` to test: it spends credits and uploads publicly.

---

## Known limitations

- **TikTok is not registered.** `TiktokUploader` exists, and the Publisher builds a TikTok config, but `main.rs` registers only the YouTube uploader, so TikTok is skipped silently.
- **Piper speaks English only.** It uses the English `en_US-lessac-medium` model, so the Builder uses Piper only for profiles whose `language` is English; other languages always go to ElevenLabs (`eleven_multilingual_v2`).
- **`voice_id` is ignored by Piper.** With Piper every character speaks with the same voice; per-character voices only work in ElevenLabs mode.
- **Images are square.** flux-1-schnell returns a square picture, which the Renderer center-crops to 9:16, so the Writer is told to keep the main subject centered.
- **Uploads are always public** (`privacyStatus: "public"`).
- **`acquire` can wait forever.** If every key of a provider is blocked or over its daily limit, `acquire` loops until one frees up instead of failing.
- **Producer agent is a placeholder.**

---

## License

MIT, see [LICENSE](LICENSE).
