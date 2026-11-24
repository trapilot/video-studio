---
name: debug-workflow
description: Inspect and fix a stuck or failed workflow in data_history.db and output/: jobs and workflows tables, requeue or fail jobs, find generated files, OAuth token problems. Use when a run is stuck, failing, retrying or the user asks what state the studio is in.
---

# Debug a workflow

State is in SQLite `./data_history.db` (gitignored); schema in `migrations/schema_01.sql`. The `sqlite3` CLI may not be installed: install it or use Python's `sqlite3` module (`python3 -I -c ...`). Confirm with the user before changing the DB or deleting files.

## Inspect
```bash
sqlite3 data_history.db "SELECT id, profile, status, topic, datetime(created_at,'unixepoch') FROM workflows ORDER BY created_at DESC LIMIT 10;"
sqlite3 data_history.db "SELECT id, workflow_id, agent, status, retry_count, max_retry, datetime(run_after,'unixepoch'), substr(result,1,200) FROM jobs ORDER BY created_at DESC LIMIT 15;"
sqlite3 data_history.db "SELECT provider, expires_at, updated_at FROM oauth_tokens;"
```
`jobs.result` holds the last error of a retried or failed job, or the payload of a completed one.

## How the queue behaves
- `claim_job` takes the oldest `pending` job with `version = APP_VERSION`, `locked_at IS NULL` and `run_after` empty or past.
- Failure: `retry_job` sets `pending`, `retry_count + 1`, `run_after` = now + 60 s doubling (cap 1 h). `max_retry` (3) is the total number of attempts: after the third failure the job and its workflow become `failed`.
- A restart requeues jobs stuck in `processing` (`recover_interrupted_jobs`).
- The scheduler creates a workflow only when no Planner job is pending/processing and fewer than `workflow_per_day` workflows exist today (local day). A stuck Planner job blocks new workflows.
- Jobs with another `version` than `APP_VERSION` are never claimed.

## Common fixes
- **Retry a failed job now:** `UPDATE jobs SET status='pending', retry_count=0, locked_at=NULL, run_after=NULL WHERE id='<id>';` then `UPDATE workflows SET status='running' WHERE id='<workflow_id>';`
- **Skip waiting for backoff:** `UPDATE jobs SET run_after=NULL WHERE id='<id>';`
- **Abandon a workflow:** `UPDATE workflows SET status='failed' WHERE id='<id>'; UPDATE jobs SET status='failed' WHERE workflow_id='<id>' AND status IN ('pending','processing');`
- **Force regeneration:** delete files in the output folder (below). Builder skips existing `shot_N/image.png` and `audio.wav`; Renderer skips `shot_N/video.mp4` and `final_video.mp4`.
- **Schema change:** edit `schema_01.sql` and delete `data_history.db` (the schema is applied only when the file is created; not deployed yet).

## Output folders
`./output/{YYYYMMDD}/{workflow_id}/` with `shot_N/{image.png,audio.wav,video.mp4}`, `video.mp4`, `subtitle.ass`, `final_video.mp4`. `*.part` files are leftovers of an interrupted write and are safe to delete.

## Common errors
- "YouTube is not authorized yet" / "Youtube token not found": the browser login never completed. Restart the studio: with no stored refresh token it starts the OAuth flow again and opens the browser (callback on 127.0.0.1:8080; redirect URI `http://localhost:8080/auth/callback`).
- Builder audio errors: Piper binary/model missing falls back to ElevenLabs, which needs `ELEVEN_LABS_KEY_1`.
- "ffmpeg/ffprobe not found in PATH": install them.
- Provider waits forever: all keys of a provider are blocked or over their daily limit (`acquire` waits); check `*_KEY_n` env vars (numbered from 1, no gaps).
- "Unknown profile" for a workflow: a profile id in `workflows.profile` no longer exists in `PROFILES`.

## Verify
`cargo check`, `cargo test`. Never `cargo run` to reproduce without the user's OK (paid APIs, public uploads).
