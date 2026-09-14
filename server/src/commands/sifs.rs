//! 安全仪表功能（SIF）commands —— 业务核心
//!
//! 设计目标：
//! 1. `list_sifs` 单条 SQL 聚合，返回每条 SIF 的：
//!      - 位号清单（去重，role-tag 索引）、跨图清单
//!      - detector/final/aux 的 COUNT
//!      - 每图一个 diagram_count + 跨图数 SIF 关联图数
//! 2. link/unlink 走 sif_instrument 表（角色 + 端口 + 所在图）
//! 3. 删除 SIF 走 CASCADE，自动清 link/旁路记录/工单
//!
//! 阶段 B：sif / instrument / diagram 三方关联均按 org 作用域校验，
//! 跨组织关联一律 404（不暴露存在性）。
//!
//! 审计（M2.4）：create/update/delete/link/unlink 均写 audit_log。

use crate::commands::audit::{compute_field_diff, entity_audit_payload, snapshot, write_audit};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;

const SIF_COLS: &str = "id, project_id, code, name, description, sil_design, sil_verified,
     demand_mode, pfdavg_target, proof_interval";

/// 跨图汇总视图（一行 = 一个 SIF）
#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifSummary {
    pub id: i64,
    pub project_id: i64,
    pub project_code: String,
    pub code: String,
    pub name: String,
    pub description: String,
    pub sil_design: String,
    pub sil_verified: String,
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    pub proof_interval: i64,
    pub detector_count: i64,
    pub final_count: i64,
    pub logic_count: i64,
    pub aux_count: i64,
    pub diagram_count: i64,
    pub detectors_csv: String, // 去重后的检测位号
    pub finals_csv: String,
    pub diagrams_csv: String, // 涉及图号
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifBasic {
    pub id: i64,
    pub project_id: i64,
    pub code: String,
    pub name: String,
    pub description: String,
    pub sil_design: String,
    pub sil_verified: String,
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    pub proof_interval: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SifInput {
    pub project_id: i64,
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_sil")]
    pub sil_design: String,
    #[serde(default = "default_sil")]
    pub sil_verified: String,
    #[serde(default = "default_demand")]
    pub demand_mode: String,
    pub pfdavg_target: Option<f64>,
    #[serde(default = "default_proof")]
    pub proof_interval: i64,
}

fn default_sil() -> String {
    "NA".into()
}
fn default_demand() -> String {
    "low".into()
}
fn default_proof() -> i64 {
    12
}

/// 跨图汇总：一次 SQL 拿全部位号 + 图号，减少 N+1
pub async fn list_sifs_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    project_id: Option<i64>,
) -> AppResult<Vec<SifSummary>> {
    let base = "SELECT s.id, s.project_id, p.code AS project_code, s.code, s.name, s.description,
                      s.sil_design, s.sil_verified, s.demand_mode, s.pfdavg_target,
                      s.proof_interval,
                      COUNT(DISTINCT CASE WHEN si.role='detector' THEN si.instrument_id END) AS detector_count,
                      COUNT(DISTINCT CASE WHEN si.role='final'    THEN si.instrument_id END) AS final_count,
                      COUNT(DISTINCT CASE WHEN si.role='logic'    THEN si.instrument_id END) AS logic_count,
                      COUNT(DISTINCT CASE WHEN si.role='aux'      THEN si.instrument_id END) AS aux_count,
                      COUNT(DISTINCT si.diagram_id) AS diagram_count,
                      GROUP_CONCAT(DISTINCT CASE WHEN si.role='detector' THEN i.tag END) AS detectors_csv,
                      GROUP_CONCAT(DISTINCT CASE WHEN si.role='final'    THEN i.tag END) AS finals_csv,
                      GROUP_CONCAT(DISTINCT d.code)                            AS diagrams_csv
               FROM sif s
               JOIN project p ON p.id = s.project_id AND p.org_id = s.org_id
               LEFT JOIN sif_instrument si ON si.sif_id = s.id
               LEFT JOIN instrument i      ON i.id = si.instrument_id AND i.org_id = s.org_id
               LEFT JOIN diagram d         ON d.id = si.diagram_id AND d.org_id = s.org_id
              WHERE s.org_id = ?";

    let rows = if let Some(pid) = project_id {
        sqlx::query_as::<_, SifSummary>(&format!(
            "{base} AND s.project_id = ? GROUP BY s.id ORDER BY s.code"
        ))
        .bind(org_id)
        .bind(pid)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, SifSummary>(&format!("{base} GROUP BY s.id ORDER BY s.code"))
            .bind(org_id)
            .fetch_all(pool)
            .await?
    };
    Ok(rows)
}

