# Characters

Recurring on-screen characters are data, not code. Each character is one TOML file. Adding, editing or retiring a character needs a restart, not a rebuild.

## Folder layout

```text
data/
└── characters/
    ├── README.md            # this file
    └── kids_edu/            # folder name = profile id (src/entities/profile.rs)
        ├── 01_bin.toml
        ├── 02_yen.toml
        ├── 03_nam.toml
        └── 04_suong.toml
```

- `data/` is the default data folder. Set `STUDIO_DATA_DIR` to use another one; the studio then reads `{STUDIO_DATA_DIR}/characters/{profile_id}/*.toml`.
- The cast is loaded once at startup (`entities::character::init`, called in `main` before the profile is resolved). Changes take effect on the next start.
- A missing `characters/` folder is fine: every profile is then narrator-only. A profile with no folder, or with no enabled characters, is also narrator-only (the Writer uses the speaker `Narrator`).
- Files that do not end in `.toml` are ignored (notes, backups).

## Add a character

1. Pick the profile and open its folder, for example `data/characters/kids_edu/`.
2. Copy an existing file to a new name with the next number prefix, for example `05_lan.toml`.
3. Set `name` (required) and fill in the fields you want. Delete or comment out the rest.
4. Give the character a `visual_template` in the same art style as the other characters. It is used in every image prompt where the character appears as an actor.
5. Optional: set `voice_id` to an ElevenLabs voice id. It is used only when speech is generated with ElevenLabs (the fallback when Piper is not installed). Piper ignores it, and without it the character uses `config.voice.base_voice`.
6. Restart the studio. A mistake (typo in a field name, duplicate name, ...) stops the program at startup with the file path and the reason.

Files load in file-name order, which is the order of the cast in the prompts, so use a numeric prefix. The file name has no other meaning: the character is identified by `name` inside the file.

## Fields

Only `name` is required. Strings default to empty, and empty strings are left out of the prompts. Unknown fields are rejected (`deny_unknown_fields`).

| Field | Required / default | What it does | Used in |
|---|---|---|---|
| `enabled` | default `true` | `false` retires the character without deleting the file. A disabled character is invisible to the pipeline. | Planner, Writer |
| `spotlight` | default `false` | `true` means the character can be chosen as the main character. `false` makes the character a supporting ("relation") character. | Planner |
| `name` | required | Full name. The LLM uses it (or `nickname`) as actor and speaker name. `"Narrator"` is reserved. | Planner, Writer, image prompts |
| `nickname` | empty | Short or alternative name. Names the LLM returns are matched against `name` **or** `nickname`; the Planner stores the full `name`. | Planner, Writer |
| `voice_id` | empty = `config.voice.base_voice` | ElevenLabs voice id for this character's lines. Not used by Piper today. | Builder (ElevenLabs mode) |
| `birth_year` | omitted = age shown as `unspecified` | Age is computed from it at run time (current year minus birth year). | Planner, Writer, `{age}` |
| `role` | empty | Narrative role (for example "Protagonist"). | Planner (supporting), Writer |
| `family_role` | empty | Relationship role inside the family or group. | Writer |
| `story_function` | empty | What this character does in a story. | Writer |
| `profession` | empty | Job or occupation. | Planner (supporting), Writer |
| `topic_domain` | empty | Topics this character fits. Helps the Planner pick a main character. | Planner (spotlight) |
| `catchphrase` | empty | A signature line. | Writer |
| `speaking_style` | empty | How the character talks. | Writer |
| `visual_template` | empty | What the character looks like. `{age}` is replaced with the current age. | Writer (as "Appearance"), image prompts |
| `strengths` | empty | Positive traits. | Writer |
| `weaknesses` | empty | Flaws that drive the story. | Writer |
| `core_values` | empty | What the character cares about. | Writer |
| `relationship_prompt` | empty | How this character relates to the others. | Planner, Writer |
| `personality_prompt` | empty | Who this character is. | Planner, Writer |

Where each field goes, in detail:

