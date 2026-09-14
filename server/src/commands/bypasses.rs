//! 旁路授权台账 commands —— IEC 61511-1 §11.5.2 合规
//!
//! 业务规则：
//!   - reason ≥ 5 字；bypassed_by / approved_by 必填
//!   - planned_restore 必须 > now + 1 小时（防意外设过期）
//!   - 仅未恢复的旁路可被 update / restore
//!
//! 状态计算（运行时 SQL CASE，不存冗余列）：
//!   active | overdue | restored
//!
//! 阶段 B：全部查询强制 b.org_id 作用域，project/sif join 同 org 校验。
//! 审计：bypass_create / bypass_update / bypass_restore / bypass_delete。

use crate::commands::audit::write_audit;
use crate::{AppError, AppResult};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

const MIN_REASON_LEN: usize = 5;
const MIN_PLANNED_HOURS: i64 = 1;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 旁路列表项（含 SQL 计算列 status / hours_to_restore）
#[derive(Debug, Serialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BypassListItem {
    pub id: i64,
    pub project_id: i64,
    pub project_code: String,
    pub sif_id: i64,
    pub sif_code: String,
    /// SIF 当前验算 SIL（来自 sif.sil_verified）
    pub sif_sil: String,
    pub bypassed_by: String,
    pub bypassed_at: String,
    pub reason: String,
    pub planned_restore: String,
    pub restored_at: String,
    pub permit_no: String,
    pub approved_by: String,
    /// SQL CASE：active | overdue | restored
    pub status: String,
    /// hours_to_restore = julianday diff × 24；正=距到期小时，负=已逾期小时
    pub hours_to_restore: f64,
}

/// 新建旁路输入
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BypassInput {
    pub project_id: i64,
    pub sif_id: i64,
    pub bypassed_by: String,
    pub reason: String,
    /// ISO 8601（带时区）
    pub planned_restore: String,
    #[serde(default)]
    pub permit_no: String,
    pub approved_by: String,
}

/// 部分更新（只改非空字段）
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BypassUpdate {
    #[serde(default)]
    pub bypassed_by: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub planned_restore: Option<String>,
    #[serde(default)]
    pub permit_no: Option<String>,
    #[serde(default)]
    pub approved_by: Option<String>,
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

fn parse_planned(s: &str) -> AppResult<DateTime<Utc>> {
    // 兼容 "2026-09-15T10:00:00Z" 和 "2026-09-15T10:00:00+08:00"
    DateTime::parse_from_rfc3339(s.trim())
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            AppError::Validation(format!("planned_restore must be ISO 8601 (RFC 3339): {e}"))
        })
}

fn validate_input(input: &BypassInput) -> AppResult<DateTime<Utc>> {
    if input.bypassed_by.trim().is_empty() {
        return Err(AppError::Validation("bypassed_by is required".into()));
    }
    if input.approved_by.trim().is_empty() {
        return Err(AppError::Validation(
            "approved_by is required (IEC 61511-1 §11.5.2)".into(),
        ));
    }
    if input.reason.trim().chars().count() < MIN_REASON_LEN {
        return Err(AppError::Validation(format!(
            "reason must be ≥ {MIN_REASON_LEN} chars (got {})",
            input.reason.trim().chars().count()
        )));
    }
    let planned = parse_planned(&input.planned_restore)?;
    let now = Utc::now();
    if planned <= now + Duration::hours(MIN_PLANNED_HOURS) {
        return Err(AppError::Validation(format!(
            "planned_restore must be ≥ {MIN_PLANNED_HOURS}h in the future (now={}, planned={})",
            now.to_rfc3339(),
            planned.to_rfc3339()
        )));
    }
    Ok(planned)
}

// ---------------------------------------------------------------------------
// 共享 SELECT 片段（join 均限定同 org）
// ---------------------------------------------------------------------------

const SELECT_BYPASS: &str = "SELECT b.id, b.project_id, p.code AS project_code,
            b.sif_id, s.code AS sif_code, s.sil_verified AS sif_sil,
            b.bypassed_by, b.bypassed_at, b.reason, b.planned_restore,
            b.restored_at, b.permit_no, b.approved_by,
            CASE
              WHEN b.restored_at != '' THEN 'restored'
              WHEN datetime(b.planned_restore) < datetime('now') THEN 'overdue'
              ELSE 'active'
            END AS status,
            CAST((julianday(b.planned_restore) - julianday('now')) * 24 AS REAL) AS hours_to_restore
     FROM bypass_record b
     JOIN project p ON p.id = b.project_id AND p.org_id = b.org_id
     JOIN sif s    ON s.id = b.sif_id AND s.org_id = b.org_id
    WHERE b.org_id = ?";