pub async fn get_sif_inner(pool: &sqlx::SqlitePool, org_id: i64, id: i64) -> AppResult<SifBasic> {
    sqlx::query_as::<_, SifBasic>(&format!(
        "SELECT {SIF_COLS} FROM sif WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("sif id={id}")))
}

pub async fn create_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &SifInput,
    actor: &str,
) -> AppResult<SifBasic> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::Validation("code and name are required".into()));
    }
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    let res = sqlx::query(
        "INSERT INTO sif (org_id, project_id, code, name, description, sil_design, sil_verified,
                          demand_mode, pfdavg_target, proof_interval)
         VALUES (?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.sil_design)
    .bind(&input.sil_verified)
    .bind(&input.demand_mode)
    .bind(input.pfdavg_target)
    .bind(input.proof_interval)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "sif code '{}' already exists in project {}",
                input.code, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_sif_inner(pool, org_id, id).await?;

    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(pool, org_id, actor, "sif_create", "sif", Some(id), payload).await;

    Ok(created)
}

pub async fn update_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &SifInput,
    actor: &str,
) -> AppResult<SifBasic> {
    let before = get_sif_inner(pool, org_id, id).await?;

    // 目标项目必须同 org（防止把 SIF 挪到别组织项目）
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    let n = sqlx::query(
        "UPDATE sif SET
            project_id=?, code=?, name=?, description=?, sil_design=?, sil_verified=?,
            demand_mode=?, pfdavg_target=?, proof_interval=?, updated_at=datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(&input.description)
    .bind(&input.sil_design)
    .bind(&input.sil_verified)
    .bind(&input.demand_mode)
    .bind(input.pfdavg_target)
    .bind(input.proof_interval)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("sif id={id}")));
    }

    let after = get_sif_inner(pool, org_id, id).await?;
    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload = entity_audit_payload(
        Some(snapshot(&before)),
        Some(snapshot(&after)),
        fields_changed,
    );
    let _ = write_audit(pool, org_id, actor, "sif_update", "sif", Some(id), payload).await;

    Ok(after)
}

