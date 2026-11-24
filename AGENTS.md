# AGENTS.md

## Overview

Video Studio is a multi-agent pipeline that produces short-form vertical video of any type and publishes it to YouTube Shorts and TikTok.

Each agent has a single responsibility and communicates through a shared workflow state (`VideoState`, `src/models.rs`). What kind of video is produced is not hard-coded: it is defined by a **studio profile** (`src/profiles.rs`) that the Planner, Writer and Publisher read.

---

## Workflow

Planner → Writer → Builder → Renderer → Publisher → End

The state machine is `run_agent_workflow` in `src/workflow.rs`, one `match` arm per `AgentNode` (`src/enums.rs`). Every arm moves `current_node` forward and calls `db.save_state(&state.reset())`, so a restart resumes at the next stage. The outer loop, retries and session resume are in `src/scheduler.rs`.

---

## Studio profiles

A `StudioProfile` is one kind of video. The active profile is chosen by the `STUDIO_PROFILE` env var (default `kids_edu`; an unknown id stops the program at startup). A session stores the id in `VideoState.profile_id` and keeps that profile until it finishes.

| Field | Used by | Purpose |
|---|---|---|
| `id` | scheduler, state, DB | Stable slug: env value, output folder, `topic_history.profile`. Never rename. |
| `name` | Planner, Writer, logs | Show or channel name. |
| `format`, `audience`, `tone` | Planner, Writer | Prompt text describing what is produced and for whom. |
| `topic_brief` | Planner | What to come up with each run. |
| `language` | Planner, Writer | Language of all content. |
| `visual_style` | Writer | Image style when there is no cast. |
| `duration_secs`, `scene_count` | Writer | Target length and scene range. |
| `content_rules` | Writer | Extra numbered rules appended to the prompt. |
| `publish` | Publisher | `description`, `tags`, `youtube_category`, `made_for_kids`. |

Built-in profiles: `kids_edu` (family cast from `data/characters/kids_edu/`, made for kids, category 27) and `explainer` (narrator-only, general audience, category 28). To add one, see the `add-profile` skill or the README.

The cast is not a profile field. It is data: one TOML file per character in `data/characters/<profile_id>/` (`STUDIO_DATA_DIR` overrides `./data`), loaded once at startup by `characters::init` (`src/characters.rs`). A profile with no folder or no enabled characters is narrator-only. See `data/characters/README.md`.

Helpers: `StudioProfile::cast()` (the loaded characters, in file-name order), `StudioProfile::has_cast()` (any enabled character), `StudioProfile::character(name)` (enabled characters only), `profiles::profile_by_id`, `profiles::active_profile`. `cargo test` validates every profile.

---

## Agents

### Planner Agent

**Purpose**

Choose a topic that fits the profile and has not been used recently. With a cast, also choose the characters.

**Input**

* The profile (`state.profile()`)
* The last 20 topics for this profile: `db.get_recent_topics(&state.profile_id, 20)`

**Output**

The LLM returns JSON; the same shape is used for every profile.

With a cast (`kids_edu`):

```json
{
  "topic": "Why do stars twinkle?",
  "main_character": "Quốc Khánh",
  "spotlight_characters": ["Quốc Khánh", "Hoàng Yến"],
  "supporting_characters": ["Nam"]
}
```

Narrator-only (`explainer`):

```json
{
  "topic": "How does a microwave heat food?",
  "main_character": "",
  "spotlight_characters": [],
  "supporting_characters": []
}
```

The agent sets `target_topic`, `main_character`, `spotlight_characters` and `supporting_characters` on the state.

**What the profile controls**

* The prompt: name, format, audience, tone, language, `topic_brief`.
* Cast or no cast. With a cast (from the character files), the prompt lists spotlight characters (`name`, `topic_domain`, age, `personality_prompt`, `relationship_prompt`) and relation characters (`name`, `profession`, age, `role`, `relationship_prompt`) and requires the main character to be a spotlight character. Names are validated with `profile.character()`; an unknown name returns `Err` and the scheduler retries. Without a cast, whatever the LLM returns for the character fields is discarded and the fields are set empty.

**Location:** `AgentNode::Planner` arm in `src/workflow.rs`.

---

### Writer Agent

**Purpose**

Generate the full script for a short vertical video.

**Input**

* `target_topic` and the character selection
* The profile: format, audience, tone, visual style, duration, scene count, language, `content_rules`
* The cast from `data/characters/`: every non-empty field of each chosen character (nickname, age, role, family role, story function, profession, personality, speaking style, catchphrase, strengths, weaknesses, core values, relations)

**Output**

A `VideoArtifact` stored in `state.final_json`:

```json
{
  "title": "Why Stars Twinkle",
  "scenes": [
    {
      "scene_id": 1,
      "duration": 5,
      "transition": "Fade",
      "motion": "ZoomIn",
      "visual_prompt": "...",
      "voice_segments": [
        {
          "speaker": "Quốc Khánh",
          "text": "..."
        }
      ]
    }
  ]
}
```

