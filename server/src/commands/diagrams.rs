//! 联锁逻辑图（diagram）commands —— 持有 M0 编辑器 JSON 快照
//!
//! 在线版：纯业务函数 `*_inner(pool, org_id, ...)`，由 http/rpc.rs 分发。
//! 阶段 B：org 作用域 + save 走 version 乐观锁（CAS，409 表示他人已改）。

use crate::commands::audit::write_audit_best_effort;
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{FromRow, SqlitePool};

const DIAGRAM_COLS: &str = "id, project_id, code, name, sif_id, sheet_size, revision, data,
     updated_at, created_at, version";

#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Diagram {
    pub id: i64,
    pub project_id: i64,
    pub code: String,
    pub name: String,
    pub sif_id: Option<i64>,
    pub sheet_size: String,
    pub revision: String,
    pub data: String, // M0 编辑器 JSON 字符串（"…"）
    pub updated_at: String,
    pub created_at: String,
    /// 阶段 B：乐观锁版本号，每次保存 +1
    pub version: i64,
}

#[derive(Debug, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DiagramSummary {
    pub id: i64,
    pub project_id: i64,
    pub code: String,
    pub name: String,
    pub sif_id: Option<i64>,
    pub sheet_size: String,
    pub revision: String,
    pub updated_at: String,
    pub version: i64,
    pub sif_code: Option<String>, // 来自 sif 表，避免前端再 join
    pub sif_sil_design: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DiagramInput {
    pub project_id: i64,
    pub code: String,
    pub name: String,
    #[serde(default = "default_sheet")]
    pub sheet_size: String,
    #[serde(default = "default_rev")]
    pub revision: String,
    #[serde(default = "default_data")]
    pub data: String,
    pub sif_id: Option<i64>,
}

/// 保存结果：返回新版本号，前端替换本地 version 后才允许再次保存
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDiagramResult {
    pub id: i64,
    pub version: i64,
}

fn default_sheet() -> String {
    "A1".into()
}
fn default_rev() -> String {
    "A0".into()
}
fn default_data() -> String {
    "{}".into()
}

// ===========================================================================
// list_diagrams：LEFT JOIN sif 给前端直展示当前图挂的 SIF 编号 + SIL
// ===========================================================================

pub async fn list_diagrams_inner(
    pool: &SqlitePool,
    org_id: i64,
    project_id: Option<i64>,
) -> AppResult<Vec<DiagramSummary>> {
    let rows = if let Some(pid) = project_id {
        sqlx::query_as::<_, DiagramSummary>(
            "SELECT d.id, d.project_id, d.code, d.name, d.sif_id, d.sheet_size, d.revision,
                    d.updated_at, d.version, s.code AS sif_code, s.sil_design AS sif_sil_design
             FROM diagram d
             LEFT JOIN sif s ON d.sif_id = s.id AND s.org_id = d.org_id
             WHERE d.org_id = ? AND d.project_id = ?
             ORDER BY d.code",
        )
        .bind(org_id)
        .bind(pid)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, DiagramSummary>(
            "SELECT d.id, d.project_id, d.code, d.name, d.sif_id, d.sheet_size, d.revision,
                    d.updated_at, d.version, s.code AS sif_code, s.sil_design AS sif_sil_design
             FROM diagram d
             LEFT JOIN sif s ON d.sif_id = s.id AND s.org_id = d.org_id
             WHERE d.org_id = ?
             ORDER BY d.updated_at DESC",
        )
        .bind(org_id)
        .fetch_all(pool)
        .await?
    };
    Ok(rows)
}

// ===========================================================================
// get_diagram：取单张图完整 M0 JSON
// ===========================================================================

pub async fn get_diagram_inner(pool: &SqlitePool, org_id: i64, id: i64) -> AppResult<Diagram> {
    sqlx::query_as::<_, Diagram>(&format!(
        "SELECT {DIAGRAM_COLS} FROM diagram WHERE org_id = ? AND id = ?"
    ))
    .bind(org_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound(format!("diagram id={id}")))
}

