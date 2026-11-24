---
name: writer-agent
description: Work on the Writer stage — the LLM prompt that produces the VideoArtifact script (title, scenes, motion, transition, visual_prompt, voice_segments). Use when changing script structure, prompt rules, scene schema, or voice assignment.
---

# Writer Agent

**Code:** `AgentNode::Writer` arm in `src/workflow.rs`. Schema in `src/models.rs` (`VideoArtifact`, `Scene`, `VoiceSegment`).

## Contract
- Input: `target_topic`, the character selection, and the profile (`state.profile()`): name, format, audience, tone, visual style, duration, scene count, language, `content_rules`, and the cast from `data/characters/` (`profile.cast()`).
- Output: `state.final_json = Some(VideoArtifact)`, then moves to `AgentNode::Builder` and saves state.
- After parsing, each `voice_segment.voice_id` is filled from the speaker's `Character.voice_id` (looked up with `profile.character()`), or `CONFIG.voice.base_voice` for the narrator and unknown speakers.

## Output validation
After parsing, `normalize_artifact` (in `workflow.rs`) rejects scripts with no scenes, empty voice-over or empty visual prompts (that's an `Err`, so the scheduler retries), renumbers scenes 1..n (ids become file names), and replaces unknown motion/transition names with `None` / `Transition::DEFAULT`. Extend it when you add fields that later stages depend on.

## Schema changes
Changing `Scene`/`VideoArtifact` means updating **three** places together:
1. The struct in `src/models.rs`.
2. The `Return STRICT JSON:` example in the Writer system prompt.
3. Consumers: `build_timelines` / `ffmpeg_render` in `src/renderer.rs`.
If the change is incompatible, also bump `APP_VERSION` in `src/db.rs`.

## Motion / Transition
- The allowed values come from `Motion::ALL` / `Transition::ALL` in `src/enums.rs`. The prompt is generated from those lists, so adding an enum variant (with `Display`, `FromStr`, and the ffmpeg mapping) is enough. Don't hardcode names in the prompt.
- Update the "Motion guidelines" / "Transition guidelines" text if a new variant needs a usage hint.

## Prompt rules to preserve
- The prompt is generated from the profile. Nothing about audience, length or language is hardcoded.
- Visual style: the main character's `visual_anchor` with a cast (empty anchor falls back to `profile.visual_style`), else `profile.visual_style`.
- With a cast, speakers are the cast. `describe_characters` in `workflow.rs` gives the LLM every non-empty field of each chosen character (nickname, age, role, family role, story function, profession, personality, speaking style, catchphrase, strengths, weaknesses, core values, relations), so a new field in `Character` must be added to its list to reach the prompt. Without one, every segment uses `"Narrator"` (`profiles::NARRATOR`).
- Rules are numbered in this order: generic rules (language, duration range, scene range, at least one voice_segment per scene, still-image visual prompts, no invented motion/transition names), then cast rules, then `profile.content_rules`.
- Kid-specific rules (dialogue in every scene, kid-friendly visuals, English only) live in the `kids_edu` profile's `content_rules`, not in the Writer code.
- Output is parsed with `responseMimeType: application/json` (`build_content(..., true)`), so keep the prompt asking for JSON only.

Verify with `cargo check`.
