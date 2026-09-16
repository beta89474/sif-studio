//! 报警台账 commands（ISA-18.2 / EEMUA-191）
//!
//! 报警与联锁逻辑图 / SIF 无业务关联，独立成表；但可选软关联一台仪表
//! （instrument_id），删仪表时报警保留（SET NULL）。
//!
//! 隔离规则：
//!   - 全部读写强制 org_id 作用域；
//!   - project_id 必填且必须属于本 org；
//!   - instrument_id 可空；非空时必须属于本 org 且同一项目。
//!
//! 审计：create / update / delete 写 audit_log，payload 走统一
//! {before, after, fieldsChanged} 协议。

use crate::commands::audit::{
    compute_field_diff, entity_audit_payload, list_history_for_target_inner, snapshot,
    write_audit, AuditEntry,
};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

const SELECT_COLS: &str = "id, project_id, tag, instrument_id, description, alarm_type,
     priority, category, setpoint, unit, deadband, delay_seconds, status,
     response_action, notes";

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Alarm {
    pub id: i64,
    pub project_id: i64,
    pub tag: String,
    /// 可选关联仪表（仪表删除 → SET NULL，报警记录保留）
    pub instrument_id: Option<i64>,
    pub description: String,
    pub alarm_type: String,
    pub priority: String,
    pub category: String,
    pub setpoint: Option<f64>,
    pub unit: String,
    pub deadband: Option<f64>,
    pub delay_seconds: i64,
    pub status: String,
    pub response_action: String,
    pub notes: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlarmInput {
    pub project_id: i64,
    pub tag: String,
    #[serde(default)]
    pub instrument_id: Option<i64>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_type")]
    pub alarm_type: String,
    #[serde(default = "default_priority")]
    pub priority: String,
    #[serde(default = "default_category")]
    pub category: String,
    pub setpoint: Option<f64>,
    #[serde(default)]
    pub unit: String,
    pub deadband: Option<f64>,
    #[serde(default)]
    pub delay_seconds: i64,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub response_action: String,
    #[serde(default)]
    pub notes: String,
}

fn default_type() -> String {
    "H".into()
}
fn default_priority() -> String {
    "medium".into()
}
fn default_category() -> String {
    "process".into()
}
fn default_status() -> String {
    "normal".into()
}

const ALARM_TYPES: &[&str] = &["HH", "H", "LL", "L", "DEV", "RATE", "DISC", "OTHER"];
const PRIORITIES: &[&str] = &["critical", "high", "medium", "low"];
const CATEGORIES: &[&str] = &["process", "equipment", "safety"];
const STATUSES: &[&str] = &["normal", "active", "bypassed", "shelved"];

/// 全组织报警（跨项目总览，仍按 org 隔离）
pub async fn list_alarms_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<Vec<Alarm>> {
    sqlx::query_as::<_, Alarm>(&format!(
        "SELECT {SELECT_COLS} FROM alarm_ledger WHERE org_id = ?
         ORDER BY priority, tag"
    ))
    .bind(org_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_alarms_by_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: i64,
) -> AppResult<Vec<Alarm>> {
    sqlx::query_as::<_, Alarm>(&format!(
        "SELECT {SELECT_COLS} FROM alarm_ledger
         WHERE org_id = ? AND project_id = ?
         ORDER BY priority, tag"
    ))
    .bind(org_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn get_alarm_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
) -> AppResult<Alarm> {
    sqlx::query_as::<_, Alarm>(&format!(
        "SELECT {SELECT_COLS} FROM alarm_ledger WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("alarm id={id}")))
}

/// 项目归属校验（必须同 org）
async fn ensure_project_in_org(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: i64,
) -> AppResult<()> {
    let exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(project_id)
            .fetch_optional(pool)
            .await?;
    if exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {project_id} not found in current organization"
        )));
    }
    Ok(())
}

/// 仪表归属校验（如填写）：必须同 org 且同项目，防止跨项目乱挂
async fn ensure_instrument_in_project(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    instrument_id: i64,
    project_id: i64,
) -> AppResult<()> {
    let pid: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT project_id FROM instrument WHERE org_id = ? AND id = ?",
    )
    .bind(org_id)
    .bind(instrument_id)
    .fetch_optional(pool)
    .await?;
    match pid {
        None => Err(AppError::Validation(format!(
            "instrument_id {instrument_id} not found in current organization"
        ))),
        Some((Some(p),)) if p == project_id => Ok(()),
        Some((p,)) => Err(AppError::Validation(format!(
            "instrument_id {instrument_id} belongs to project {:?}, not project {project_id}",
            p
        ))),
    }
}

