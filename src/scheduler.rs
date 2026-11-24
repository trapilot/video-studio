use std::env;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

use crate::db::DbManager;
use crate::models::*;
use crate::helper::*;
use crate::profiles::StudioProfile;
use crate::workflow;


pub async fn run_scheduler(
    client: reqwest::Client,
    db: Arc<DbManager>,
    profile: &'static StudioProfile,
) {
    let app_debug = "True" == env::var("APP_DEBUG").unwrap_or_else(|_| "False".to_string());

    loop {
        let mut state = match db.get_recent_state().await {
            Ok(Some(s)) => s,
            // An unfinished session keeps the profile it was started with, even if
            // STUDIO_PROFILE has changed since; the new profile applies to the next session.
            Ok(None) => VideoState::new(profile),
            Err(e) => {
                eprintln!("DB error, wait 30 minutes to retry: {}", e);
                sleep(Duration::from_mins(30)).await;
                continue;
            }
        };

        // A session whose profile was removed from PROFILES can never succeed.
        if let Err(e) = state.profile() {
            eprintln!("⛔ {} | session: {}, cancelling", e, state.session_id);
            let _ = db.save_state(state.cancelled()).await;
            continue;
        }

        if state.meta.retry_count >= state.meta.max_retry {
            println!(
                "⛔ Pending session: {} | node: {:?} | topic: {}",
                state.session_id,
                state.current_node,
                state.target_topic,
            );

            let updated_at = chrono::DateTime::parse_from_rfc3339(&state.meta.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Local))
                .ok();

            if let Some(last_time) = updated_at {
                let elapsed = chrono::Local::now() - last_time;

                // Threshold: 30 minutes
                if elapsed.num_minutes() >= 30 {
                    let _ = db.delete_state(&state.session_id).await;
                    println!("ℹ️  Removed session: {}", state.session_id);
                } else {
                    if app_debug {
                        // let _ = db.save_state(&state.revert(AgentNode::Renderer)).await;
                        let _ = db.save_state(&state.revived()).await;
                        println!("🔄 Revived session: {}", state.session_id);
                    } else {
                        println!("ℹ️  Reported, wait 10 minutes for next check");
                        sleep(Duration::from_mins(10)).await;
                    }
                }
            } else {
                let _ = db.save_state(&state.cancelled()).await;
                println!("ℹ️ Cancelled, wait 30 minutes to start new session");

                sleep(Duration::from_mins(30)).await;
            }
            continue;
        }

        let result = workflow::run_agent_workflow(
            &client,
            &db,
            &mut state,
        ).await;

        match result {
            Ok(artifact) => {
                let _ = db.save_state(&state.done()).await;
                let _ = db.save_topic(&state.profile_id, &state.target_topic).await;
 
                println!("✅ OK: {}, Updated into long-term memory", artifact.title);
                sleep(Duration::from_mins(10)).await;
            }

            Err(e) => {
                println!("🛑 ERR: {}", e);
                // Exponential backoff: the ceiling doubles per failure (reset on success).
                state.meta.backoff_ms = next_backoff(state.meta.backoff_ms.max(0) as u64) as i32;
                let delay = jitter(state.meta.backoff_ms as u64);

                if let Err(db_err) = db.save_state(state.retry(e)).await {
                    eprintln!("🔴 Failed to save state: {}", db_err);
                }

                if state.meta.retry_count < state.meta.max_retry {
                    println!("🔁 Retry in {}ms", delay);
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }
}