// ---------------------------------------------------------------------------
// list_bypasses
// ---------------------------------------------------------------------------

/// status 可选：active | overdue | restored | all（默认 all）
pub async fn list_bypasses_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: Option<i64>,
    status: Option<&str>,
) -> AppResult<Vec<BypassListItem>> {
    let status_clause = match status {
        Some("active") => " AND status = 'active'",
        Some("overdue") => " AND status = 'overdue'",
        Some("restored") => " AND status = 'restored'",
        Some("all") | Some("") | None => "",
        Some(other) => {
            return Err(AppError::Validation(format!(
                "invalid status '{other}' (need active|overdue|restored|all)"
            )))
        }
    };
    let pid_clause = if project_id.is_some() {
        " AND b.project_id = ?"
    } else {
        ""
    };

    let sql = format!(
        "{SELECT_BYPASS}{status_clause}{pid_clause} ORDER BY b.restored_at DESC, b.planned_restore ASC"
    );

    let mut q = sqlx::query_as::<_, BypassListItem>(&sql).bind(org_id);
    if let Some(pid) = project_id {
        q = q.bind(pid);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// create_bypass
// ---------------------------------------------------------------------------

pub async fn create_bypass_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    input: BypassInput,
) -> AppResult<BypassListItem> {
    let _planned = validate_input(&input)?;
    if input.project_id <= 0 {
        return Err(AppError::Validation("project_id is required".into()));
    }
    if input.sif_id <= 0 {
        return Err(AppError::Validation("sif_id is required".into()));
    }

    // 校验 project 属于本 org
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::NotFound(format!(
            "project id={}",
            input.project_id
        )));
    }

    // 校验 SIF 存在、属于本 org 且属于该 project（防跨项目误关）
    let sif_row: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, project_id FROM sif WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.sif_id)
            .fetch_optional(pool)
            .await?;
    let (_, sif_pid) =
        sif_row.ok_or_else(|| AppError::NotFound(format!("sif id={}", input.sif_id)))?;
    if sif_pid != input.project_id {
        return Err(AppError::Validation(format!(
            "sif {} belongs to project {} but input project_id={}",
            input.sif_id, sif_pid, input.project_id
        )));
    }

    let res = sqlx::query(
        "INSERT INTO bypass_record
           (org_id, project_id, sif_id, bypassed_by, reason, planned_restore, permit_no, approved_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(input.sif_id)
    .bind(input.bypassed_by.trim())
    .bind(input.reason.trim())
    .bind(&input.planned_restore)
    .bind(input.permit_no.trim())
    .bind(input.approved_by.trim())
    .execute(pool)
    .await?;

    let id = res.last_insert_rowid();

    let payload = serde_json::json!({
        "sifId": input.sif_id,
        "bypassedBy": input.bypassed_by,
        "approvedBy": input.approved_by,
        "plannedRestore": input.planned_restore,
        "permitNo": input.permit_no,
        "reasonPreview": input.reason.chars().take(40).collect::<String>(),
    });
    write_audit(
        pool,
        org_id,
        actor,
        "bypass_create",
        "bypass_record",
        Some(id),
        payload,
    )
    .await?;

    get_one(pool, org_id, id).await
}

// ---------------------------------------------------------------------------
// update_bypass
// ---------------------------------------------------------------------------

pub async fn update_bypass_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
    patch: BypassUpdate,
) -> AppResult<BypassListItem> {
    // 仅未恢复的可改
    let row: Option<(String,)> =
        sqlx::query_as("SELECT restored_at FROM bypass_record WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let restored = row
        .ok_or_else(|| AppError::NotFound(format!("bypass id={id}")))?
        .0;
    if !restored.is_empty() {
        return Err(AppError::Conflict(format!(
            "bypass {id} already restored at {restored} — cannot update"
        )));
    }

    // 校验 reason 长度（如果提供）
    if let Some(r) = &patch.reason {
        if r.trim().chars().count() < MIN_REASON_LEN {
            return Err(AppError::Validation(format!(
                "reason must be ≥ {MIN_REASON_LEN} chars (got {})",
                r.trim().chars().count()
            )));
        }
    }
    // 校验 planned_restore 时间（如果提供）
    if let Some(p) = &patch.planned_restore {
        let planned = parse_planned(p)?;
        let now = Utc::now();
        if planned <= now + Duration::hours(MIN_PLANNED_HOURS) {
            return Err(AppError::Validation(format!(
                "planned_restore must be ≥ {MIN_PLANNED_HOURS}h in the future"
            )));
        }
    }
    // approved_by 非空校验（如果提供）
    if let Some(a) = &patch.approved_by {
        if a.trim().is_empty() {
            return Err(AppError::Validation("approved_by cannot be empty".into()));
        }
    }
    if let Some(b) = &patch.bypassed_by {
        if b.trim().is_empty() {
            return Err(AppError::Validation("bypassed_by cannot be empty".into()));
        }
    }

    // 动态 UPDATE（只改提供的字段）
    let mut sets: Vec<&str> = Vec::new();
    if patch.bypassed_by.is_some() {
        sets.push("bypassed_by = ?");
    }
    if patch.reason.is_some() {
        sets.push("reason = ?");
    }
    if patch.planned_restore.is_some() {
        sets.push("planned_restore = ?");
    }
    if patch.permit_no.is_some() {
        sets.push("permit_no = ?");
    }
    if patch.approved_by.is_some() {
        sets.push("approved_by = ?");
    }

    if sets.is_empty() {
        // 没改任何字段，直接返回当前行
        return get_one(pool, org_id, id).await;
    }

    let sql = format!(
        "UPDATE bypass_record SET {} WHERE org_id = ? AND id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    if let Some(b) = &patch.bypassed_by {
        q = q.bind(b.trim());
    }
    if let Some(r) = &patch.reason {
        q = q.bind(r.trim());
    }
    if let Some(p) = &patch.planned_restore {
        q = q.bind(p.trim());
    }
    if let Some(n) = &patch.permit_no {
        q = q.bind(n.trim());
    }
    if let Some(a) = &patch.approved_by {
        q = q.bind(a.trim());
    }
    q = q.bind(org_id).bind(id);
    q.execute(pool).await?;

    let payload = serde_json::json!({
        "patchedFields": sets.iter().map(|s| s.split('=').next().unwrap_or("").trim().to_string()).collect::<Vec<_>>(),
    });
    write_audit(
        pool,
        org_id,
        actor,
        "bypass_update",
        "bypass_record",
        Some(id),
        payload,
    )
    .await?;

    get_one(pool, org_id, id).await
}

async fn get_one(pool: &sqlx::SqlitePool, org_id: i64, id: i64) -> AppResult<BypassListItem> {
    sqlx::query_as::<_, BypassListItem>(&format!("{SELECT_BYPASS} AND b.id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("bypass id={id}")))
}

// ---------------------------------------------------------------------------
// restore_bypass
// ---------------------------------------------------------------------------

/// 恢复（写 restored_at = now 或指定时间）
///
/// `restored_at`: ISO 8601；None → 默认 now（UTC）
pub async fn restore_bypass_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
    restored_at: Option<&str>,
) -> AppResult<BypassListItem> {
    // 校验时间格式（如果提供）
    let restored_str: String = if let Some(s) = restored_at {
        let dt = parse_planned(s)?;
        dt.to_rfc3339()
    } else {
        Utc::now().to_rfc3339()
    };

    // 校验存在且未恢复
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT restored_at, sif_id FROM bypass_record WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(id)
            .fetch_optional(pool)
            .await?;
    let (already, sif_id) = row.ok_or_else(|| AppError::NotFound(format!("bypass id={id}")))?;
    if !already.is_empty() {
        return Err(AppError::Conflict(format!(
            "bypass {id} already restored at {already}"
        )));
    }

    // 恢复时间不能早于 planned_restore 之 24 小时前（允许现场手动改时间）
    let planned: String =
        sqlx::query_scalar("SELECT planned_restore FROM bypass_record WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(id)
            .fetch_one(pool)
            .await?;
    let restored_dt = parse_planned(&restored_str)?;
    let planned_dt = parse_planned(&planned)?;
    if restored_dt < planned_dt - Duration::hours(24) {
        return Err(AppError::Validation(format!(
            "restored_at {restored_str} is too early vs planned_restore {planned} (> 24h before)"
        )));
    }

    sqlx::query("UPDATE bypass_record SET restored_at = ? WHERE org_id = ? AND id = ?")
        .bind(&restored_str)
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?;

    let payload = serde_json::json!({
        "sifId": sif_id,
        "restoredAt": restored_str,
    });
    write_audit(
        pool,
        org_id,
        actor,
        "bypass_restore",
        "bypass_record",
        Some(id),
        payload,
    )
    .await?;

    get_one(pool, org_id, id).await
}

// ---------------------------------------------------------------------------
// delete_bypass
// ---------------------------------------------------------------------------

/// 物理删除（admin 用；正常业务应 restore）
pub async fn delete_bypass_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
) -> AppResult<u64> {
    // 先取行用于审计 payload
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT sif_id, restored_at, reason FROM bypass_record WHERE org_id = ? AND id = ?",
    )
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    let (sif_id, restored_at, reason) =
        row.ok_or_else(|| AppError::NotFound(format!("bypass id={id}")))?;

    let n = sqlx::query("DELETE FROM bypass_record WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();

    let payload = serde_json::json!({
        "sifId": sif_id,
        "restoredAt": restored_at,
        "reasonPreview": reason.chars().take(40).collect::<String>(),
    });
    write_audit(
        pool,
        org_id,
        actor,
        "bypass_delete",
        "bypass_record",
        Some(id),
        payload,
    )
    .await?;

    Ok(n)
}

// ---------------------------------------------------------------------------
// 单元测试（解析 + 校验）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_input_requires_approved_by() {
        let input = BypassInput {
            project_id: 1,
            sif_id: 1,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器 PT-201".into(),
            planned_restore: "2099-01-01T00:00:00Z".into(),
            permit_no: "WP-001".into(),
            approved_by: "".into(), // ← 空
        };
        let err = validate_input(&input).unwrap_err();
        assert!(err.to_string().contains("approved_by"));
    }

    #[test]
    fn validate_input_requires_min_reason_len() {
        let input = BypassInput {
            project_id: 1,
            sif_id: 1,
            bypassed_by: "alice".into(),
            reason: "检修".into(), // ← 2 字
            planned_restore: "2099-01-01T00:00:00Z".into(),
            permit_no: "".into(),
            approved_by: "bob".into(),
        };
        let err = validate_input(&input).unwrap_err();
        assert!(err.to_string().contains("reason"));
    }

    #[test]
    fn validate_input_rejects_past_planned_restore() {
        let input = BypassInput {
            project_id: 1,
            sif_id: 1,
            bypassed_by: "alice".into(),
            reason: "检修压力变送器 PT-201".into(),
            planned_restore: "2020-01-01T00:00:00Z".into(), // ← 太早
            permit_no: "".into(),
            approved_by: "bob".into(),
        };
        let err = validate_input(&input).unwrap_err();
        assert!(err.to_string().contains("planned_restore"));
    }

    #[test]
    fn parse_planned_accepts_offset_timezone() {
        let dt = parse_planned("2026-09-15T18:00:00+08:00").unwrap();
        // +08:00 18:00 = UTC 10:00
        let expected = DateTime::parse_from_rfc3339("2026-09-15T10:00:00Z").unwrap();
        assert_eq!(dt, expected.with_timezone(&Utc));
    }
}

// ---------------------------------------------------------------------------
// count_overdue_bypasses —— 启动时扫逾期（合规缺口不能静默）
// ---------------------------------------------------------------------------

/// 全组织 overdue 计数（含最久的一条，供横幅"逾期最久 N 小时"展示）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueBypassStatus {
    /// overdue 数量
    pub count: i64,
    /// 最久一条已逾期小时数（绝对值）；None 当 count=0
    pub oldest_overdue_hours: Option<f64>,
}

pub async fn count_overdue_bypasses_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<OverdueBypassStatus> {
    let row: (i64, Option<f64>) = sqlx::query_as(
        "SELECT
           COUNT(*) AS cnt,
           MAX(CAST((julianday('now') - julianday(planned_restore)) * 24 AS REAL)) AS oldest_hours
         FROM bypass_record
         WHERE org_id = ?
           AND restored_at = ''
           AND datetime(planned_restore) < datetime('now')",
    )
    .bind(org_id)
    .fetch_one(pool)
    .await?;

    Ok(OverdueBypassStatus {
        count: row.0,
        oldest_overdue_hours: row.1,
    })
}