pub async fn delete_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    actor: &str,
) -> AppResult<u64> {
    let before = get_sif_inner(pool, org_id, id).await?;

    let n = sqlx::query("DELETE FROM sif WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("sif id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(pool, org_id, actor, "sif_delete", "sif", Some(id), payload).await;

    Ok(n)
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SifLink {
    pub id: i64,
    pub sif_id: i64,
    pub instrument_id: i64,
    pub role: String,
    pub port_index: i64,
    pub diagram_id: Option<i64>,
    pub note: String,
    pub tag: String,
    pub kind: String,
    pub service: String,
    pub sil_target: String,
}

pub async fn list_sif_links_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
) -> AppResult<Vec<SifLink>> {
    let rows = sqlx::query_as::<_, SifLink>(
        "SELECT sl.id, sl.sif_id, sl.instrument_id, sl.role, sl.port_index,
                sl.diagram_id, sl.note,
                i.tag, i.kind, i.service, i.sil_target
         FROM sif_instrument sl
         JOIN sif s ON s.id = sl.sif_id AND s.org_id = ?
         JOIN instrument i ON i.id = sl.instrument_id AND i.org_id = ?
         WHERE sl.sif_id = ?
         ORDER BY sl.role, sl.port_index, i.tag",
    )
    .bind(org_id)
    .bind(org_id)
    .bind(sif_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub async fn link_instrument_to_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    instrument_id: i64,
    role: String,
    port_index: i64,
    diagram_id: Option<i64>,
    note: String,
    actor: &str,
) -> AppResult<i64> {
    if !["detector", "final", "logic", "aux"].contains(&role.as_str()) {
        return Err(AppError::Validation(format!("role '{role}' invalid")));
    }
    // 三方归属校验（均须同 org；不存在统一 404）
    let sif_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM sif WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(sif_id)
            .fetch_optional(pool)
            .await?;
    if sif_exists.is_none() {
        return Err(AppError::NotFound(format!("sif id={sif_id}")));
    }
    let inst_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM instrument WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(instrument_id)
            .fetch_optional(pool)
            .await?;
    if inst_exists.is_none() {
        return Err(AppError::NotFound(format!("instrument id={instrument_id}")));
    }
    if let Some(did) = diagram_id {
        let diag_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM diagram WHERE org_id = ? AND id = ?")
                .bind(org_id)
                .bind(did)
                .fetch_optional(pool)
                .await?;
        if diag_exists.is_none() {
            return Err(AppError::NotFound(format!("diagram id={did}")));
        }
    }

    let res = sqlx::query(
        "INSERT INTO sif_instrument (sif_id, instrument_id, role, port_index, diagram_id, note)
         VALUES (?,?,?,?,?,?)",
    )
    .bind(sif_id)
    .bind(instrument_id)
    .bind(&role)
    .bind(port_index)
    .bind(diagram_id)
    .bind(&note)
    .execute(pool)
    .await;
    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "instrument {instrument_id} already linked to sif {sif_id} role={role}"
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    // M2.4 审计：sif_link payload 含 description 自然语言
    let instrument_tag: Option<String> =
        sqlx::query_scalar("SELECT tag FROM instrument WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(instrument_id)
            .fetch_optional(pool)
            .await?;
    let diagram_code: Option<String> = if let Some(did) = diagram_id {
        sqlx::query_scalar("SELECT code FROM diagram WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(did)
            .fetch_optional(pool)
            .await?
    } else {
        None
    };
    let instrument_tag_str = instrument_tag
        .clone()
        .unwrap_or_else(|| format!("inst#{instrument_id}"));
    let desc = format!(
        "关联 {} ({}) 作为 sif_id={} 的 {} · 端口 {} · 图 {}",
        instrument_tag_str,
        role,
        sif_id,
        role_cn(&role),
        port_index,
        diagram_code.as_deref().unwrap_or("（未挂图）"),
    );
    let mut payload = entity_audit_payload(
        None,
        Some(json!({
            "sif_id": sif_id,
            "instrument_id": instrument_id,
            "role": role,
            "port_index": port_index,
            "diagram_id": diagram_id,
            "note": note,
            "instrument_tag": instrument_tag,
            "diagram_code": diagram_code,
        })),
        vec!["link".to_string()],
    );
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("description".into(), serde_json::Value::String(desc));
    }
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "sif_link",
        "sif",
        Some(sif_id),
        payload,
    )
    .await;

    Ok(id)
}

fn role_cn(r: &str) -> String {
    match r {
        "detector" => "检测".into(),
        "final" => "最终".into(),
        "logic" => "逻辑".into(),
        "aux" => "旁路".into(),
        _ => r.to_string(),
    }
}

pub async fn unlink_instrument_from_sif_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    link_id: i64,
    actor: &str,
) -> AppResult<u64> {
    // 先查要删的 link 详情（用于审计 payload），JOIN sif 限定 org
    #[derive(sqlx::FromRow)]
    struct LinkRow {
        sif_id: i64,
        instrument_id: i64,
        role: String,
        port_index: i64,
        diagram_id: Option<i64>,
        instrument_tag: Option<String>,
        diagram_code: Option<String>,
    }
    let row: Option<LinkRow> = sqlx::query_as(
        "SELECT sl.sif_id, sl.instrument_id, sl.role, sl.port_index, sl.diagram_id,
                i.tag AS instrument_tag, d.code AS diagram_code
         FROM sif_instrument sl
         JOIN sif s ON s.id = sl.sif_id AND s.org_id = ?
         JOIN instrument i ON i.id = sl.instrument_id AND i.org_id = ?
         LEFT JOIN diagram d ON d.id = sl.diagram_id AND d.org_id = ?
         WHERE sl.id = ?",
    )
    .bind(org_id)
    .bind(org_id)
    .bind(org_id)
    .bind(link_id)
    .fetch_optional(pool)
    .await?;

    // CASCADE 删除仍以 sif 的 org 归属兜底
    let n = sqlx::query(
        "DELETE FROM sif_instrument
         WHERE id = ?
           AND EXISTS (SELECT 1 FROM sif WHERE id = sif_instrument.sif_id AND org_id = ?)",
    )
    .bind(link_id)
    .bind(org_id)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("link id={link_id}")));
    }

    if let Some(r) = row {
        let instrument_tag_str = r
            .instrument_tag
            .clone()
            .unwrap_or_else(|| format!("inst#{}", r.instrument_id));
        let diagram_code_clone = r.diagram_code.clone();
        let sif_id = r.sif_id;
        let role = r.role.clone();
        let port_index = r.port_index;
        let instrument_id = r.instrument_id;
        let diagram_id = r.diagram_id;
        let desc = format!(
            "解除 {} ({}) 与 sif_id={} 的关联（端口 {}）",
            instrument_tag_str, role, sif_id, port_index,
        );
        let mut payload = entity_audit_payload(
            Some(json!({
                "sif_id": sif_id,
                "instrument_id": instrument_id,
                "role": role,
                "port_index": port_index,
                "diagram_id": diagram_id,
                "instrument_tag": instrument_tag_str,
                "diagram_code": diagram_code_clone,
            })),
            None,
            vec!["unlink".to_string()],
        );
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("description".into(), serde_json::Value::String(desc));
        }
        let _ = write_audit(
            pool,
            org_id,
            actor,
            "sif_unlink",
            "sif",
            Some(sif_id),
            payload,
        )
        .await;
    }

    Ok(n)
}

// ===========================================================================
// M2.4 — SIF 修改历史（sif 自身 create/update/delete + 每次 link/unlink）
// ===========================================================================
pub async fn list_sif_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<crate::commands::audit::AuditEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).or(Some(200));
    crate::commands::audit::list_history_for_target_inner(pool, org_id, "sif", sif_id, lim).await
}