// ===========================================================================
// create_diagram：手工建图（项目详情页新建弹窗走这里）
//
// 业务规则（一张联锁图 = 一个 SIF）：
//   - 若调用方显式传了 sif_id，则校验后直接挂接；
//   - 若 sif_id 为 None，自动在同项目下创建一个 SIF（编号顺延 SIF-xxx，
//     名称沿用图名），再把图挂到该 SIF。SIF 与图在同一事务内落库，
//     保证「有图必有 SIF」，SIF 汇总/总览即时可见。
// ===========================================================================

/// 在项目内生成下一个不冲突的 SIF 编号（SIF-001 / SIF-002 …）。
/// 取现有 SIF-数字 编号的最大序号 +1，再做一次存在性兜底防止手工编号撞号。
async fn next_sif_code(
    pool: &SqlitePool,
    org_id: i64,
    project_id: i64,
) -> AppResult<String> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT code FROM sif WHERE org_id = ? AND project_id = ?")
            .bind(org_id)
            .bind(project_id)
            .fetch_all(pool)
            .await?;
    let existing: std::collections::HashSet<&str> =
        rows.iter().map(|(c,)| c.as_str()).collect();
    let max_seq = rows
        .iter()
        .filter_map(|(c,)| c.strip_prefix("SIF-"))
        .filter_map(|n| n.parse::<i64>().ok())
        .max()
        .unwrap_or(0);
    let mut seq = max_seq + 1;
    loop {
        let candidate = format!("SIF-{seq:03}");
        if !existing.contains(candidate.as_str()) {
            return Ok(candidate);
        }
        seq += 1;
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn create_diagram_inner(
    pool: &SqlitePool,
    org_id: i64,
    input: &DiagramInput,
    actor: &str,
) -> AppResult<Diagram> {
    if input.code.trim().is_empty() || input.name.trim().is_empty() {
        return Err(AppError::Validation("code and name are required".into()));
    }
    // 项目 / SIF 归属校验（均须同 org）
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
    if let Some(sid) = input.sif_id {
        let sif_exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM sif WHERE org_id = ? AND id = ?")
                .bind(org_id)
                .bind(sid)
                .fetch_optional(pool)
                .await?;
        if sif_exists.is_none() {
            return Err(AppError::Validation(format!(
                "sif_id {sid} not found in current organization"
            )));
        }
    }

    // 未指定 SIF → 自动建一个，与图同事务落库。
    // 注意：SIF 编号必须在 begin() 之前用 pool 生成——SQLite 连接池上限为 1，
    // 事务持有唯一连接期间再用 pool 查询会自死锁（PoolTimedOut）。
    let auto_sif_code = match input.sif_id {
        Some(_) => None,
        None => Some(next_sif_code(pool, org_id, input.project_id).await?),
    };

    let mut tx = pool.begin().await?;

    let sif_id = match input.sif_id {
        Some(sid) => sid,
        None => {
            let sif_code = auto_sif_code.as_deref().expect("已在事务前生成");
            let description = format!("随联锁图 {} 自动生成", input.code);
            let r = sqlx::query(
                "INSERT INTO sif (org_id, project_id, code, name, description,
                                  sil_design, sil_verified, demand_mode, pfdavg_target, proof_interval)
                 VALUES (?,?,?,?,?, 'NA','NA','low', NULL, 12)",
            )
            .bind(org_id)
            .bind(input.project_id)
            .bind(sif_code)
            .bind(&input.name)
            .bind(&description)
            .execute(&mut *tx)
            .await;
            match r {
                Ok(rec) => rec.last_insert_rowid(),
                // 并发烧号撞号（理论上 next_sif_code 已兜底）
                Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
                    return Err(AppError::Conflict(format!(
                        "auto sif code '{sif_code}' collided in project {}; retry",
                        input.project_id
                    )));
                }
                Err(e) => return Err(AppError::from(e)),
            }
        }
    };

    let res = sqlx::query(
        "INSERT INTO diagram (org_id, project_id, code, name, sif_id, sheet_size, revision, data)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(sif_id)
    .bind(&input.sheet_size)
    .bind(&input.revision)
    .bind(&input.data)
    .execute(&mut *tx)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            // 事务随析构回滚，自动建的 SIF 一并撤销，不留孤儿
            return Err(AppError::Conflict(format!(
                "diagram code '{}' in project {} already exists",
                input.code, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    tx.commit().await?;

    // 自动建的 SIF 写审计（best-effort，不影响主流程）
    if let Some(code) = auto_sif_code {
        let payload = json!({
            "before": null,
            "after": {
                "id": sif_id,
                "projectId": input.project_id,
                "code": code,
                "name": input.name,
                "autoFromDiagram": input.code,
            },
            "fieldsChanged": ["*"],
        });
        write_audit_best_effort(
            pool, org_id, actor, "sif_create", "sif", Some(sif_id), payload,
        )
        .await;
    }

    get_diagram_inner(pool, org_id, id).await
}

// ===========================================================================
// save_diagram_data：M0 编辑器回写的唯一入口（version CAS 乐观锁）
// ===========================================================================

pub async fn save_diagram_data_inner(
    pool: &SqlitePool,
    org_id: i64,
    id: i64,
    expected_version: i64,
    data: &str,
) -> AppResult<SaveDiagramResult> {
    // 先确认图存在且属于本 org —— 不存在给 404（不与 409 混淆）
    let current: (i64,) = sqlx::query_as("SELECT version FROM diagram WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("diagram id={id}")))?;

    if current.0 != expected_version {
        return Err(AppError::Conflict(format!(
            "diagram id={id} version conflict: expected v{expected_version}, current v{}",
            current.0
        )));
    }

    let n = sqlx::query(
        "UPDATE diagram
            SET data = ?, version = version + 1, updated_at = datetime('now')
          WHERE org_id = ? AND id = ? AND version = ?",
    )
    .bind(data)
    .bind(org_id)
    .bind(id)
    .bind(expected_version)
    .execute(pool)
    .await?
    .rows_affected();
    if n == 0 {
        // 并发竞争：读和写之间被他人抢先
        return Err(AppError::Conflict(format!(
            "diagram id={id} was modified by someone else; reload and retry"
        )));
    }
    Ok(SaveDiagramResult {
        id,
        version: expected_version + 1,
    })
}

// ===========================================================================
// ensure_default_diagram：进入项目若无任何 diagram，自动建一张占位图
// ===========================================================================

pub async fn ensure_default_diagram_inner(
    pool: &SqlitePool,
    org_id: i64,
    project_id: i64,
    actor: &str,
) -> AppResult<Diagram> {
    let exist: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM diagram WHERE org_id = ? AND project_id = ? ORDER BY id LIMIT 1",
    )
    .bind(org_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    if let Some((id,)) = exist {
        return get_diagram_inner(pool, org_id, id).await;
    }
    create_diagram_inner(
        pool,
        org_id,
        &DiagramInput {
            project_id,
            code: "DGM-001".into(),
            name: "SIF 联锁逻辑图".into(),
            sheet_size: "A1".into(),
            revision: "A0".into(),
            /* ★ 这里不要自造 schema。
            M0 编辑器的持久化契约是 {v,blocks,rowCount,sifCols,sif,silv,doc,…}，
            其中 blocks 必须是数组，这是「是不是可用快照」的唯一判据。
            留 "{}"（明确的"无可用快照"）→ 编辑器保留内置示例图当起点，
            并在顶栏提示「尚未保存」，用户保存后才真正落库。 */
            data: "{}".into(),
            // None → create_diagram 自动建配套 SIF（有图必有 SIF）
            sif_id: None,
        },
        actor,
    )
    .await
}