fn validate(input: &AlarmInput) -> AppResult<()> {
    if input.tag.trim().is_empty() {
        return Err(AppError::Validation("tag is required".into()));
    }
    if input.project_id <= 0 {
        return Err(AppError::Validation("project_id is required (>=1)".into()));
    }
    if !ALARM_TYPES.contains(&input.alarm_type.as_str()) {
        return Err(AppError::Validation(format!(
            "alarm_type '{}' invalid",
            input.alarm_type
        )));
    }
    if !PRIORITIES.contains(&input.priority.as_str()) {
        return Err(AppError::Validation(format!(
            "priority '{}' invalid",
            input.priority
        )));
    }
    if !CATEGORIES.contains(&input.category.as_str()) {
        return Err(AppError::Validation(format!(
            "category '{}' invalid",
            input.category
        )));
    }
    if !STATUSES.contains(&input.status.as_str()) {
        return Err(AppError::Validation(format!(
            "status '{}' invalid",
            input.status
        )));
    }
    if input.delay_seconds < 0 {
        return Err(AppError::Validation("delay_seconds must be >= 0".into()));
    }
    Ok(())
}

pub async fn create_alarm_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    input: AlarmInput,
) -> AppResult<Alarm> {
    validate(&input)?;
    ensure_project_in_org(pool, org_id, input.project_id).await?;
    if let Some(iid) = input.instrument_id {
        ensure_instrument_in_project(pool, org_id, iid, input.project_id).await?;
    }

    let res = sqlx::query(
        "INSERT INTO alarm_ledger
           (org_id, project_id, tag, instrument_id, description, alarm_type, priority,
            category, setpoint, unit, deadband, delay_seconds, status, response_action, notes)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(&input.tag)
    .bind(input.instrument_id)
    .bind(&input.description)
    .bind(&input.alarm_type)
    .bind(&input.priority)
    .bind(&input.category)
    .bind(input.setpoint)
    .bind(&input.unit)
    .bind(input.deadband)
    .bind(input.delay_seconds)
    .bind(&input.status)
    .bind(&input.response_action)
    .bind(&input.notes)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "alarm tag '{}' already exists in project {}",
                input.tag, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_alarm_inner(pool, org_id, id).await?;
    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "alarm_create",
        "alarm_ledger",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_alarm_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
    input: AlarmInput,
) -> AppResult<Alarm> {
    validate(&input)?;

    let before = get_alarm_inner(pool, org_id, id).await?;
    ensure_project_in_org(pool, org_id, input.project_id).await?;
    if let Some(iid) = input.instrument_id {
        ensure_instrument_in_project(pool, org_id, iid, input.project_id).await?;
    }

    let n = sqlx::query(
        "UPDATE alarm_ledger SET
            project_id=?, tag=?, instrument_id=?, description=?, alarm_type=?, priority=?,
            category=?, setpoint=?, unit=?, deadband=?, delay_seconds=?, status=?,
            response_action=?, notes=?, updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.project_id)
    .bind(&input.tag)
    .bind(input.instrument_id)
    .bind(&input.description)
    .bind(&input.alarm_type)
    .bind(&input.priority)
    .bind(&input.category)
    .bind(input.setpoint)
    .bind(&input.unit)
    .bind(input.deadband)
    .bind(input.delay_seconds)
    .bind(&input.status)
    .bind(&input.response_action)
    .bind(&input.notes)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("alarm id={id}")));
    }

    let after = get_alarm_inner(pool, org_id, id).await?;
    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload =
        entity_audit_payload(Some(snapshot(&before)), Some(snapshot(&after)), fields_changed);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "alarm_update",
        "alarm_ledger",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

pub async fn delete_alarm_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
) -> AppResult<u64> {
    let before = get_alarm_inner(pool, org_id, id).await?;

    let n = sqlx::query("DELETE FROM alarm_ledger WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("alarm id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "alarm_delete",
        "alarm_ledger",
        Some(id),
        payload,
    )
    .await;

    Ok(n)
}

// ===========================================================================
// 修改历史（复用统一 audit_log 查询；target_table = 'alarm_ledger'）
// ===========================================================================

pub type AlarmHistoryEntry = AuditEntry;

pub async fn list_alarm_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    alarm_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<AlarmHistoryEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).or(Some(100));
    list_history_for_target_inner(pool, org_id, "alarm_ledger", alarm_id, lim).await
}
