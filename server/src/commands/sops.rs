//! 检验测试规程（SOP / Test Plan）commands —— IEC 61511-1 §16.2.2
//!
//! 业务规则：
//!   - code 在组织内唯一；title 必填
//!   - SOP 可跨 SIF 复用，以 version 留痕
//!   - 删除 SOP 时 proof_test.sop_id 由 schema ON DELETE SET NULL 自动清空
//!   - 全部查询强制 org_id 作用域
//!   - 审计：proof_test_sop_create / update / delete

use crate::commands::audit::{
    compute_field_diff, entity_audit_payload, snapshot, write_audit,
};
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

const SELECT_SOP: &str = "SELECT id, org_id, code, title, version, doc_ref,
            scope, test_method, pass_criteria, notes, created_at, updated_at,
            (SELECT COUNT(*) FROM proof_test pt WHERE pt.sop_id = proof_test_sop.id) AS usage_count
         FROM proof_test_sop
        WHERE org_id = ?";

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 检验规程（含被多少条检验记录引用的聚合计数）
#[derive(Debug, Serialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProofTestSop {
    pub id: i64,
    pub org_id: i64,
    pub code: String,
    pub title: String,
    pub version: String,
    pub doc_ref: String,
    pub scope: String,
    pub test_method: String,
    pub pass_criteria: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
    pub usage_count: i64,
}

/// 新建/更新规程输入
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofTestSopInput {
    pub code: String,
    pub title: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub doc_ref: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub test_method: String,
    #[serde(default)]
    pub pass_criteria: String,
    #[serde(default)]
    pub notes: String,
}

fn default_version() -> String {
    "v1.0".into()
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

fn validate_input(input: &ProofTestSopInput) -> AppResult<()> {
    if input.code.trim().is_empty() {
        return Err(AppError::Validation("code is required".into()));
    }
    if input.title.trim().is_empty() {
        return Err(AppError::Validation("title is required".into()));
    }
    if input.version.trim().is_empty() {
        return Err(AppError::Validation("version is required".into()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub async fn list_sops_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<Vec<ProofTestSop>> {
    let rows = sqlx::query_as::<_, ProofTestSop>(&format!(
        "{SELECT_SOP} ORDER BY code"
    ))
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_sop_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
) -> AppResult<ProofTestSop> {
    sqlx::query_as::<_, ProofTestSop>(&format!("{SELECT_SOP} AND id = ?"))
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("proof_test_sop id={id}")))
}

pub async fn create_sop_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: &ProofTestSopInput,
    actor: &str,
) -> AppResult<ProofTestSop> {
    validate_input(input)?;

    let res = sqlx::query(
        "INSERT INTO proof_test_sop
            (org_id, code, title, version, doc_ref, scope, test_method, pass_criteria, notes)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(org_id)
    .bind(input.code.trim())
    .bind(input.title.trim())
    .bind(input.version.trim())
    .bind(input.doc_ref.trim())
    .bind(input.scope.trim())
    .bind(input.test_method.trim())
    .bind(input.pass_criteria.trim())
    .bind(input.notes.trim())
    .execute(pool)
    .await;
    let id = match res {
        Ok(r) => r.last_insert_rowid(),
        Err(sqlx::Error::Database(db)) if db.is_unique_violation() => {
            return Err(AppError::Conflict(format!(
                "SOP code '{}' already exists",
                input.code
            )));
        }
        Err(e) => return Err(AppError::from(e)),
    };

    let created = get_sop_inner(pool, org_id, id).await?;
    let payload = entity_audit_payload(None, Some(snapshot(&created)), vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "proof_test_sop_create",
        "proof_test_sop",
        Some(id),
        payload,
    )
    .await;

    Ok(created)
}

pub async fn update_sop_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    input: &ProofTestSopInput,
    actor: &str,
) -> AppResult<ProofTestSop> {
    validate_input(input)?;
    let before = get_sop_inner(pool, org_id, id).await?;

    let n = sqlx::query(
        "UPDATE proof_test_sop SET
            code = ?, title = ?, version = ?, doc_ref = ?, scope = ?,
            test_method = ?, pass_criteria = ?, notes = ?,
            updated_at = datetime('now')
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.code.trim())
    .bind(input.title.trim())
    .bind(input.version.trim())
    .bind(input.doc_ref.trim())
    .bind(input.scope.trim())
    .bind(input.test_method.trim())
    .bind(input.pass_criteria.trim())
    .bind(input.notes.trim())
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.is_unique_violation() => {
            AppError::Conflict(format!("SOP code '{}' already exists", input.code))
        }
        other => AppError::from(other),
    })?
    .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("proof_test_sop id={id}")));
    }

    let after = get_sop_inner(pool, org_id, id).await?;
    let fields_changed = compute_field_diff(&snapshot(&before), &snapshot(&after));
    let payload =
        entity_audit_payload(Some(snapshot(&before)), Some(snapshot(&after)), fields_changed);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "proof_test_sop_update",
        "proof_test_sop",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

/// 删除规程。schema ON DELETE SET NULL 自动解除检验记录关联，
/// 但为防止误删高频引用规程，usage_count > 0 时要求显式 force=true。
pub async fn delete_sop_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    id: i64,
    force: bool,
    actor: &str,
) -> AppResult<u64> {
    let before = get_sop_inner(pool, org_id, id).await?;
    if before.usage_count > 0 && !force {
        return Err(AppError::Validation(format!(
            "规程 {} 正被 {} 条检验记录引用；确认解除关联请用 force=true",
            before.code, before.usage_count
        )));
    }

    let n = sqlx::query("DELETE FROM proof_test_sop WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound(format!("proof_test_sop id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "proof_test_sop_delete",
        "proof_test_sop",
        Some(id),
        payload,
    )
    .await;

    Ok(n)
}
