# AGENTS.md

## Overview

Video Studio is a job-queue pipeline of single-purpose agents. It produces short vertical videos of any type and publishes them to YouTube Shorts. The kind of video is not hard-coded: it is defined by a **studio profile** (`src/entities/profile.rs`) that the Planner, Writer, Builder and Publisher read.

Agents never call each other and share no in-memory state. Each agent reads one JSON payload from its job, does one piece of work and hands the result to the next agent by creating the next job (`db.handoff_job`). All state lives in SQLite (`data_history.db`).

```text
Manager -> Planner -> Writer -> Builder -> Renderer -> Publisher -> Cleaner
           StoryContext  Storyboard  Timeline  VideoMetadata  UploaderState
```

---

## Engine

`WorkflowEngine` (`src/workflow.rs`) owns one `AgentEntry` (an `Agent` implementation and an `AgentPool`) per `AgentType` (`src/agent.rs`).

| Agent | Pool concurrency |
|---|---|
| Manager | 1 |
| Planner | 10 |
| Writer | 2 |
| Builder | 1 |
| Renderer | 1 |
| Publisher | 2 |
| Cleaner | 1 |

**Scheduler task** (`start_scheduler`): loops every 30 seconds. If `db.agent_is_busy(Planner)` is false (no Planner job pending or processing) and `db.count_workflows_today()` (UTC day) is below `config.workflow_per_day` (3), it calls `db.create_workflow(profile.id, "Create {profile.format}")` for the active profile (`STUDIO_PROFILE`) and creates the first job: parent `Manager`, agent `Planner`, payload `"Create new workflow"`. The Manager agent is therefore a placeholder that no job addresses.

**Dispatch loop** (`dispatch`): calls `db.claim_job()`, which atomically moves the oldest pending job (`version = APP_VERSION`, `locked_at IS NULL`, `run_after` empty or past) to `processing`. It looks up the agent entry and calls `AgentPool::try_spawn`. If the pool is full the job goes back with `db.release_job` (retry count untouched). With no job the loop sleeps 5 s, and 1 s between jobs.

**AgentPool** (`src/agent.rs`): a semaphore of the pool's size. `try_spawn` runs `agent.run(state, job)` in a task. On `Err`:

- `retry_count < max_retry` (3): `db.retry_job` sets the job back to `pending`, increments `retry_count` and sets `run_after` to now plus `db::retry_delay_secs(retry_count)` (60 s, doubling, capped at 3600 s). The error text is stored in `jobs.result`.
- otherwise: `db.fail_job` marks the job `failed` and the workflow `failed`.

**Recovery**: at startup `main.rs` calls `db.recover_interrupted_jobs()`, which turns every `processing` job back into `pending`.

**Completion**: the Cleaner marks the workflow `completed`.

**Handoff**: `db.handoff_job(job, next, payload)` inserts a job `{workflow_id, parent: job.agent, agent: next, payload}` and completes the current job with the payload as its result.

### Tables (`migrations/schema_01.sql`)

| Table | Holds |
|---|---|
| `workflows` | `id`, `profile`, `task`, `topic`, `status` (`pending`/`running`/`completed`/`failed`) |
| `jobs` | `id`, `workflow_id`, `agent`, `parent`, `version`, `status`, `payload`, `result`, `retry_count`, `max_retry` (3), `locked_at`, `run_after` |
| `providers` | reserved, not read by code |
| `oauth_tokens` | OAuth tokens, `provider` is the primary key (`youtube`) |

---

## Shared types

| Type | File | Purpose |
|---|---|---|
| `AgentType`, `Agent`, `AgentPool`, `AgentError` | `src/agent.rs` | Agent contract. `AgentError` has `Handoff`, `Execute`, `Encode`, `Decode`. |
| `job_profile(state, job)` | `src/agent.rs` | The `&'static StudioProfile` stored with the job's workflow. |
| `Job`, `Workflow`, `JobStatus`, `WorkflowStatus` | `src/models/job.rs` | Queue rows. `Job::workflow_path()` is the output folder. |
| `StoryContext`, `Storyboard`, `Shot`, `Actor`, `Dialogue`, `Environment`, `Camera` | `src/entities/movie.rs` | Planner and Writer output. |
| `Timeline`, `Clip`, `VideoMetadata` | `src/entities/movie.rs` | Builder and Renderer output. |
| `Motion`, `Transition`, `Position` | `src/entities/movie.rs` | LLM-facing enums (lowercase names), each with an `ALL` list. |
| `UploaderState`, `UploaderError`, `Channel`, `ChannelConfig` | `src/uploader.rs` | Publisher output and upload config. |
| `StudioProfile`, `PublishProfile` | `src/entities/profile.rs` | Per-video-type settings. |
| `Character` | `src/entities/character.rs` | Cast member loaded from TOML. |
| `AppState` | `src/main.rs` | `services` (db, providers, uploaders), `config`, `profile` (profile for new workflows). |

