use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};

use crate::agent;


#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize, sqlx::Type)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase", type_name = "text")]
pub enum WorkflowStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Display, EnumString, Serialize, Deserialize, sqlx::Type)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase", type_name = "text")]
pub enum JobStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}
impl Default for JobStatus {
    fn default() -> Self {
        JobStatus::Pending
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Workflow {
    pub id: String,
    pub task: String,
    pub topic: Option<String>,
    pub status: WorkflowStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Job {
    pub id: String,
    pub workflow_id: String,
    pub parent: agent::AgentType,
    pub agent: agent::AgentType,
    pub version: String,
    pub status: JobStatus,
    pub payload: String,
    pub result: Option<String>,
    pub retry_count: i64,
    pub max_retry: i64,
    pub locked_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
/// `./output/{YYYYMMDD}/{workflow_id}`, with the local date of `created_at`.
pub fn workflow_path(created_at: i64, workflow_id: &str) -> String {
    let date = chrono::DateTime::from_timestamp(created_at, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%Y%m%d").to_string())
        .unwrap_or_else(|| "undated".to_string());

    format!("./output/{}/{}", date, workflow_id)
}
