//! Characters are data, not code: one TOML file per character under
//! `{data_dir}/characters/{profile_id}/`. They are loaded once at startup.
//!
//! To add a character, drop a new `.toml` file into the profile's folder and
//! restart. No rebuild needed. Files load in file-name order, so a numeric
//! prefix (`01_bin.toml`) controls the order the cast is listed in prompts.
//! See `data/characters/README.md` for the field reference.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use chrono::{Datelike, Utc};
use serde::Deserialize;

use super::profile::{PROFILES, NARRATOR};

/// Default data folder, overridable with `STUDIO_DATA_DIR`.
pub const DEFAULT_DATA_DIR: &str = "./data";

/// A recurring on-screen character. Only `name` is required in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Character {
    /// `false` retires the character without deleting the file.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// `true` = can be chosen as the main character.
    #[serde(default)]
    pub spotlight: bool,

    /// Unique within the profile. The LLM uses it (or `nickname`) as the actor/speaker name.
    pub name: String,
    #[serde(default)]
    pub nickname: String,
    /// TTS voice for this character. Empty = the default voice.
    #[serde(default)]
    pub voice_id: String,
    /// Used to compute the age shown to the LLM and `{age}` in `visual_template`.
    #[serde(default)]
    pub birth_year: Option<i32>,

    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub family_role: String,
    #[serde(default)]
    pub story_function: String,

    #[serde(default)]
    pub profession: String,
    #[serde(default)]
    pub topic_domain: String,

    #[serde(default)]
    pub catchphrase: String,
    #[serde(default)]
    pub speaking_style: String,
    /// What the character looks like, for image prompts. `{age}` is replaced with the current age.
    #[serde(default)]
    pub visual_template: String,

    #[serde(default)]
    pub strengths: String,
    #[serde(default)]
    pub weaknesses: String,
    #[serde(default)]
    pub core_values: String,

    /// How this character relates to the others. Read by the LLM.
    #[serde(default)]
    pub relationship_prompt: String,
    /// Who this character is. Read by the LLM.
    #[serde(default)]
    pub personality_prompt: String,
}

fn yes() -> bool {
    true
}

impl Character {
    pub fn age(&self) -> Option<i32> {
        self.birth_year.map(|y| Utc::now().year() - y)
    }

    /// Age for prompts: a number, or "unspecified" when `birth_year` is omitted.
    pub fn age_label(&self) -> String {
        self.age()
            .map(|a| a.to_string())
            .unwrap_or_else(|| "unspecified".to_string())
    }

    /// `visual_template` with `{age}` filled in.
    pub fn visual_anchor(&self) -> String {
        self.visual_template.replace("{age}", &self.age_label())
    }

    /// True if `name` refers to this character by full name or nickname.
    pub fn answers_to(&self, name: &str) -> bool {
        let name = name.trim();
        self.name == name || (!self.nickname.is_empty() && self.nickname == name)
    }

    pub fn voice(&self) -> Option<&str> {
        let v = self.voice_id.trim();
        (!v.is_empty()).then_some(v)
    }

    /// Prompt block with every non-empty field, so whatever is written in the
    /// file actually reaches the LLM.
    pub fn describe(&self) -> String {
        let age = self.age_label();
        let anchor = self.visual_anchor();
        let fields = [
            ("Nickname", self.nickname.as_str()),
            ("Age", age.as_str()),
            ("Role", self.role.as_str()),
            ("Family role", self.family_role.as_str()),
            ("Story function", self.story_function.as_str()),
            ("Profession", self.profession.as_str()),
            ("Personality", self.personality_prompt.as_str()),
            ("Speaking style", self.speaking_style.as_str()),
            ("Catchphrase", self.catchphrase.as_str()),
            ("Strengths", self.strengths.as_str()),
            ("Weaknesses", self.weaknesses.as_str()),
            ("Core values", self.core_values.as_str()),
            ("Relations", self.relationship_prompt.as_str()),
            ("Appearance", anchor.as_str()),
        ];
        let body = fields
            .iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| format!("  {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");
        format!("- {}\n{}", self.name, body)
    }
}

static CASTS: OnceLock<HashMap<String, Vec<Character>>> = OnceLock::new();

/// Load every profile's cast from `{data_dir}/characters`. Call once at startup.
/// A missing `characters` folder is fine (all profiles narrator-only); a bad file is an error.
pub fn init(data_dir: &str) -> Result<(), String> {
    let casts = load_all(Path::new(data_dir))?;
    CASTS.set(casts).map_err(|_| "Characters already loaded".to_string())
}

