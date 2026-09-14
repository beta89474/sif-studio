//! 仪表主数据 commands
//!
//! 仪表是 SIF Studio 的中心字典：检测/最终/逻辑/旁路元件共享一张表。
//! 唯一约束在 (project_id, tag) 上（M2.9），重复创建直接返回 409。
//!
//! 阶段 B：全部读写强制 org_id 作用域；项目归属校验也限定在同 org 内。
//!
//! 审计（M2.3）：
//!   - create / update / delete 三处均写 audit_log
//!   - payload 统一协议：{before, after, fieldsChanged}

use crate::commands::audit::{
    compute_field_diff, instrument_audit_payload, list_history_for_target_inner, snapshot,
    write_audit, AuditEntry,
};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

const SELECT_COLS: &str = "id, tag, service, kind, role, psv_id, manufacturer, model,
     range_min, range_max, unit, setpoint, sil_target,
     proof_interval, installed_at, notes, project_id";

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Instrument {
    pub id: i64,
    pub tag: String,
    pub service: String,
    pub kind: String,
    pub role: String,
    pub psv_id: String,
    pub manufacturer: String,
    pub model: String,
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    pub unit: String,
    pub setpoint: Option<f64>,
    pub sil_target: String,
    pub proof_interval: i64,
    pub installed_at: String,
    pub notes: String,
    /// M2.9 — 项目归属（project-scoped 仪表台账）。
    pub project_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstrumentInput {
    pub tag: String,
    #[serde(default)]
    pub service: String,
    pub kind: String,
    pub role: String,
    #[serde(default)]
    pub psv_id: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub model: String,
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    #[serde(default)]
    pub unit: String,
    pub setpoint: Option<f64>,
    #[serde(default = "default_sil")]
    pub sil_target: String,
    #[serde(default)]
    pub proof_interval: i64,
    #[serde(default)]
    pub installed_at: String,
    #[serde(default)]
    pub notes: String,
    /// M2.9 — 必填：仪表归属项目。仪表台账按项目隔离。
    pub project_id: i64,
}

fn default_sil() -> String {
    "NA".into()
}

/// 全组织仪表（跨项目视图，仍按 org 隔离）
pub async fn list_instruments_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<Vec<Instrument>> {
    sqlx::query_as::<_, Instrument>(&format!(
        "SELECT {SELECT_COLS} FROM instrument WHERE org_id = ? ORDER BY role, tag"
    ))
    .bind(org_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn list_instruments_by_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: i64,
) -> AppResult<Vec<Instrument>> {
    sqlx::query_as::<_, Instrument>(&format!(
        "SELECT {SELECT_COLS} FROM instrument
         WHERE org_id = ? AND project_id = ?
         ORDER BY role, tag"
    ))
    .bind(org_id)
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

pub async fn get_instrument_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
) -> AppResult<Instrument> {
    sqlx::query_as::<_, Instrument>(&format!(
        "SELECT {SELECT_COLS} FROM instrument WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("instrument id={id}")))
}

/// 校验目标项目属于当前组织（防 FK 模糊错误 + 越权把仪表挂到别组织项目）
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

fn validate(input: &InstrumentInput) -> AppResult<()> {
    if input.tag.trim().is_empty() {
        return Err(AppError::Validation("tag is required".into()));
    }
    if !["detector", "final", "logic", "aux"].contains(&input.role.as_str()) {
        return Err(AppError::Validation(format!(
            "role '{}' invalid",
            input.role
        )));
    }
    if input.project_id <= 0 {
        return Err(AppError::Validation("project_id is required (>=1)".into()));
    }
    Ok(())
}

pub async fn create_instrument_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    input: InstrumentInput,
) -> AppResult<Instrument> {
    validate(&input)?;
    ensure_project_in_org(pool, org_id, input.project_id).await?;

    let res = sqlx::query(
        "INSERT INTO instrument
           (org_id, tag, service, kind, role, psv_id, manufacturer, model,
            range_min, range_max, unit, setpoint, sil_target,
            proof_interval, installed_at, notes, project_id)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(&input.tag)
    .bind(&input.service)
    .bind(&input.kind)
    .bind(&input.role)
    .bind(&input.psv_id)
    .bind(&input.manufacturer)
    .bind(&input.model)
    .bind(input.range_min)
    .bind(input.range_max)
    .bind(&input.unit)
    .bind(input.setpoint)
    .bind(&input.sil_target)
    .bind(input.proof_interval)
    .bind(&input.installed_at)
    .bind(&input.notes)
    .bind(input.project_id)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "tag '{}' already exists in project {}",
                input.tag, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_instrument_inner(pool, org_id, id).await?;

    let payload = instrument_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "instrument_create",
        "instrument",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_instrument_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
    input: InstrumentInput,
) -> AppResult<Instrument> {
    validate(&input)?;

    let before = get_instrument_inner(pool, org_id, id).await?;
    ensure_project_in_org(pool, org_id, input.project_id).await?;

    let n = sqlx::query(
        "UPDATE instrument SET
            tag=?, service=?, kind=?, role=?, psv_id=?, manufacturer=?, model=?,
            range_min=?, range_max=?, unit=?, setpoint=?, sil_target=?,
            proof_interval=?, installed_at=?, notes=?, project_id=?,
            updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(&input.tag)
    .bind(&input.service)
    .bind(&input.kind)
    .bind(&input.role)
    .bind(&input.psv_id)
    .bind(&input.manufacturer)
    .bind(&input.model)
    .bind(input.range_min)
    .bind(input.range_max)
    .bind(&input.unit)
    .bind(input.setpoint)
    .bind(&input.sil_target)
    .bind(input.proof_interval)
    .bind(&input.installed_at)
    .bind(&input.notes)
    .bind(input.project_id)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("instrument id={id}")));
    }

    let after = get_instrument_inner(pool, org_id, id).await?;

    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = instrument_audit_payload(
        Some(snapshot(&before)),
        Some(snapshot(&after)),
        fields_changed,
    );
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "instrument_update",
        "instrument",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

pub async fn delete_instrument_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
) -> AppResult<u64> {
    let before = get_instrument_inner(pool, org_id, id).await?;

    let n = sqlx::query("DELETE FROM instrument WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("instrument id={id}")));
    }

    let payload = instrument_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "instrument_delete",
        "instrument",
        Some(id),
        payload,
    )
    .await;

    Ok(n)
}

pub async fn count_instruments_inner(pool: &sqlx::SqlitePool, org_id: i64) -> AppResult<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM instrument WHERE org_id = ?")
        .bind(org_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

/// M2.9 — 项目仪表台账数量（dashboard 用）
pub async fn count_instruments_by_project_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: i64,
) -> AppResult<i64> {
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM instrument WHERE org_id = ? AND project_id = ?")
            .bind(org_id)
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

// ===========================================================================
// M2.3 — 仪表修改历史（字段级 diff，IEC 61511 / ISA 84 变更追溯要求）
// M2.4：实现统一收敛到 audit::list_history_for_target_inner。
// ===========================================================================

/// M2.3 类型保留，向后兼容前端代码。
pub type InstrumentHistoryEntry = AuditEntry;

pub async fn list_instrument_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    instrument_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<InstrumentHistoryEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).or(Some(100));
    list_history_for_target_inner(pool, org_id, "instrument", instrument_id, lim).await
}