---

## Agents

### Manager

- **File**: `src/agents/manager.rs`
- **Purpose**: reserved for workflow orchestration. Today it only prints a line.
- **Input / output**: none. The scheduler creates the workflow and the Planner job itself.

### Planner

- **File**: `src/agents/planner.rs`
- **Purpose**: choose a topic (and, with a cast, the characters) for a new video.
- **Input**: payload ignored. Profile from `job_profile`.
- **Provider**: Gemini (`ProviderRequest::Script`, JSON mode).
- **Profile usage**: `name`, `format`, `audience`, `tone`, `language`, `topic_brief`; the cast (`enabled_cast`) for the spotlight and relation character lists.
- **Process**:
  1. Reads topics of this profile from the last 30 days (`get_recent_topics`, failed workflows excluded) to avoid repeats.
  2. With a cast, lists spotlight characters (name, topic domain, age, personality, relations) and relation characters (name, profession, age, role, personality, relations).
  3. Parses the answer as `StoryContext` and runs `validate_story_context`: the topic must be non-empty. With a cast, `main_character` must be an enabled spotlight character, and every listed name must resolve with `profile.character()` (full name or nickname). Names are stored as full names. Without a cast all character fields are cleared.
  4. Hands off to the Writer, then saves the topic on the workflow (`save_topic`).
- **Output**: `StoryContext`

```json
{
  "topic": "Why is the sky blue?",
  "main_character": "Quốc Khánh",
  "spotlight_characters": ["Quốc Khánh"],
  "supporting_characters": ["Nam"]
}
```

### Writer

- **File**: `src/agents/writer.rs`
- **Purpose**: turn the topic into a storyboard of short shots.
- **Input**: `StoryContext`
- **Provider**: Gemini (JSON mode).
- **Profile usage**: `name`, `format`, `audience`, `tone`, `visual_style`, `language`, `duration_secs`, `shot_count`, `content_rules`, and the cast.
- **Process**:
  1. With a cast, adds a "CHARACTERS IN THIS STORY" block: `Character::describe()` of each chosen character, which prints every non-empty field plus `Appearance` (the `visual_template` with `{age}` filled in). Without a cast it instructs the model to use the speaker `Narrator` and allows empty `actors`.
  2. Builds numbered rules: generic rules (language, length, shot count, one visual idea per shot, 1-2 sentences per shot, at least one dialogue, no written text in visuals, vertical framing), then cast rules, then `profile.content_rules`.
  3. Lists the allowed motions, transitions and positions from `Motion::ALL`, `Transition::ALL`, `Position::ALL`.
  4. Parses the answer with `Storyboard::from_llm_json` (see below).
- **Output**: `Storyboard`

```json
{
  "title": "The Blue Sky",
  "shots": [
    {
      "shot_id": 1,
      "transition": "fade",
      "motion": "zoomin",
      "environment": { "location": "backyard", "time_of_day": "day", "weather": "clear" },
      "camera": { "shot_type": "wide", "angle": "low", "movement": "static" },
      "visual_prompt": "A boy looks up at a bright blue sky",
      "actors": [
        { "character": "Quốc Khánh", "position": "center", "action": "pointing up", "emotion": "amazed" }
      ],
      "dialogues": [
        { "character": "Quốc Khánh", "text": "Why is the sky blue?" }
      ]
    }
  ]
}
```

`Storyboard::from_llm_json` is lenient: motion, transition and position names match case-insensitively and ignoring `_`, `-` and spaces (`ZoomIn`, `zoom_in` become `zoomin`); unknown values fall back to `none`, `fade` and `center`; empty dialogue lines are dropped; shots without dialogue are removed; shots are sorted and renumbered 1..n (ids become folder names). No remaining shot is an error.

