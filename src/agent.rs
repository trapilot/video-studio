use std::sync::Arc;
use tokio::sync::Semaphore;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};
use thiserror::Error;

use crate::AppState;
use crate::models;
use crate::entities;


#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize, Eq, PartialEq, Hash, sqlx::Type)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase", type_name = "text")]
pub enum AgentType {
    Producer,
    Planner,
    Writer,
    Builder,
    Renderer,
    Publisher,
    Cleaner,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Handoff job error: {0}")]
    Handoff(String),

    #[error("Execute error: {0}")]
    Execute(String),

    #[error("Encode error: {0}")]
    Encode(String),

    #[error("Decode error: {0}")]
    Decode(String),
}

/// `max_retry` is the total number of attempts. The run that just failed was
/// attempt `retry_count + 1`; retry only if another attempt is allowed.
pub fn should_retry(retry_count: i64, max_retry: i64) -> bool {
    retry_count + 1 < max_retry
}

/// The profile of the production this job belongs to.
pub async fn job_profile(
    state: &Arc<AppState>,
    job: &models::Job,
) -> Result<&'static entities::StudioProfile, AgentError> {
    let id = state.services.db
        .get_production_profile(&job.production_id)
        .await
        .map_err(|e| AgentError::Execute(e.to_string()))?
        .ok_or_else(|| AgentError::Execute(format!("Production {} not found", job.production_id)))?;

    entities::profile_by_id(&id)
        .ok_or_else(|| AgentError::Execute(format!("Production {} uses unknown profile '{}'", job.production_id, id)))
}

#[async_trait]
pub trait Agent: Send + Sync {
    async fn run(
        &self,
        state: &Arc<AppState>,
        job: &models::Job,
    ) -> Result<(), AgentError>;
}

pub struct AgentPool {
    semaphore: Arc<tokio::sync::Semaphore>,
}

impl AgentPool {
    pub fn new(max_concurrency: usize) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(max_concurrency)),
        }
    }

    pub fn try_spawn(
        &self,
        agent: Arc<dyn Agent>,
        state: Arc<AppState>,
        job: models::Job,
    ) -> bool {
        let Ok(permit) = self.semaphore.clone().try_acquire_owned() else {
            return false;
        };

        let job_clone = job.clone();
        let state_clone = state.clone();

        tokio::spawn(async move {
            let _permit = permit;

            if let Err(e) = agent.run(&state_clone, &job).await {
                eprintln!("❌ Agent {:?} failed job {} --> {}", job_clone.agent, job_clone.id, e);
                
                let retry_count = job_clone.retry_count;
                let max_retry = job_clone.max_retry;

                let db_result = if should_retry(retry_count, max_retry) {
                    state_clone.services.db.retry_job(&job_clone.id, retry_count, e.to_string()).await
                } else {
                    state_clone.services.db.fail_job(&job_clone, e.to_string()).await
                };

                match db_result {
                    Ok(_) => {},
                    Err(e) => eprintln!("❌ Failed job error {} --> {}", job_clone.id, e),
                }
            }
        });

        true
    }
}
#[cfg(test)]
mod tests {
    use super::should_retry;

    #[test]
    fn max_retry_is_total_attempts() {
        // max_retry = 3: attempts 1 and 2 are retried, attempt 3 fails the job.
        assert!(should_retry(0, 3));
        assert!(should_retry(1, 3));
        assert!(!should_retry(2, 3));
        assert!(!should_retry(0, 1));
    }
}
