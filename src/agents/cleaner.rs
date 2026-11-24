
use std::sync::Arc;
use async_trait::async_trait;

use crate::AppState;
use crate::agent::*;
use crate::models::*;
use crate::uploader;


pub struct CleanerAgent;

#[async_trait]
impl Agent for CleanerAgent {
    async fn run(&self, state: &Arc<AppState>, job: &Job) -> Result<(), AgentError> {
        println!("📤 [Cleaner] Cleaning production...");

        let report: uploader::DistributionReport =
            serde_json::from_str(&job.payload)
            .map_err(|e| AgentError::Decode(e.to_string()))?;
        
        if let Some(err) = &report.errors.youtube {
            eprintln!("🔴 YouTube error: {}", err);
        }

        if let Some(err) = &report.errors.tiktok {
            eprintln!("🔴 TikTok error: {}", err);
        }

        if !state.config.cleanup.keep_intermediates {
            if let Some(dir) = &report.output_dir {
                // Cleanup is best effort: a failure here must not fail a published video.
                match remove_intermediates(dir, &job.production_id).await {
                    Ok(n) => println!("🧹 [Cleaner] Removed {} intermediate item(s) in {}", n, dir),
                    Err(e) => eprintln!("🔴 [Cleaner] Cleanup skipped: {}", e),
                }
            }
        }

        state.services.db
            .complete_job(&job.id, "DONE".to_string())
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        state.services.db
            .set_production_status(&job.production_id, ProductionStatus::Completed)
            .await
            .map_err(|e| AgentError::Handoff(e.to_string()))?;

        Ok(())
    }
}
/// Delete what is only needed to build the final video: the `shot_*` folders,
/// the un-subtitled `video.mp4` and leftover `.part` files. Keeps
/// `final_video.mp4` and `subtitle.ass`. Refuses any folder that is not
/// `./output/<date>/<production_id>`.
async fn remove_intermediates(dir: &str, production_id: &str) -> Result<usize, String> {
    let path = std::path::Path::new(dir);
    let under_output = path.components().any(|c| c.as_os_str() == "output");
    let is_production_dir = path.file_name().and_then(|n| n.to_str()) == Some(production_id);
    if !under_output || !is_production_dir || production_id.trim().is_empty() || dir.contains("..") {
        return Err(format!("refusing to clean unexpected folder {}", dir));
    }

    let mut removed = 0;
    let mut entries = tokio::fs::read_dir(path).await.map_err(|e| format!("{}: {}", dir, e))?;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        let name = entry.file_name().to_string_lossy().to_string();
        let p = entry.path();
        let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);

        let result = if is_dir && name.starts_with("shot_") {
            tokio::fs::remove_dir_all(&p).await
        } else if !is_dir && (name == "video.mp4" || name.ends_with(".part")) {
            tokio::fs::remove_file(&p).await
        } else {
            continue;
        };
        result.map_err(|e| format!("{}: {}", p.display(), e))?;
        removed += 1;
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removes_only_intermediates_in_the_production_folder() {
        let id = uuid::Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("vs_clean_{}", id)).join("output").join("20260101").join(&id);
        for d in ["shot_1", "shot_2"] {
            tokio::fs::create_dir_all(root.join(d)).await.unwrap();
            tokio::fs::write(root.join(d).join("image.png"), b"x").await.unwrap();
        }
        for f in ["video.mp4", "final_video.mp4", "subtitle.ass", "final_video.mp4.part"] {
            tokio::fs::write(root.join(f), b"x").await.unwrap();
        }
        let dir = root.to_string_lossy().to_string();

        assert_eq!(remove_intermediates(&dir, &id).await.unwrap(), 4);
        assert!(root.join("final_video.mp4").exists());
        assert!(root.join("subtitle.ass").exists());
        assert!(!root.join("shot_1").exists() && !root.join("video.mp4").exists());

        // Wrong production id or a folder outside output/ is refused.
        assert!(remove_intermediates(&dir, "other").await.is_err());
        assert!(remove_intermediates("/tmp", "tmp").await.is_err());

        let _ = tokio::fs::remove_dir_all(std::env::temp_dir().join(format!("vs_clean_{}", id))).await;
    }
}