- **Planner, spotlight characters** (`spotlight = true`): `name`, `topic_domain`, age, `personality_prompt`, `relationship_prompt`.
- **Planner, other characters**: `name`, `profession`, age, `role`, `personality_prompt`, `relationship_prompt`.
- **Writer**: `Character::describe()` of every character the Planner chose (main, spotlight and supporting). It prints each non-empty field: `nickname`, age, `role`, `family_role`, `story_function`, `profession`, `personality_prompt`, `speaking_style`, `catchphrase`, `strengths`, `weaknesses`, `core_values`, `relationship_prompt`, plus `Appearance` (the `visual_template` with `{age}` filled in).
- **Image prompts** (`Shot::image_prompt`): for each actor in a shot, the actor's `visual_template` (with `{age}`) goes into the prompt after the profile's `visual_style` and the Writer's `visual_prompt`. A shot's actor whose name matches no character gets no look.
- **Speech**: with ElevenLabs, `voice_id` of the speaker (matched by name or nickname); everything else uses `config.voice.base_voice`. Piper ignores `voice_id`.

## Examples

### Minimal

```toml
name = "Mia"
spotlight = true
```

A valid cast of one. Age is `unspecified`, speech uses the base voice and no look is added to image prompts.

### Full

`data/characters/kids_edu/01_bin.toml` (shortened):

```toml
enabled = true
spotlight = true

name = "Quốc Khánh"
nickname = "Bin"
birth_year = 2020

role = "Protagonist"
family_role = "child"
story_function = "Main protagonist who initiates most adventures and learning experiences."

profession = "Student"
topic_domain = "Curiosity, Exploration, Everyday Learning"

catchphrase = "I want to try it!"
speaking_style = "Very energetic, speaks quickly, asks many 'why' questions."
visual_template = "A 3D Pixar style illustration of a cute Vietnamese boy, age {age}, short black hair, bright curious eyes, black sports cap, black sleeveless tank top"

strengths = "Curious, brave, friendly, imaginative."
weaknesses = "Impulsive, impatient, easily distracted."
core_values = "Curiosity, friendship, courage, kindness."

relationship_prompt = "Bin is the son of Nam and Diem Suong. He is the best friend of Hoang Yen."
personality_prompt = "A lively and energetic boy who loves exploring everything around him."
# voice_id = "..."   # ElevenLabs voice id; omit to use the default voice
```

## Validation

Every file is checked at startup. Any failure prints the file path and stops the program; nothing is skipped silently.

| Problem | Error |
|---|---|
| Not valid TOML, unknown field (a typo), or `name` missing | `{path}: ` plus the parse error |
| `name` is empty | `{path}: `name` is empty` |
| `name` or `nickname` is `Narrator` | `{path}: "Narrator" is reserved for narration` |
| A name or nickname equals a name or nickname of another character in the same profile | `{path}: name/nickname of "..." clashes with "..."` |
| A folder under `characters/` matches no profile id | `{folder}: folder doesn't match any profile id (known: ...)` |
| A cast has enabled characters but none is enabled with `spotlight = true` | `{folder}: cast has no enabled character with `spotlight = true` (needed for the main character)` |

Names and nicknames must all be distinct inside a profile, otherwise "who is speaking" would be ambiguous. A nickname may equal the character's own name (the built-in `Nam` does that).

The startup banner prints the loaded cast:

```text
🚀 Video Studio v0.1.0 | profile: kids_edu (Bin's Family Adventures) | cast: Quốc Khánh, Hoàng Yến, Nam, Diễm Sương
```

`cast: narrator only` means the profile has no enabled characters.

## Retire a character

Set `enabled = false` and restart. The character disappears from the prompts and from validation (a Planner answer that names it is rejected). Keep the file if you may bring the character back. If you retire the only spotlight character, enable another with `spotlight = true`, or startup fails. If you disable every character the profile becomes narrator-only.

## Give a new profile a cast

1. Add the profile in `src/entities/profile.rs` (see the `add-profile` skill and the README).
2. Create `data/characters/<profile_id>/`. The folder name must equal the profile `id`.
3. Add one `.toml` file per character, with at least one `spotlight = true`.
4. Restart.

For a narrator-only profile, create no folder.

## Tips for good prompt fields

- `personality_prompt` and `relationship_prompt` are what the LLM reads most. Write concrete behavior ("asks why before every experiment").
- Mention other characters by the names the cast uses.
- Keep `catchphrase` and `speaking_style` consistent with the personality text.
- `topic_domain` steers the Planner. Be specific enough that topics map to a character.
- Keep every `visual_template` in the art style of the profile's `visual_style`. Describe look and clothes; use `{age}` so the picture ages with the character.
- Do not rename a character that existing productions use; in-flight payloads store the full `name`.
- Leave a field out rather than filling it with filler.
