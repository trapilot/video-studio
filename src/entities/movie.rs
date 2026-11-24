
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};


#[derive(Debug, Display, EnumString, Serialize, Deserialize, Clone)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Position {
    Left,
    Center,
    Right,
}
impl Position {
    pub fn default_value() -> Self {
        Position::Center
    }

    pub const ALL: &'static [Position] = &[
        Position::Left,
        Position::Center,
        Position::Right,
    ];
}

#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    None,
    ZoomIn,
    ZoomOut,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    KenBurns,
    DollyIn,
    DollyOut,
}
impl Motion {
    pub const ALL: &'static [Motion] = &[
        Motion::None,
        Motion::ZoomIn,
        Motion::ZoomOut,
        Motion::PanLeft,
        Motion::PanRight,
        Motion::PanUp,
        Motion::PanDown,
        Motion::KenBurns,
        Motion::DollyIn,
        Motion::DollyOut,
    ];

    /// `zoompan` filter that animates one still image for `frames` frames at the
    /// given output size and fps. The input must already be scaled/cropped to `w`x`h`.
    /// Every motion (including `None`) returns a filter, so all clips share size and
    /// fps, which `xfade` requires.
    pub fn ffmpeg_filter(&self, frames: u32, w: u32, h: u32, fps: u32) -> String {
        let center = "x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)'";
        let motion = match self {
            Motion::None => "z=1".to_string(),
            Motion::ZoomIn => format!("z='min(zoom+0.001,1.3)':{}", center),
            Motion::ZoomOut => format!("z='max(1.0,1.3-on*0.001)':{}", center),
            Motion::PanLeft => "z=1.1:x='(iw-iw/zoom)*(1-on/FRAMES)':y='ih/2-(ih/zoom/2)'".to_string(),
            Motion::PanRight => "z=1.1:x='(iw-iw/zoom)*on/FRAMES':y='ih/2-(ih/zoom/2)'".to_string(),
            Motion::PanUp => "z=1.1:x='iw/2-(iw/zoom/2)':y='(ih-ih/zoom)*(1-on/FRAMES)'".to_string(),
            Motion::PanDown => "z=1.1:x='iw/2-(iw/zoom/2)':y='(ih-ih/zoom)*on/FRAMES'".to_string(),
            Motion::KenBurns => "z='min(zoom+0.0005,1.2)':x='(iw-iw/zoom)*on/FRAMES':y='(ih-ih/zoom)*on/FRAMES'".to_string(),
            Motion::DollyIn => format!("z='min(zoom+0.001,1.5)':{}", center),
            Motion::DollyOut => format!("z='max(1.0,1.5-on*0.002)':{}", center),
        };
        // Pans move across the whole frame over the clip, whatever its length.
        let frames = frames.max(1);
        let motion = motion.replace("FRAMES", &frames.to_string());
        format!("zoompan={}:d={}:s={}x{}:fps={}", motion, frames, w, h, fps)
    }
}

#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Transition {
    Fade,
    SlideLeft,
    SlideRight,
    WipeLeft,
    WipeRight,
    CircleOpen,
    FadeBlack,
    FadeWhite,
}
impl Transition {
    /// Cross-fade between two clips of lengths `a` and `b`: `DURATION`, shortened
    /// to half the shorter clip so `xfade` never needs more frames than a clip has.
    /// The Builder (subtitle timing) and the Renderer (xfade offsets) both use it.
    pub fn fade_between(a: f64, b: f64) -> f64 {
        Self::DURATION.min(a / 2.0).min(b / 2.0)
    }

