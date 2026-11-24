use uuid::Uuid;
use serde::{Deserialize, Serialize};

use crate::profiles::*;
use crate::helper::*;
use crate::enums::*;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WorkflowMeta {
    pub status: String, // running | cooldown | failed | done | cancelled
    pub retry_count: i32,
    pub max_retry: i32,
    pub backoff_ms: i32,
    pub last_error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct VideoState {
    /// Id of the `StudioProfile` this session was started with.
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

    // 👇 add tracking flags
    pub youtube_uploaded: bool,
    pub tiktok_uploaded: bool,
}
impl VideoState {
    /// The profile this session was started with. Errors if it was removed from `PROFILES`.
    pub fn profile(&self) -> Result<&'static StudioProfile, String> {
        profile_by_id(&self.profile_id)
            .ok_or_else(|| format!("Session uses unknown profile '{}'", self.profile_id))
    }

    pub fn new(profile: &StudioProfile) -> Self {
        let session_id = Uuid::new_v4().to_string();
        let target_path = format!(
            "./output/{}_{}/{}",
            now_ymd(),
            profile.id,
            session_id,
        );
        
        Self {
            session_id,
            profile_id: profile.id.to_string(),
            target_path,
            target_topic: "".into(),
            current_node: AgentNode::Planner,
            main_character: "".into(),
            spotlight_characters: vec![],
            supporting_characters: vec![],
            concept_ideas: vec![],
            draft_script: "".into(),
            final_json: None,
            video_timelines: vec![],
            video_path: "".into(),
            youtube_uploaded: false,
            tiktok_uploaded: false,
            meta: WorkflowMeta {
                status: "running".into(),
                retry_count: 0,
                max_retry: 3,
                backoff_ms: BASE_BACKOFF_MS as i32,
                last_error: None,
                updated_at: now_rfc(),
            },
        }
    }

    pub fn done(&mut self) -> &Self {
        self.meta.updated_at = now_rfc();
        self.meta.last_error = None;
        self.meta.status = "done".into();
        self
    }

    pub fn revert(&mut self, node: AgentNode) -> &Self {
        self.current_node = node;
        self.reset()
    }

    pub fn retry(&mut self, e: String) -> &Self {
        self.meta.updated_at = now_rfc();
        self.meta.last_error = Some(e.clone());
        self.meta.retry_count = self.meta.retry_count + 1;
        self.meta.status = "cooldown".into();
        self
    }

    pub fn revived(&mut self) -> &Self {
        self.meta.updated_at = now_rfc();
        self.meta.retry_count = self.meta.max_retry - 1;
        self.meta.status = "revived".into();
        self
    }

    pub fn reset(&mut self) -> &Self {
        self.meta.updated_at = now_rfc();
        self.meta.retry_count = 0;
        self.meta.backoff_ms = BASE_BACKOFF_MS as i32;
        self.meta.last_error = None;
        self.meta.status = "running".into();
        self
    }

    pub fn cancelled(&mut self) -> &Self {
        self.meta.updated_at = now_rfc();
        self.meta.status = "cancelled".into();
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoTimeline {
    pub scene_id: u8,

    pub audio_path: String,
    pub visual_path: String,

    pub start_time: f64,
    pub end_time: f64,
    pub duration: f64,

    pub transition: String,
    pub motion: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoArtifact {
    pub title: String,
    pub scenes: Vec<Scene>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VoiceSegment {
    pub text: String,
    pub speaker: String,
    pub voice_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Scene {
    pub scene_id: u8,
    /// The Writer's estimate in seconds. Informational only: the real scene
    /// length is measured from the generated audio.
    #[serde(default)]
    pub duration: f64,
    pub motion: String,
    pub transition: String,
    pub visual_prompt: String,
    pub voice_segments: Vec<VoiceSegment>
}
