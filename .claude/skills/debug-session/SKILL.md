---
name: debug-session
description: Inspect, resume, revert, or clear a stuck or failed pipeline session in data_history.db and output/. Use when a run is stuck, retrying forever, failing at a node, or the user asks what state the pipeline is in.
---

# Debug a pipeline session

State is in SQLite at `./data_history.db` (gitignored). The schema is in `DbManager::new`, `src/db.rs`.

## Inspect
```bash
sqlite3 data_history.db "SELECT session_id, status, retry_count, max_retry, last_error, updated_at FROM workflow_state ORDER BY updated_at DESC LIMIT 5;"
sqlite3 data_history.db "SELECT json_extract(state_json,'$.profile_id'), json_extract(state_json,'$.current_node'), json_extract(state_json,'$.target_topic'), json_extract(state_json,'$.target_path') FROM workflow_state ORDER BY updated_at DESC LIMIT 1;"
sqlite3 data_history.db "SELECT profile, topic, created_at FROM topic_history ORDER BY id DESC LIMIT 20;"
```
The `sqlite3` CLI may not be installed. If so, install it or inspect the DB another way (for example a short script using the Python `sqlite3` module).
Then list the session's `target_path` to see which assets exist (`images/`, `audios/`, `videos/`, `final_video.mp4`).

## How the scheduler treats sessions (`src/scheduler.rs`)
- It resumes the most recently updated row where `version = APP_VERSION` and status isn't `done`/`cancelled`.
- Each failure triggers `state.retry(e)` with exponential backoff: `meta.backoff_ms` doubles per failure (2 s up to 30 min, `helper::next_backoff`) with jitter, and resets after each successful stage.
- When `retry_count >= max_retry` (3):
  - If more than 30 min since `updated_at`, the row is deleted and a new session starts.
  - Otherwise it waits 10 min, or with `APP_DEBUG=True` it calls `revived()` and tries again right away.
  - If `updated_at` can't be parsed, the session is cancelled.
- A session resumes with the profile in its `profile_id`, regardless of `STUDIO_PROFILE`.
- Rows that fail to deserialize are silently treated as "no session". Suspect this after any struct change and check `APP_VERSION`.

## Common fixes (confirm with the user before changing the DB or deleting files)
- **Re-run from a node:** set `current_node` and reset the retry count:
  `sqlite3 data_history.db "UPDATE workflow_state SET state_json=json_set(json_set(state_json,'$.current_node','Renderer'),'$.meta.retry_count',0), retry_count=0 WHERE session_id='<id>';"`
- **Force a re-render:** delete `videos/scene_*.mp4` and `final_video.mp4` in the session folder.
- **Abandon a session:** `UPDATE workflow_state SET status='cancelled', state_json=json_set(state_json,'$.meta.status','cancelled') WHERE session_id='<id>';`

Note: a session that shows `Builder` reuses any images and audio already on disk and only generates the missing ones.
