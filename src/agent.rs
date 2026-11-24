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
    Manager,
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

/// The profile of the workflow this job belongs to.
pub async fn job_profile(
    state: &Arc<AppState>,
    job: &models::Job,
) -> Result<&'static entities::StudioProfile, AgentError> {
    let id = state.services.db
        .get_workflow_profile(&job.workflow_id)
        .await
        .map_err(|e| AgentError::Execute(e.to_string()))?
        .ok_or_else(|| AgentError::Execute(format!("Workflow {} not found", job.workflow_id)))?;

    entities::profile_by_id(&id)
        .ok_or_else(|| AgentError::Execute(format!("Workflow {} uses unknown profile '{}'", job.workflow_id, id)))
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

                let db_result = if retry_count < max_retry {
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