//! 项目 commands —— 一组相关联锁逻辑图属于同一项目
//!
//! 阶段 B：全部读写强制 org_id 作用域（org 来自登录会话，不接受前端传入）。

use crate::commands::audit::{compute_field_diff, entity_audit_payload, snapshot, write_audit};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub client: String,
    pub location: String,
    pub phase: String,
    pub started_at: String,
    pub finished_at: String,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub location: String,
    #[serde(default = "default_phase")]
    pub phase: String,
    #[serde(default)]
    pub finished_at: String,
    #[serde(default)]
    pub notes: String,
}

fn default_phase() -> String {
    "design".into()
}

const COLS: &str = "id, code, name, client, location, phase, started_at, finished_at, notes";

pub async fn list_projects_inner(pool: &sqlx::SqlitePool, org_id: i64) -> AppResult<Vec<Project>> {
    let rows = sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLS} FROM project WHERE org_id = ? ORDER BY started_at DESC, id DESC"
    ))
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 取项目；org 不匹配一律 404（不暴露"存在但无权"）
pub async fn get_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
) -> AppResult<Project> {
    sqlx::query_as::<_, Project>(&format!(
        "SELECT {COLS} FROM project WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("project id={id}")))
}

pub async fn create_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &ProjectInput,
    actor: &str,
) -> AppResult<Project> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::Validation("code and name are required".into()));
    }
    let res = sqlx::query(
        "INSERT INTO project (org_id, code, name, client, location, phase, finished_at, notes)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.client)
    .bind(&input.location)
    .bind(&input.phase)
    .bind(&input.finished_at)
    .bind(&input.notes)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "project code '{}' exists",
                input.code
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_project_inner(pool, org_id, id).await?;

    // M2.4 审计：project_create
    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "project_create",
        "project",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &ProjectInput,
    actor: &str,
) -> AppResult<Project> {
    let before = get_project_inner(pool, org_id, id).await?;

    let n = sqlx::query(
        "UPDATE project SET
            code=?, name=?, client=?, location=?, phase=?, finished_at=?, notes=?,
            updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.client)
    .bind(&input.location)
    .bind(&input.phase)
    .bind(&input.finished_at)
    .bind(&input.notes)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("project id={id}")));
    }

    let after = get_project_inner(pool, org_id, id).await?;
    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = entity_audit_payload(
        Some(snapshot(&before)),
        Some(snapshot(&after)),
        fields_changed,
    );
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "project_update",
        "project",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

pub async fn delete_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    actor: &str,
) -> AppResult<u64> {
    let before = get_project_inner(pool, org_id, id).await?;

    let n = sqlx::query("DELETE FROM project WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("project id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "project_delete",
        "project",
        Some(id),
        payload,
    )
    .await;

    Ok(n)
}

/// 若当前组织还没有任何项目，建一个默认项目便于上手；否则返回最早的项目。
pub async fn ensure_default_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<Project> {
    let exist: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM project WHERE org_id = ? ORDER BY id LIMIT 1")
            .bind(org_id)
            .fetch_optional(pool)
            .await?;
    if let Some((id,)) = exist {
        return get_project_inner(pool, org_id, id).await;
    }
    create_project_inner(
        pool,
        org_id,
        &ProjectInput {
            code: "PRJ-DEFAULT".into(),
            name: "默认项目".into(),
            client: String::new(),
            location: String::new(),
            phase: "design".into(),
            finished_at: String::new(),
            notes: "首次启动自动创建，可在项目列表重命名或删除。".into(),
        },
        "system",
    )
    .await
}

// ===========================================================================
// M2.4 — Project 修改历史，复用 audit::list_history_for_target_inner
// ===========================================================================
pub async fn list_project_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<crate::commands::audit::AuditEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).or(Some(200));
    crate::commands::audit::list_history_for_target_inner(pool, org_id, "project", project_id, lim)
        .await
}
