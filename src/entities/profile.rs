//! Studio profiles: one profile = one kind of video the studio can produce.
//!
//! A profile tells the agents *what* to make: the format, audience, tone,
//! visual style, length, extra content rules, and how to publish the result.
//! The optional recurring cast is data: one TOML file per character in
//! `data/characters/{profile id}/` (see `character.rs`). Agent code itself is
//! format-agnostic.
//!
//! The profile for new workflows is chosen with the `STUDIO_PROFILE` env var
//! (matched against `StudioProfile::id`). If it is unset, `DEFAULT_PROFILE_ID`
//! is used. Each workflow stores its profile id, so in-flight work keeps the
//! profile it started with.
//!
//! To add a new kind of video, append a `StudioProfile` to `PROFILES` (and,
//! for a cast, create its `data/characters/{id}/` folder).

use std::env;

use super::character::{self, Character};

/// Profile used when `STUDIO_PROFILE` is not set.
pub const DEFAULT_PROFILE_ID: &str = "kids_edu";

/// Speaker name used for voice-over when a profile has no cast.
pub const NARRATOR: &str = "Narrator";

#[derive(Debug)]
pub struct StudioProfile {
    /// Stable slug. Used in `STUDIO_PROFILE`, `data/characters/{id}/`, and topic dedup.
    /// Never rename it once videos have been produced, or dedup history is lost.
    pub id: &'static str,
    /// Human-readable name (shown in logs and given to the LLM as the show/channel name).
    pub name: &'static str,

    /// What kind of video this is, as a phrase completing "producing ...".
    pub format: &'static str,
    /// Who the video is for.
    pub audience: &'static str,
    /// Voice and mood of the content.
    pub tone: &'static str,
    /// What the Planner should come up with each run ("Create {topic_brief}").
    pub topic_brief: &'static str,
    /// Language for all spoken and written content.
    pub language: &'static str,

    /// Art style prepended to every image prompt, so all shots look consistent.
    pub visual_style: &'static str,

    /// Target total length in seconds (min, max).
    pub duration_secs: (u16, u16),
    /// Number of shots (min, max). Each shot is one image with 1-2 spoken sentences.
    pub shot_count: (u8, u8),

    /// Extra rules appended to the Writer prompt (safety, brand, cultural constraints).
    pub content_rules: &'static [&'static str],

    pub publish: PublishProfile,
}

#[derive(Debug)]
pub struct PublishProfile {
    pub description: &'static str,
    pub tags: &'static [&'static str],
    /// YouTube category id, e.g. "27" Education, "28" Science & Technology, "22" People & Blogs.
    pub youtube_category: &'static str,
    /// YouTube "made for kids" flag. Must be true for content aimed at children (COPPA).
    pub made_for_kids: bool,
}

impl StudioProfile {
    /// Recurring characters, loaded from `data/characters/{id}/*.toml`.
    /// Empty = narrator-only video.
    pub fn cast(&self) -> &'static [Character] {
        character::cast_for(self.id)
    }

    pub fn enabled_cast(&self) -> impl Iterator<Item = &'static Character> {
        self.cast().iter().filter(|c| c.enabled)
    }

    pub fn has_cast(&self) -> bool {
        self.enabled_cast().next().is_some()
    }

    /// Find an enabled character by full name or nickname.
    pub fn character(&self, name: &str) -> Option<&'static Character> {
        self.enabled_cast().find(|c| c.answers_to(name))
    }
}

/// Look up a profile by id.
pub fn profile_by_id(id: &str) -> Option<&'static StudioProfile> {
    PROFILES.iter().find(|p| p.id == id)
}

/// The profile selected by `STUDIO_PROFILE`, or the default.
/// Returns an error for an unknown id so a typo doesn't silently produce the wrong kind of video.
pub fn active_profile() -> Result<&'static StudioProfile, String> {
    let id = env::var("STUDIO_PROFILE").unwrap_or_default();
    let id = id.trim();
    let id = if id.is_empty() { DEFAULT_PROFILE_ID } else { id };

    profile_by_id(id).ok_or_else(|| {
        let known = PROFILES.iter().map(|p| p.id).collect::<Vec<_>>().join(", ");
        format!("Unknown STUDIO_PROFILE '{}'. Known profiles: {}", id, known)
    })
}

pub static PROFILES: &[StudioProfile] = &[
    // Character-driven educational stories for young children (the original show).
    StudioProfile {
        id: "kids_edu",
        name: "Bin's Family Adventures",
        format: "a short animated educational story for young children, told through a recurring family cast",
        audience: "children aged 4-8 and their parents",
        tone: "warm, playful, curious, encouraging",
        topic_brief: "a fresh educational topic a young child would be curious about",
        language: "English",
        visual_style: "3D Pixar style animation, bright colors, soft lighting, child-friendly",
        duration_secs: (45, 75),
        shot_count: (8, 15),
        content_rules: &[
            "Every shot must contain dialogue from the characters.",
            "Content and visual prompts must be kid-friendly.",
            "Suitable for the main character's age.",
            "No Chinese elements.",
            "Use English context only.",
        ],
        publish: PublishProfile {
            description: "An AI-generated educational short movie for kids",
            tags: &["ai movie", "english", "vietnamese", "education", "children"],
            youtube_category: "27",
            made_for_kids: true,
        },
    },

    // Narrator-only explainer for a general audience (no cast).
    StudioProfile {
        id: "explainer",
        name: "Quick Explainers",
        format: "a short narrated explainer video that makes one idea clear in under a minute",
        audience: "curious adults and teenagers",
        tone: "clear, confident, friendly, no filler",
        topic_brief: "a surprising 'how does it work' or 'why does it happen' question about science, technology, or everyday life",
        language: "English",
        visual_style: "clean cinematic digital illustration, high detail",
        duration_secs: (40, 60),
        shot_count: (8, 14),
        content_rules: &[
            "Open with a hook in the first shot.",
            "Each shot explains one step of the idea.",
            "Facts must be accurate; avoid speculation.",
        ],
        publish: PublishProfile {
            description: "A quick AI-generated explainer",
            tags: &["explainer", "science", "technology", "shorts", "learn"],
            youtube_category: "28",
            made_for_kids: false,
        },
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn profiles_are_well_formed() {
        assert!(profile_by_id(DEFAULT_PROFILE_ID).is_some(), "default profile missing");

        let mut ids = HashSet::new();
        for p in PROFILES {
            assert!(ids.insert(p.id), "duplicate profile id: {}", p.id);
            assert!(!p.id.is_empty() && p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "profile id must be a lowercase slug (used in folder names): {}", p.id);
            assert!(p.duration_secs.0 > 0 && p.duration_secs.0 <= p.duration_secs.1, "bad duration_secs in {}", p.id);
            assert!(p.shot_count.0 > 0 && p.shot_count.0 <= p.shot_count.1, "bad shot_count in {}", p.id);
        }
    }
}