After parsing, each segment gets `voice_id`: the cast member's `voice_id` (empty in the file means the base voice), or `CONFIG.voice.base_voice` for any other speaker. `motion` and `transition` must be values from `Motion::ALL` and `Transition::ALL`; the prompt lists them from those arrays.

**What the profile controls**

* Header: name, format, audience, tone.
* Visual style: the main character's `visual_anchor` with a cast (an empty anchor falls back to `profile.visual_style`), otherwise `profile.visual_style`.
* Voice: with a cast, the character descriptions; without, speaker must be `"Narrator"` (`profiles::NARRATOR`) for every segment, speaking to the viewer.
* Rules, numbered in this order: generic rules (language, duration range, scene range, at least one voice_segment per scene, still-image visual prompts, no invented motion or transition names), then cast rules (cast only: use only provided characters as speakers; the main character drives the story; narrator-only: narration only, no dialogue), then `profile.content_rules`.

**Location:** `AgentNode::Writer` arm in `src/workflow.rs`. Schema in `src/models.rs`.

---

### Builder Agent

**Purpose**

Generate one image and one voice-over for each scene and compute scene timings.

**Providers**

* Image: Cloudflare Workers AI, `flux-1-schnell` (`cloudflare::generate_image`)
* Text-to-speech: ElevenLabs (`elevenlabs::generate_tts`), at most 2 concurrent requests
* Duration: measured from the audio file with `ffprobe`

The calls are made from `generate_scene_visual` and `generate_scene_audio` in `src/renderer.rs`. Other clients exist in `src/api/` but are not wired.

**Output**

```text
output/{YYYYMMDD}_{profile_id}/{session_id}/images/scene_1.png
output/{YYYYMMDD}_{profile_id}/{session_id}/images/scene_2.png
output/{YYYYMMDD}_{profile_id}/{session_id}/audios/scene_1.mp3
output/{YYYYMMDD}_{profile_id}/{session_id}/audios/scene_2.mp3
...
```

and `state.video_timelines` (`Vec<VideoTimeline>`). Existing files are skipped, so retries only generate what is missing.

**Location:** `AgentNode::Builder` arm in `src/workflow.rs`, `build_timelines` in `src/renderer.rs`.

---

### Renderer Agent

**Purpose**

Combine images and audio into scene videos and merge them into the final video.

**Technology**

* FFmpeg (`ffprobe` for durations). Each image is animated with a `zoompan` camera motion; clips are joined with `xfade` transitions.

**Output**

```text
output/{YYYYMMDD}_{profile_id}/{session_id}/videos/scene_1.mp4
output/{YYYYMMDD}_{profile_id}/{session_id}/videos/scene_2.mp4
output/{YYYYMMDD}_{profile_id}/{session_id}/final_video.mp4
```

Existing scene clips and `final_video.mp4` are skipped. The path is stored in `state.video_path`.

**Location:** `AgentNode::Renderer` arm in `src/workflow.rs`, `ffmpeg_render` in `src/renderer.rs`; filters in `src/enums.rs`.

---

### Publisher (Uploader) Agent

**Purpose**

Publish the completed video to social platforms.

**Targets**

* YouTube Shorts: title is the script `title` (fallback: topic), max 100 characters, `<` `>` removed; description, tags, `categoryId` and `madeForKids` / `selfDeclaredMadeForKids` come from `profile.publish`. Privacy is `public`.
* TikTok: same title.

Both uploads run concurrently. Only a YouTube failure fails the stage; a TikTok failure is logged. A platform whose `*_uploaded` flag is already true is skipped, so a retry never posts twice. Missing credentials and API errors come back as `Err`, never a panic.

**Location:** `AgentNode::Publisher` arm in `src/workflow.rs`, `src/uploader.rs`.

---

## Shared State

From `src/models.rs`:

```rust
pub struct VideoState {
    /// Id of the `StudioProfile` this session was started with. Sessions saved
    /// before profiles existed deserialize as the default profile.
    #[serde(default = "default_profile_id")]
    pub profile_id: String,
    pub target_path: String,
    pub target_topic: String,
    pub main_character: String,
    pub spotlight_characters: Vec<String>,
    pub supporting_characters: Vec<String>,
    pub concept_ideas: Vec<String>,
    pub draft_script: String,
    pub final_json: Option<VideoArtifact>,
    pub video_path: String,
    pub video_timelines: Vec<VideoTimeline>,
    pub session_id: String,
    pub current_node: AgentNode,
    pub meta: WorkflowMeta,

    // tracking flags
    pub youtube_uploaded: bool,
    pub tiktok_uploaded: bool,
}
```

`target_path` is `./output/{YYYYMMDD}_{profile_id}/{session_id}`. `meta` holds status, retry count, `max_retry` (3), backoff and last error.

---

## Design Principles

1. One responsibility per agent.
2. Agents should be replaceable.
3. Providers are abstracted from workflow logic.
4. Rendering and uploading should remain independent of content generation.
5. Failures should return Result<T, String> instead of panicking.
6. Pipeline is format-agnostic: everything about *what* to make lives in a StudioProfile.

---

## Future Agents

* SubtitleAgent
* MusicAgent
* ThumbnailAgent
* SEOAgent
* TranslationAgent
* QAAgent