`Shot::image_prompt(profile)` builds the image prompt: `profile.visual_style`, the Writer's `visual_prompt`, `Characters:` with each actor's `visual_anchor()` (looked up with `profile.character`), action, emotion and position, the setting, the camera, and "vertical 9:16 composition, subject centered, no text, no captions, no logos". It is capped at 2000 characters.

### Builder

- **File**: `src/agents/builder.rs`
- **Purpose**: generate the assets of every shot and compute the timing.
- **Input**: `Storyboard`
- **Providers**: Cloudflare Workers AI through `ProviderManager` for images. Speech: at runtime the Builder checks that `./tools/piper/piper` and `./tools/models/en_US-lessac-medium.onnx` both exist. If so it runs local Piper; otherwise it logs that and uses ElevenLabs through the provider manager (`ELEVEN_LABS_KEY_n`, concurrency 2).
- **Profile usage**: `shot.image_prompt(profile)`; in ElevenLabs mode `profile.character(speaker)` to find the speaker's `voice_id`.
- **Process** (shots run concurrently, bounded by provider concurrency):
  1. Creates `{output}/shot_N/`.
  2. If `image.png` is missing: generates the image, rejects non-image bytes (`helper::is_image`), writes atomically.
  3. If `audio.wav` is missing: synthesizes each dialogue line, with Piper (up to 4 at a time) or ElevenLabs (22.05 kHz 16-bit mono PCM wrapped as WAV; voice is the speaker's `voice_id`, else `config.voice.base_voice`; `config.tts` settings apply), then concatenates the WAV segments with `hound` into `audio.wav.part`, renames it, removes the segments. All segments must have the same WAV format.
  4. Measures the audio length with `ffprobe`.
  5. Computes the timeline. Each clip lasts `speech + Transition::DURATION` (2 s). Speech starts after half of that. Consecutive clips overlap by `Transition::fade_between(prev, current)`, the same value the Renderer uses for `xfade`, so subtitle times match the final video.
- **Output**: `Timeline`

```json
{
  "title": "The Blue Sky",
  "final_path": "./output/20260101/<workflow_id>/final_video.mp4",
  "video_path": "./output/20260101/<workflow_id>/video.mp4",
  "subtitle_path": "./output/20260101/<workflow_id>/subtitle.ass",
  "clips": [
    {
      "shot_id": 1,
      "audio_path": ".../shot_1/audio.wav",
      "visual_path": ".../shot_1/image.png",
      "video_path": ".../shot_1/video.mp4",
      "start_time": 1.0,
      "end_time": 4.2,
      "duration": 5.2,
      "subtitle": "Why is the sky blue?",
      "acrossfade": 0.0,
      "transition": "fade",
      "motion": "zoomin"
    }
  ]
}
```

### Renderer

- **File**: `src/agents/renderer.rs`
- **Purpose**: render the final video with FFmpeg.
- **Input**: `Timeline`
- **Profile usage**: none. Frame size and fps come from `config.video` (1080x1920 at 25 fps).
- **Process**:
  1. Checks that `ffmpeg` and `ffprobe` run; rejects an empty timeline.
  2. If `final_video.mp4` exists, returns immediately.
  3. For each clip without `shot_N/video.mp4`: scales and center-crops the image to the frame, applies the `zoompan` filter of the shot's `Motion`, delays the speech by half the transition and pads it with silence, encodes to `video.mp4.part` and renames.
  4. Joins the clips: one clip is re-muxed; several are joined with `xfade` (the previous clip's transition) and `acrossfade` of `Transition::fade_between`.
  5. Writes `subtitle.ass` (one line per clip between `start_time` and `end_time`) and burns it in, producing `final_video.mp4` through `.part`.
- **Output**: `VideoMetadata`

```json
{ "title": "The Blue Sky", "video_path": "./output/20260101/<workflow_id>/final_video.mp4" }
```

### Publisher

- **File**: `src/agents/publisher.rs`
- **Purpose**: upload the finished video.
- **Input**: `VideoMetadata`
- **Profile usage**: `publish.youtube_category`, `publish.tags`, `publish.description`, `publish.made_for_kids`.
- **Process**: builds a YouTube config from the profile and a TikTok config, then `UploaderManager::upload_all` runs the registered uploaders concurrently. Only YouTube is registered (`main.rs`), so TikTok is skipped. The YouTube uploader gets a token with `oauth::get_youtube_token` (refreshing it when it expires within 5 minutes), sanitizes the title (no `<` or `>`, at most 100 characters, `Untitled` if empty) and uploads with `privacyStatus: "public"`. If no uploader succeeded the agent returns an error (retry). Per-channel errors are recorded in the output.
- **Output**: `UploaderState`

```json
{ "all_uploaded": true, "any_uploaded": true, "errors": { "youtube": null, "tiktok": null } }
```

### Cleaner

- **File**: `src/agents/cleaner.rs`
- **Purpose**: close the workflow.
- **Input**: `UploaderState`
- **Process**: prints any per-channel error, completes its job with `DONE` and sets the workflow status to `completed`. It does not delete files.
- **Output**: none.

---

## Providers

`ProviderManager` (`src/provider.rs`) hides provider keys, concurrency and quota handling from agents.

| Provider | Client | Use | Keys (env) | Concurrency |
|---|---|---|---|---|
| `Gemini` | `providers/gemini.rs` | `ProviderRequest::Script` | `GEMINI_KEY_n` | 2 |
| `CFWorker` | `providers/cf_worker.rs` | `ProviderRequest::Image` | `CF_WORKER_KEY_n` + `CF_WORKER_ACCOUNT_n` | 10 |
| `ElevenLabs` | `providers/eleven_labs.rs` | `ProviderRequest::Audio` (speech fallback when Piper is not installed) | `ELEVEN_LABS_KEY_n` | 2 |

Agents use `manager.acquire(&Provider::X)` then `guard.call(request)`. `acquire` returns `None` for an unregistered provider, a provider with no keys or an open circuit; it waits when the concurrency limit is reached or all keys are blocked. `call` uses the key `acquire` picked and, on a quota error (`RequestExceeded`), blocks that key for `retry_after` and tries the next one. Errors are mapped to `ProviderError`. Keys are never logged and never put in URLs (Gemini uses the `x-goog-api-key` header).

---

## Profiles

A `StudioProfile` is one kind of video. See the README for the field table and `src/entities/profile.rs` for the built-ins `kids_edu` (default, family cast, made for kids, category 27) and `explainer` (narrator-only, category 28).

- `STUDIO_PROFILE` selects the profile of **new** workflows. An unknown id stops startup.
- `workflows.profile` stores the id. Agents call `job_profile(state, job)` and never read `state.profile`, which is only for creating workflows.
- The cast is data: `data/characters/<profile_id>/*.toml` (`STUDIO_DATA_DIR` overrides `./data`), loaded once at startup by `entities::character::init`. See `data/characters/README.md`.
- Helpers: `StudioProfile::cast()`, `enabled_cast()`, `has_cast()`, `character(name)` (full name or nickname, enabled characters only), `profile_by_id`, `active_profile`.

---

## Design principles

1. **One responsibility per agent.** Plan, write, build, render, publish, clean up. Do not merge stages.
2. **Agents communicate only through job payloads.** The payload is a serialized struct (`StoryContext`, `Storyboard`, `Timeline`, `VideoMetadata`, `UploaderState`). Anything an agent needs about the workflow, such as its profile, is read from the database with the workflow id.
3. **Providers sit behind `ProviderManager`.** Agents ask for a `Provider` and send a `ProviderRequest`. They do not build HTTP calls to LLM or image APIs.
4. **Profile-driven and format-agnostic.** Audience, tone, language, visual style, length, rules and publish metadata come from the profile. Characters come from data files. Agent code does not name a specific show or character.
5. **Errors, not panics.** Agents return `AgentError`. Never `unwrap()` LLM output, provider responses or files. A returned error becomes a retry with backoff and finally a failed workflow.
6. **Idempotent and atomic.** A retried job must be safe to run again: skip finished assets, write files through `.part` and rename.

---

## Future agents

Ideas that fit the contract (a new `AgentType`, a payload struct, a registry entry in `WorkflowEngine::new`, a handoff from the previous agent):

- **Reviewer** between Writer and Builder: score or reject a storyboard (safety, length) before spending image credits.
- **Thumbnailer**: generate a cover image from the storyboard.
- **Analytics**: read back view counts and feed topic selection.
- **Cleaner extensions**: delete or archive `output/` folders of completed workflows.
- **Manager**: move workflow creation from the scheduler into the Manager agent.
