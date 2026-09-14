//! 联锁逻辑图（diagram）commands —— 持有 M0 编辑器 JSON 快照
//!
//! 在线版：纯业务函数 `*_inner(pool, org_id, ...)`，由 http/rpc.rs 分发。
//! 阶段 B：org 作用域 + save 走 version 乐观锁（CAS，409 表示他人已改）。

use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
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
// create_diagram：手工建图（项目列表页新建弹窗走这里）
// ===========================================================================

pub async fn create_diagram_inner(
    pool: &SqlitePool,
    org_id: i64,
    input: &DiagramInput,
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

    let res = sqlx::query(
        "INSERT INTO diagram (org_id, project_id, code, name, sif_id, sheet_size, revision, data)
         VALUES (?,?,?,?,?,?,?,?)",
    )
    .bind(org_id)
    .bind(input.project_id)
    .bind(&input.code)
    .bind(&input.name)
    .bind(input.sif_id)
    .bind(&input.sheet_size)
    .bind(&input.revision)
    .bind(&input.data)
    .execute(pool)
    .await;

    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "diagram code '{}' in project {} already exists",
                input.code, input.project_id
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

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
            sif_id: None,
        },
    )
    .await
}