    pub const DURATION: f64 = 2.0;
    pub const ALL: &'static [Transition] = &[
        Transition::Fade,
        Transition::SlideLeft,
        Transition::SlideRight,
        Transition::WipeLeft,
        Transition::WipeRight,
        Transition::CircleOpen,
        Transition::FadeBlack,
        Transition::FadeWhite,
    ];

    pub fn ffmpeg_name(&self) -> &'static str {
        match self {
            Self::Fade => "fade",
            Self::SlideLeft => "slideleft",
            Self::SlideRight => "slideright",
            Self::WipeLeft => "wipeleft",
            Self::WipeRight => "wiperight",
            Self::CircleOpen => "circleopen",
            Self::FadeBlack => "fadeblack",
            Self::FadeWhite => "fadewhite",
        }
    }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoMetadata {
    pub title: String,
    pub video_path: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Timeline {
    pub title: String,
    pub final_path: String,
    pub video_path: String,
    pub subtitle_path: String,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct StoryContext {
    pub topic: String,
    pub main_character: String,
    pub spotlight_characters: Vec<String>,
    pub supporting_characters: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Storyboard {
    pub title: String,
    pub shots: Vec<Shot>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Clip {
    pub shot_id: u32,
    pub audio_path: String,
    pub visual_path: String,
    pub video_path: String,
    pub start_time: f64,
    pub end_time: f64,
    pub duration: f64,
    pub subtitle: String,
    pub acrossfade: f64,
    pub transition: Transition,
    pub motion: Motion,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Shot {
    pub shot_id: u32,
    pub visual_prompt: String,
    pub motion: Motion,
    pub transition: Transition,
    #[serde(default)]
    pub actors: Vec<Actor>,
    pub dialogues: Vec<Dialogue>,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub camera: Camera,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Actor {
    pub character: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub emotion: String,
    #[serde(default = "Position::default_value")]
    pub position: Position,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dialogue {
    pub character: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Camera {
    pub shot_type: String,   // or enum
    pub angle: String,
    pub movement: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Environment {
    pub location: String,
    pub time_of_day: String,
    pub weather: String,
}

impl Storyboard {
    /// Parse the Writer's JSON leniently and make it safe for the Builder and
    /// Renderer:
    /// - motion / transition / position names are matched case-insensitively and
    ///   ignoring `_`/`-`/spaces ("ZoomIn", "zoom_in" -> zoomin); unknown values fall
    ///   back to none / fade / center instead of rejecting the whole storyboard;
    /// - empty dialogue lines are dropped and shots without any dialogue removed;
    /// - shots are renumbered 1..n (ids become folder names, so they must be unique).
    pub fn from_llm_json(text: &str) -> Result<Storyboard, String> {
        let mut value: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("Storyboard is not JSON: {}", e))?;

        if let Some(shots) = value.get_mut("shots").and_then(|s| s.as_array_mut()) {
            for shot in shots {
                normalize_enum(shot, "motion", Motion::ALL.iter().map(|m| m.to_string()), "none");
                normalize_enum(shot, "transition", Transition::ALL.iter().map(|t| t.to_string()), "fade");
                if let Some(actors) = shot.get_mut("actors").and_then(|a| a.as_array_mut()) {
                    for actor in actors {
                        normalize_enum(actor, "position", Position::ALL.iter().map(|p| p.to_string()), "center");
                    }
                }
            }
        }

        let mut board: Storyboard =
            serde_json::from_value(value).map_err(|e| format!("Invalid storyboard: {}", e))?;

        board.title = board.title.trim().to_string();
        board.shots.sort_by_key(|s| s.shot_id);
        for shot in &mut board.shots {
            shot.dialogues.retain(|d| !d.text.trim().is_empty());
        }
        board.shots.retain(|s| !s.dialogues.is_empty());

        if board.shots.is_empty() {
            return Err("Storyboard has no shot with dialogue".to_string());
        }
        for (i, shot) in board.shots.iter_mut().enumerate() {
            shot.shot_id = (i + 1) as u32;
        }
        Ok(board)
    }
}

/// Replace `obj[key]` with the canonical spelling from `allowed`, or `fallback`.
fn normalize_enum(obj: &mut serde_json::Value, key: &str, allowed: impl Iterator<Item = String>, fallback: &str) {
    let squash = |s: &str| s.to_lowercase().replace(['_', '-', ' '], "");
    let given = obj.get(key).and_then(|v| v.as_str()).map(squash).unwrap_or_default();
    let canonical = allowed
        .into_iter()
        .find(|a| squash(a) == given)
        .unwrap_or_else(|| fallback.to_string());
    if let Some(map) = obj.as_object_mut() {
        map.insert(key.to_string(), serde_json::Value::String(canonical));
    }
}

/// Cloudflare flux-1-schnell rejects prompts over 2048 characters.
pub const MAX_IMAGE_PROMPT_CHARS: usize = 2000;

impl Shot {
    /// Full image prompt: the profile's art style, the Writer's description of
    /// the shot, each actor's look from their character file, the setting and
    /// the camera. Every shot shares the style and character looks, so the
    /// images stay consistent.
    pub fn image_prompt(&self, profile: &super::StudioProfile) -> String {
        let actors = self.actors
            .iter()
            .map(|a| {
                let look = profile
                    .character(&a.character)
                    .map(|c| c.visual_anchor())
                    .filter(|l| !l.trim().is_empty())
                    .map(|l| format!(" ({})", l))
                    .unwrap_or_default();
                format!("{}{}, {}, {}, {} of frame", a.character, look, a.action, a.emotion, a.position)
            })
            .collect::<Vec<_>>();

        let mut parts = vec![
            profile.visual_style.to_string(),
            self.visual_prompt.trim().to_string(),
        ];
        if !actors.is_empty() {
            parts.push(format!("Characters: {}", actors.join("; ")));
        }
        parts.push(format!(
            "Setting: {}, {}, {} weather",
            self.environment.location, self.environment.time_of_day, self.environment.weather
        ));
        parts.push(format!("Camera: {} shot, {} angle", self.camera.shot_type, self.camera.angle));
        parts.push("vertical 9:16 composition, subject centered, no text, no captions, no logos".to_string());

        let prompt = parts
            .into_iter()
            .filter(|p| !p.trim().is_empty())
            .collect::<Vec<_>>()
            .join(". ");
        prompt.chars().take(MAX_IMAGE_PROMPT_CHARS).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::profile_by_id;

    fn shot(actors: Vec<Actor>) -> Shot {
        Shot {
            shot_id: 1,
            visual_prompt: "Bin looks up at the night sky".into(),
            motion: Motion::ZoomIn,
            transition: Transition::Fade,
            actors,
            dialogues: vec![Dialogue { character: "Bin".into(), text: "Wow!".into() }],
            environment: Environment { location: "backyard".into(), time_of_day: "night".into(), weather: "clear".into() },
            camera: Camera { shot_type: "wide".into(), angle: "low".into(), movement: "static".into() },
        }
    }

    #[test]
    fn storyboard_parsing_is_lenient_and_safe() {
        let json = r#"{
            "title": " Stars ",
            "shots": [
                {"shot_id": 7, "motion": "ZoomIn", "transition": "Slide_Left", "visual_prompt": "b",
                 "actors": [{"character": "Bin", "position": "LEFT"}],
                 "dialogues": [{"character": "Bin", "text": "Two"}]},
                {"shot_id": 7, "motion": "wobble", "transition": "??", "visual_prompt": "a",
                 "dialogues": [{"character": "Bin", "text": " "}, {"character": "Bin", "text": "One"}]},
                {"shot_id": 9, "motion": "none", "transition": "fade", "visual_prompt": "c",
                 "dialogues": [{"character": "Bin", "text": "  "}]}
            ]
        }"#;
        let b = Storyboard::from_llm_json(json).unwrap();
        assert_eq!(b.title, "Stars");
        assert_eq!(b.shots.len(), 2, "shot without dialogue dropped");
        assert_eq!(b.shots.iter().map(|s| s.shot_id).collect::<Vec<_>>(), vec![1, 2]);
        assert!(matches!(b.shots[0].motion, Motion::ZoomIn));
        assert!(matches!(b.shots[0].transition, Transition::SlideLeft));
        assert!(matches!(b.shots[0].actors[0].position, Position::Left));
        assert!(matches!(b.shots[1].motion, Motion::None));
        assert!(matches!(b.shots[1].transition, Transition::Fade));
        assert_eq!(b.shots[1].dialogues.len(), 1);

        assert!(Storyboard::from_llm_json(r#"{"title":"x","shots":[]}"#).is_err());
        assert!(Storyboard::from_llm_json("not json").is_err());
    }

    #[test]
    fn image_prompt_uses_style_shot_and_character_look() {
        crate::entities::character::init_for_tests();
        let kids = profile_by_id("kids_edu").unwrap();
        let bin = &kids.cast()[0];

        let p = shot(vec![Actor { character: "Bin".into(), action: "pointing".into(), emotion: "amazed".into(), position: Position::Center }])
            .image_prompt(kids);

        assert!(p.starts_with(kids.visual_style));
        assert!(p.contains("Bin looks up at the night sky"));
        assert!(p.contains(&bin.visual_anchor()[..40]), "actor look from the character file");
        assert!(p.contains("backyard"));
        assert!(!p.contains("{age}"));
        assert!(p.chars().count() <= MAX_IMAGE_PROMPT_CHARS);
    }
}
