---
name: planner-agent
description: Work on the Planner agent (src/agents/planner.rs): topic and cast selection, topic dedup, Brief validation. Use when changing how topics or characters are chosen, or when the Planner fails or repeats topics.
---

# Planner agent

**Code**: `src/agents/planner.rs` (`PlannerAgent`, `validate_brief`). **Pool**: 10.

## Contract
- Input: payload ignored. Profile from `job_profile(state, job)`.
- Calls Gemini (JSON mode) via `ProviderManager`.
- Output (handoff to Writer): `Brief { topic, main_character, spotlight_characters, supporting_characters }`.
- Saves the topic on the production (`db.save_topic`) after the handoff.

## How it works
- System prompt from `profile.name/format/audience/tone/language`. The user prompt lists `PREVIOUS TOPICS` (`db.get_recent_topics(profile.id, 30, 30)`: same profile, last 30 days, failed productions excluded) and "Create {topic_brief}".
- With a cast (`profile.has_cast()`): spotlight characters (name, topic_domain, age, personality, relations) and relation characters (name, profession, age, role, personality, relations) are listed.
- `validate_brief`: empty topic is an error; with a cast the main character must be an enabled `spotlight` character and all names must resolve with `profile.character()` (nickname accepted, full name stored); without a cast all character fields are cleared.

## Gotchas
- Never hardcode a cast, audience or language here: use the profile.
- An invalid LLM answer returns `AgentError::Execute`/`Decode`, so the job is retried with backoff (up to 3 retries). Do not add `unwrap()`.
- Dedup is per `profile.id`. Renaming a profile id loses its topic history.
- Changing `Brief` changes the Writer's input: add fields as `#[serde(default)]` or bump `APP_VERSION`.

## Verify
`cargo check`, `cargo test` (`agents::planner::tests`). Do not `cargo run` (paid APIs, public uploads) without the user's OK.
