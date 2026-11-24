---
name: writer-agent
description: Work on the Writer stage — the LLM prompt that produces the VideoArtifact script (title, scenes, motion, transition, visual_prompt, voice_segments). Use when changing script structure, prompt rules, scene schema, or voice assignment.
---

# Writer Agent

**Code:** `AgentNode::Writer` arm in `src/workflow.rs`. Schema in `src/models.rs` (`VideoArtifact`, `Scene`, `VoiceSegment`).

## Contract
- Input: `target_topic`, `main_character`, `spotlight_characters`, `supporting_characters`, character prompts from `EDU_CHARACTERS`.
- Output: `state.final_json = Some(VideoArtifact)`, then moves to `AgentNode::Builder` and saves state.
- After parsing, each `voice_segment.voice_id` is filled from the speaker's `Character.voice_id`.

## Schema changes
Changing `Scene`/`VideoArtifact` means updating **three** places together:
1. The struct in `src/models.rs` (new fields: `Option<T>` or `#[serde(default)]`).
2. The `Return STRICT JSON:` example in the Writer system prompt.
3. Consumers: `build_timelines` / `ffmpeg_render` in `src/renderer.rs`.
If the change is incompatible, also bump `APP_VERSION` in `src/db.rs`.

## Motion / Transition
- The allowed values come from `Motion::ALL` / `Transition::ALL` in `src/enums.rs`. The prompt is generated from those lists, so adding an enum variant (with `Display`, `FromStr`, and the ffmpeg mapping) is enough. Don't hardcode names in the prompt.
- Update the "Motion guidelines" / "Transition guidelines" text if a new variant needs a usage hint.

## Prompt rules to preserve
- Language comes from `CONFIG.movie.language`.
- 45–60s total, 3–5 scenes, dialogue in every scene, only the provided characters, kid-friendly visuals.
- Output is parsed with `responseMimeType: application/json` (`build_content(..., true)`), so keep the prompt asking for JSON only.

Verify with `cargo check`.
