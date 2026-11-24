use std::path::Path;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

use crate::agent;
use crate::models::*;


pub const APP_VERSION: u8 = 1;

/// Delay before retry number `retry_count + 1`: 60 s doubling, capped at 1 h.
pub fn retry_delay_secs(retry_count: i64) -> i64 {
    (60_i64 << retry_count.clamp(0, 6)).min(3600)
}

#[derive(Clone)]
pub struct DbManager {
    pub pool: SqlitePool,
}
impl DbManager {
    pub async fn new(db_url: &str) -> Result<Self, sqlx::Error> {
        let initialized = Path::new(db_url).exists();
        if !initialized {
            std::fs::File::create(db_url).ok();
        }

        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect(&format!("sqlite:{}?mode=rwc", db_url))
            .await?;

        if !initialized {
            sqlx::query(include_str!("../migrations/schema_01.sql"))
                .execute(&pool)
                .await?;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }


        // let now_date = chrono::Utc::now().date_naive();
        // let schema01_date = chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        // if now_date <= schema01_date {}

        Ok(Self { pool })
    }

    pub async fn get_oauth_token(
        &self,
        provider: &str,
    ) -> Result<Option<OAuthToken>, sqlx::Error> {
        let token = sqlx::query_as::<_, OAuthToken>(
            r#"
            SELECT
                provider,
                client_id,
                client_secret,
                access_token,
                refresh_token,
                auth_code,
                expires_at,
                updated_at
            FROM oauth_tokens
            WHERE provider = ?
            "#
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?;

        Ok(token)
    }

    pub async fn save_oauth_token(
        &self,
        token: &OAuthToken,
    ) -> Result<Option<OAuthToken>, sqlx::Error> {
        let auth_token = sqlx::query_as::<_, OAuthToken>(
            r#"
            INSERT INTO oauth_tokens (
                provider,
                client_id,
                client_secret,
                access_token,
                refresh_token,
                auth_code,
                expires_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(provider)
            DO UPDATE SET
                client_id = excluded.client_id,
                client_secret = excluded.client_secret,
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                auth_code = excluded.auth_code,
                expires_at = excluded.expires_at,
                updated_at = excluded.updated_at
            RETURNING *
            "#
        )
        .bind(&token.provider)
        .bind(&token.client_id)
        .bind(&token.client_secret)
        .bind(&token.access_token)
        .bind(&token.refresh_token)
        .bind(&token.auth_code)
        .bind(token.expires_at)
        .bind(token.updated_at)
        .fetch_optional(&self.pool)
        .await?;

        Ok(auth_token)
    }

    pub async fn create_job(
        &self,
        workflow_id: String,
        parent: agent::AgentType,
        agent: agent::AgentType,
        payload: String,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        let next_24h = now + 24 * 60 * 60;

        sqlx::query(
            r#"
            INSERT INTO jobs (
                id,
                workflow_id,
                parent,
                agent,
                version,
                status,
                payload,
                threshold_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(workflow_id)
        .bind(parent.to_string())
        .bind(agent.to_string())
        .bind(APP_VERSION)
        .bind(JobStatus::Pending.to_string())
        .bind(payload)
        .bind(next_24h)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn claim_job(&self) -> Result<Option<Job>, sqlx::Error> {
        let job = sqlx::query_as::<_, Job>(
            r#"
            UPDATE jobs
            SET status = ?, locked_at = ?
            WHERE id = (
                SELECT id
                FROM jobs
                WHERE version = ? AND status = ? AND locked_at IS NULL
                  AND (run_after IS NULL OR run_after <= ?)
                ORDER BY created_at
                LIMIT 1
            )
            RETURNING *
            "#
        )
        .bind(JobStatus::Processing.to_string())
        .bind(chrono::Utc::now().timestamp())
        .bind(APP_VERSION)
        .bind(JobStatus::Pending.to_string())
        .bind(chrono::Utc::now().timestamp())
        .fetch_optional(&self.pool)
        .await?;

        // match &job {
        //     Some(j) => println!("Job: {}", j.workflow_id),
        //     None => println!("No pending jobs found"),
        // }

        Ok(job)
    }

    pub async fn complete_job(
        &self,
        job_id: &str,
        result: String,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, result = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(JobStatus::Completed.to_string())
        .bind(result)
        .bind(chrono::Utc::now().timestamp())
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Put a failed job back in the queue, runnable again after an exponential
    /// backoff (1 min, 2 min, 4 min, ... capped at 1 h) so a failing provider
    /// isn't hammered.
    pub async fn retry_job(
        &self,
        job_id: &str,
        retry_count: i64,
        error: String,
    ) -> Result<(), sqlx::Error> {
        let now = chrono::Utc::now().timestamp();
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, result = ?, updated_at = ?, locked_at = NULL,
                retry_count = retry_count + 1, run_after = ?
            WHERE id = ?
            "#,
        )
        .bind(JobStatus::Pending.to_string())
        .bind(error)
        .bind(now)
        .bind(now + retry_delay_secs(retry_count))
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Give up on a job and mark its workflow failed.
    pub async fn fail_job(
        &self,
        job: &Job,
        error: String,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, result = ?, updated_at = ?, locked_at = NULL
            WHERE id = ?
            "#,
        )
        .bind(JobStatus::Failed.to_string())
        .bind(error)
        .bind(chrono::Utc::now().timestamp())
        .bind(&job.id)
        .execute(&self.pool)
        .await?;

        self.set_workflow_status(&job.workflow_id, WorkflowStatus::Failed).await
    }

    pub async fn set_workflow_status(
        &self,
        workflow_id: &str,
        status: WorkflowStatus,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE workflows SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status.to_string())
            .bind(chrono::Utc::now().timestamp())
            .bind(workflow_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Unlock a claimed job that couldn't be started (agent pool full), without
    /// touching its retry count.
    pub async fn release_job(&self, job_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE jobs SET status = ?, locked_at = NULL WHERE id = ?")
            .bind(JobStatus::Pending.to_string())
            .bind(job_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// At startup nothing is running yet, so any job still marked processing was
    /// interrupted by a crash or restart. Requeue it; otherwise it stays locked
    /// forever and (for the Planner) blocks every new workflow.
    pub async fn recover_interrupted_jobs(&self) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("UPDATE jobs SET status = ?, locked_at = NULL WHERE status = ?")
            .bind(JobStatus::Pending.to_string())
            .bind(JobStatus::Processing.to_string())
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn revert_job(&self, job_id: &str) -> Result<bool, sqlx::Error> {
        let result = sqlx::query(
            r#"
            UPDATE jobs
            SET status = ?, retry_count = 0, locked_at = NULL
            WHERE id = ?
            "#
        )
        .bind(JobStatus::Pending.to_string())
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        // println!("Rollback: {}", result.rows_affected() == 1);
        Ok(result.rows_affected() == 1)
    }

    pub async fn handoff_job(
        &self,
        job: &Job,
        agent: agent::AgentType,
        payload: String,
    ) -> Result<(), String> {
        if job.agent != agent {
            self.create_job(
                job.workflow_id.clone(),
                job.agent.clone(),
                agent,
                payload.clone(),
            )
            .await
            .map_err(|e| e.to_string())?;
        }

        self.complete_job(&job.id, payload)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub async fn create_workflow(
        &self,
        profile: &str,
        task: String,
    ) -> Result<String, sqlx::Error> {

        let workflow_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp();
        let next_24h = now + 24 * 60 * 60;

        sqlx::query(
            r#"
            INSERT INTO workflows (
                id,
                profile,
                task,
                status,
                threshold_at,
                created_at,
                updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&workflow_id)
        .bind(profile)
        .bind(task)
        .bind(WorkflowStatus::Running.to_string())
        .bind(next_24h)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(workflow_id)
    }

    pub async fn count_workflows_today(&self) -> Result<i64, sqlx::Error> {
        let start = self.start_of_today_ts();

        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM workflows
            WHERE created_at >= ?
            "#
        )
        .bind(start)
        .fetch_one(&self.pool)
        .await?;

        Ok(count.0)
    }

    pub async fn save_topic(
        &self,
        workflow_id: &str,
        topic: String,
    ) -> Result<Option<Workflow>, sqlx::Error> {
        let workflow = sqlx::query_as::<_, Workflow>(
            r#"
            UPDATE workflows
            SET topic = ?, updated_at = ?
            WHERE id = ?
            RETURNING *
            "#,
        )
        .bind(topic)
        .bind(chrono::Utc::now().timestamp())
        .bind(workflow_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(workflow)
    }

    /// Topics of this profile's workflows from the last `days` days, newest first.
    pub async fn get_recent_topics(&self, profile: &str, days: u8, limit: u32) -> Result<Vec<String>, sqlx::Error> {
        let since = self.start_of_recent_ts(days);
        let topics = sqlx::query_scalar::<_, String>(
            r#"
            SELECT topic
            FROM workflows
            WHERE profile = ? AND created_at >= ? AND status != ? AND topic IS NOT NULL
            ORDER BY created_at DESC
            LIMIT ?
            "#
        )
        .bind(profile)
        .bind(since)
        .bind(WorkflowStatus::Failed.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(topics)
    }
    
    /// Profile id a workflow was created with.
    pub async fn get_workflow_profile(&self, workflow_id: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>("SELECT profile FROM workflows WHERE id = ?")
            .bind(workflow_id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn agent_is_busy(
        &self,
        agent: agent::AgentType,
    ) -> Result<bool, sqlx::Error> {
        let exists: i64 = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1
                FROM jobs
                WHERE version = ? AND agent = ?
                AND status IN (?, ?)
            )
            "#,
        )
        .bind(APP_VERSION)
        .bind(agent.to_string())
        .bind(JobStatus::Pending.to_string())
        .bind(JobStatus::Processing.to_string())
        .fetch_one(&self.pool)
        .await?;

        Ok(exists > 0)
    }

    fn start_of_today_ts(&self) -> i64 {
        let now = chrono::Utc::now();
        let start = now.date_naive().and_hms_opt(0, 0, 0).unwrap();
        start.and_utc().timestamp()
    }

    fn start_of_recent_ts(&self, days: u8) -> i64 {
        let days_back = days.saturating_sub(1) as u64;
        chrono::Utc::now()
            .date_naive()
            .checked_sub_days(chrono::Days::new(days_back))
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn temp_db() -> (DbManager, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("video_studio_test_{}.db", uuid::Uuid::new_v4()));
        let db = DbManager::new(path.to_str().unwrap()).await.unwrap();
        (db, path)
    }

    #[tokio::test]
    async fn queue_topics_backoff_and_recovery() {
        let (db, path) = temp_db().await;

        // Topic history is per profile and skips failed workflows.
        let kids = db.create_workflow("kids_edu", "t".into()).await.unwrap();
        db.save_topic(&kids, "Why stars twinkle".into()).await.unwrap();
        let failed = db.create_workflow("kids_edu", "t".into()).await.unwrap();
        db.save_topic(&failed, "Broken".into()).await.unwrap();
        db.set_workflow_status(&failed, WorkflowStatus::Failed).await.unwrap();
        let other = db.create_workflow("explainer", "t".into()).await.unwrap();
        db.save_topic(&other, "How batteries work".into()).await.unwrap();

        assert_eq!(db.get_recent_topics("kids_edu", 30, 10).await.unwrap(), vec!["Why stars twinkle"]);
        assert_eq!(db.get_workflow_profile(&other).await.unwrap().as_deref(), Some("explainer"));

        // A retried job waits for its backoff before it can be claimed again.
        db.create_job(kids.clone(), agent::AgentType::Manager, agent::AgentType::Planner, "{}".into()).await.unwrap();
        let job = db.claim_job().await.unwrap().expect("job claimable");
        db.retry_job(&job.id, job.retry_count, "boom".into()).await.unwrap();
        assert!(db.claim_job().await.unwrap().is_none(), "retry must wait for its backoff");

        // A job left in `processing` by a crash is requeued at startup.
        sqlx::query("UPDATE jobs SET run_after = NULL WHERE id = ?").bind(&job.id).execute(&db.pool).await.unwrap();
        let job = db.claim_job().await.unwrap().expect("claimable after backoff");
        assert_eq!(job.retry_count, 1);
        assert_eq!(db.recover_interrupted_jobs().await.unwrap(), 1);
        let job = db.claim_job().await.unwrap().expect("claimable after recovery");

        // Releasing (agent pool full) keeps the retry count; failing marks the workflow failed.
        db.release_job(&job.id).await.unwrap();
        let job = db.claim_job().await.unwrap().unwrap();
        assert_eq!(job.retry_count, 1);
        db.fail_job(&job, "gave up".into()).await.unwrap();
        let status: String = sqlx::query_scalar("SELECT status FROM workflows WHERE id = ?")
            .bind(&kids).fetch_one(&db.pool).await.unwrap();
        assert_eq!(status, "failed");

        db.pool.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retry_delay_grows_and_caps() {
        assert_eq!(retry_delay_secs(0), 60);
        assert_eq!(retry_delay_secs(1), 120);
        assert_eq!(retry_delay_secs(10), 3600);
    }
}