/// The loaded cast of a profile, in file-name order. Empty if it has none.
pub fn cast_for(profile_id: &str) -> &'static [Character] {
    CASTS
        .get()
        .and_then(|c| c.get(profile_id))
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn load_all(data_dir: &Path) -> Result<HashMap<String, Vec<Character>>, String> {
    let root = data_dir.join("characters");
    let mut casts = HashMap::new();

    if !root.is_dir() {
        return Ok(casts);
    }

    for entry in read_sorted(&root)? {
        if !entry.is_dir() {
            continue;
        }
        let profile_id = entry.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        // A folder that matches no profile is almost always a typo; fail loudly.
        if !PROFILES.iter().any(|p| p.id == profile_id) {
            let known = PROFILES.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
            return Err(format!(
                "{}: folder doesn't match any profile id (known: {})",
                entry.display(), known
            ));
        }
        casts.insert(profile_id, load_cast(&entry)?);
    }

    Ok(casts)
}

fn load_cast(dir: &Path) -> Result<Vec<Character>, String> {
    let mut cast: Vec<Character> = Vec::new();

    for path in read_sorted(dir)? {
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        let mut character: Character = toml::from_str(&text)
            .map_err(|e| format!("{}: {}", path.display(), e))?;
        character.name = character.name.trim().to_string();
        character.nickname = character.nickname.trim().to_string();

        if character.name.is_empty() {
            return Err(format!("{}: `name` is empty", path.display()));
        }
        if character.answers_to(NARRATOR) {
            return Err(format!("{}: \"{}\" is reserved for narration", path.display(), NARRATOR));
        }
        // Names and nicknames must all be distinct, or "who is speaking" becomes ambiguous.
        if let Some(other) = cast.iter().find(|c| {
            c.answers_to(&character.name) || (!character.nickname.is_empty() && c.answers_to(&character.nickname))
        }) {
            return Err(format!(
                "{}: name/nickname of \"{}\" clashes with \"{}\"",
                path.display(), character.name, other.name
            ));
        }
        cast.push(character);
    }

    if cast.iter().any(|c| c.enabled) && !cast.iter().any(|c| c.enabled && c.spotlight) {
        return Err(format!(
            "{}: cast has no enabled character with `spotlight = true` (needed for the main character)",
            dir.display()
        ));
    }

    Ok(cast)
}

fn read_sorted(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {}", dir.display(), e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
pub(crate) fn init_for_tests() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/data");
    if CASTS.get().is_none() {
        let _ = CASTS.set(load_all(Path::new(dir)).expect("data/characters must load"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_data_loads() {
        init_for_tests();
        let kids = cast_for("kids_edu");
        assert_eq!(kids.len(), 4);
        assert!(kids[0].answers_to("Bin") && kids[0].answers_to("Quốc Khánh"));
        assert!(kids[0].spotlight);
        assert!(!kids[0].visual_anchor().contains("{age}"));
        assert!(cast_for("explainer").is_empty());
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn minimal_file_only_needs_a_name() {
        let root = std::env::temp_dir().join(format!("vs_chars_{}", uuid::Uuid::new_v4()));
        write(&root.join("characters/kids_edu"), "a.toml", "name = \"Mia\"\nspotlight = true\n");
        let casts = load_all(&root).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let mia = &casts["kids_edu"][0];
        assert!(mia.enabled);
        assert_eq!(mia.age_label(), "unspecified");
        assert_eq!(mia.voice(), None);
    }

    #[test]
    fn rejects_bad_data() {
        let cases: &[(&str, &[(&str, &str)])] = &[
            ("unknown_profile", &[("a.toml", "name = \"A\"\nspotlight = true")]),
            ("kids_edu", &[("a.toml", "name = \"A\"\nspotlight = true"), ("b.toml", "name = \"A\"")]),
            ("kids_edu", &[("a.toml", "name = \"A\"\nspotlight = true"), ("b.toml", "name = \"B\"\nnickname = \"A\"")]),
            ("kids_edu", &[("a.toml", "name = \"Narrator\"\nspotlight = true")]),
            ("kids_edu", &[("a.toml", "name = \"A\"")]),                       // no spotlight
            ("kids_edu", &[("a.toml", "name = \"A\"\nspotlite = true")]),      // typo'd field
        ];
        for (folder, files) in cases {
            let root = std::env::temp_dir().join(format!("vs_chars_{}", uuid::Uuid::new_v4()));
            for (name, body) in *files {
                write(&root.join("characters").join(folder), name, body);
            }
            let result = load_all(&root);
            let _ = std::fs::remove_dir_all(&root);
            assert!(result.is_err(), "expected error for {:?}", files);
        }
    }
}
