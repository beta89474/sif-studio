//! 检验测试 commands —— IEC 61511-1 §16.3 合规
//!
//! 业务规则：
//!   - tested_at 必填且 ≤ today（不许录未来测试日期）
//!   - result ∈ {pass, fail, conditional}
//!   - tested_by 必填（合规要求记录测试人）
//!   - next_due_at = date(tested_at, '+N months')，N 取 SIF.proof_interval
//!   - SIF 删除 → proof_test CASCADE（schema 已建）
//!
//! 状态计算（运行时 SQL CASE，不存冗余列）：
//!   overdue（最新一条且 next_due_at < today）| current
//!
//! 阶段 B：全部查询强制 org_id 作用域，sif join 同 org 校验。
//! 审计：proof_test_create / proof_test_update / proof_test_delete。

use crate::commands::audit::{
    compute_field_diff, entity_audit_payload, list_history_for_target_inner, snapshot, write_audit,
};
use crate::{AppError, AppResult};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// 常量 & SQL 片段
// ---------------------------------------------------------------------------

const SELECT_PT: &str = "SELECT pt.id, pt.sif_id, s.code AS sif_code,
            p.code AS project_code, pt.tested_at, pt.result, pt.tested_by,
            pt.next_due_at, pt.findings, pt.notes, pt.created_at,
            pt.sop_id, sop.code AS sop_code, sop.title AS sop_title, sop.version AS sop_version,
            CASE
              WHEN date(pt.next_due_at) < date('now')
               AND pt.id = (SELECT MAX(id) FROM proof_test WHERE sif_id = pt.sif_id)
              THEN 'overdue' ELSE 'current'
            END AS status
     FROM proof_test pt
     JOIN sif s     ON s.id = pt.sif_id     AND s.org_id = pt.org_id
     JOIN project p ON p.id = s.project_id  AND p.org_id = pt.org_id
     LEFT JOIN proof_test_sop sop ON sop.id = pt.sop_id AND sop.org_id = pt.org_id
    WHERE pt.org_id = ?";

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 检验测试列表项（含 SQL 计算列 status）
#[derive(Debug, Serialize, FromRow, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProofTest {
    pub id: i64,
    pub sif_id: i64,
    pub sif_code: String,
    pub project_code: String,
    pub tested_at: String,
    pub result: String,
    pub tested_by: String,
    pub next_due_at: String,
    pub findings: String,
    pub notes: String,
    pub created_at: String,
    /// 关联的检验规程 SOP（§16.2.2）；无关联为 None
    pub sop_id: Option<i64>,
    pub sop_code: Option<String>,
    pub sop_title: Option<String>,
    pub sop_version: Option<String>,
    /// SQL CASE：overdue | current
    pub status: String,
    /// §16.2.2 合规标志：是否按成文规程执行（sop_id 非空）
    #[sqlx(default)]
    pub sop_linked: bool,
}

/// 新建/更新检验测试输入
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofTestInput {
    pub sif_id: i64,
    pub tested_at: String,
    #[serde(default = "default_result")]
    pub result: String,
    #[serde(default)]
    pub tested_by: String,
    #[serde(default)]
    pub findings: String,
    #[serde(default)]
    pub notes: String,
    /// 关联的检验规程 id（须同 org；None = 无规程）
    #[serde(default)]
    pub sop_id: Option<i64>,
}

fn default_result() -> String {
    "pass".into()
}

impl Default for ProofTestInput {
    fn default() -> Self {
        Self {
            sif_id: 0,
            tested_at: String::new(),
            result: default_result(),
            tested_by: String::new(),
            findings: String::new(),
            notes: String::new(),
            sop_id: None,
        }
    }
}

/// 逾期计数（供横幅）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverdueProofTestStatus {
    /// 逾期 SIF 数量（每个 SIF 只计最新一条）
    pub count: i64,
    /// 最久逾期天数（绝对值）；None 当 count=0
    pub oldest_overdue_days: Option<f64>,
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

fn validate_input(input: &ProofTestInput) -> AppResult<()> {
    if input.sif_id <= 0 {
        return Err(AppError::Validation("sif_id is required".into()));
    }
    if input.tested_at.trim().is_empty() {
        return Err(AppError::Validation("tested_at is required".into()));
    }
    // 日期格式校验
    let tested = NaiveDate::parse_from_str(input.tested_at.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::Validation(format!("tested_at 格式应为 YYYY-MM-DD: {}", input.tested_at)))?;
    // 不许录未来测试日期
    let today = chrono::Local::now().date_naive();
    if tested > today {
        return Err(AppError::Validation(format!(
            "tested_at 不能是未来日期: {} > {}",
            input.tested_at,
            today.format("%Y-%m-%d")
        )));
    }
    // result 枚举
    match input.result.as_str() {
        "pass" | "fail" | "conditional" => {}
        other => {
            return Err(AppError::Validation(format!(
                "result='{other}' 无效（pass | fail | conditional）"
            )))
        }
    }
    // tested_by 必填
    if input.tested_by.trim().is_empty() {
        return Err(AppError::Validation("tested_by is required".into()));
    }
    Ok(())
}

/// 校验 SOP 引用存在且同 org（§16.2.2）
async fn validate_sop_ref(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sop_id: Option<i64>,
) -> AppResult<()> {
    if let Some(sid) = sop_id {
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM proof_test_sop WHERE org_id = ? AND id = ?")
                .bind(org_id)
                .bind(sid)
                .fetch_optional(pool)
                .await?;
        if exists.is_none() {
            return Err(AppError::Validation(format!(
                "sop_id {sid} not found in current organization"
            )));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// list_proof_tests
// ---------------------------------------------------------------------------

pub async fn list_proof_tests_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    sif_id: Option<i64>,
) -> AppResult<Vec<ProofTest>> {
    let (sql, has_sif) = match sif_id {
        Some(_sid) => (format!("{SELECT_PT} AND pt.sif_id = ? ORDER BY pt.tested_at DESC"), true),
        None => (format!("{SELECT_PT} ORDER BY pt.tested_at DESC"), false),
    };
    let mut q = sqlx::query_as::<_, ProofTest>(&sql).bind(org_id);
    if has_sif {
        q = q.bind(sif_id.unwrap());
    }
    let mut rows = q.fetch_all(pool).await?;
    for r in rows.iter_mut() {
        r.sop_linked = r.sop_id.is_some();
    }
    Ok(rows)
}

/// 取单条（含 status + SOP 快照）
async fn get_one(pool: &sqlx::SqlitePool, org_id: i64, id: i64) -> AppResult<ProofTest> {
    let sql = format!("{SELECT_PT} AND pt.id = ?");
    let mut row = sqlx::query_as::<_, ProofTest>(&sql)
        .bind(org_id)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("proof_test id={id}")))?;
    row.sop_linked = row.sop_id.is_some();
    Ok(row)
}

// ---------------------------------------------------------------------------
// create_proof_test
// ---------------------------------------------------------------------------

pub async fn create_proof_test_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    input: ProofTestInput,
) -> AppResult<ProofTest> {
    validate_input(&input)?;

    // 校验 SIF 存在且同 org，同时取 proof_interval
    let sif_row: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, proof_interval FROM sif WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.sif_id)
            .fetch_optional(pool)
            .await?;
    let (_, proof_interval) = sif_row
        .ok_or_else(|| AppError::NotFound(format!("sif id={}", input.sif_id)))?;
    if proof_interval <= 0 {
        return Err(AppError::Validation(format!(
            "sif {} proof_interval={} 无效，必须 > 0",
            input.sif_id, proof_interval
        )));
    }
    validate_sop_ref(pool, org_id, input.sop_id).await?;

    // INSERT（不开事务，与 bypass 同模式，规避 PoolTimedOut）
    let res = sqlx::query(
        "INSERT INTO proof_test (org_id, sif_id, tested_at, result, tested_by,
                                 next_due_at, findings, notes, sop_id)
         VALUES (?, ?, ?, ?, ?, date(?, '+' || ? || ' months'), ?, ?, ?)",
    )
    .bind(org_id)
    .bind(input.sif_id)
    .bind(input.tested_at.trim())
    .bind(input.result.trim())
    .bind(input.tested_by.trim())
    .bind(input.tested_at.trim()) // date() 的第一个参数
    .bind(proof_interval) // +N months
    .bind(input.findings.trim())
    .bind(input.notes.trim())
    .bind(input.sop_id)
    .execute(pool)
    .await?;

    let id = res.last_insert_rowid();

    let payload = json!({
        "sifId": input.sif_id,
        "testedAt": input.tested_at,
        "result": input.result,
        "testedBy": input.tested_by,
        "sopId": input.sop_id,
        "nextDueAt": format!("auto-computed from +{} months", proof_interval),
    });
    write_audit(
        pool,
        org_id,
        actor,
        "proof_test_create",
        "proof_test",
        Some(id),
        payload,
    )
    .await?;

    get_one(pool, org_id, id).await
}

// ---------------------------------------------------------------------------
// update_proof_test
// ---------------------------------------------------------------------------

pub async fn update_proof_test_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
    input: ProofTestInput,
) -> AppResult<ProofTest> {
    validate_input(&input)?;

    let before = get_one(pool, org_id, id).await?;

    // 校验 SIF 存在且同 org，取 proof_interval
    let sif_row: Option<(i64, i64)> =
        sqlx::query_as("SELECT id, proof_interval FROM sif WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.sif_id)
            .fetch_optional(pool)
            .await?;
    let (_, proof_interval) = sif_row
        .ok_or_else(|| AppError::NotFound(format!("sif id={}", input.sif_id)))?;
    if proof_interval <= 0 {
        return Err(AppError::Validation(format!(
            "sif {} proof_interval={} 无效",
            input.sif_id, proof_interval
        )));
    }
    validate_sop_ref(pool, org_id, input.sop_id).await?;

    let n = sqlx::query(
        "UPDATE proof_test SET
            sif_id = ?,
            tested_at = ?,
            result = ?,
            tested_by = ?,
            next_due_at = date(?, '+' || ? || ' months'),
            findings = ?,
            notes = ?,
            sop_id = ?
         WHERE org_id = ? AND id = ?",
    )
    .bind(input.sif_id)
    .bind(input.tested_at.trim())
    .bind(input.result.trim())
    .bind(input.tested_by.trim())
    .bind(input.tested_at.trim()) // date() 参数
    .bind(proof_interval)
    .bind(input.findings.trim())
    .bind(input.notes.trim())
    .bind(input.sop_id)
    .bind(org_id)
    .bind(id)
    .execute(pool)
    .await?
    .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("proof_test id={id}")));
    }

    let after = get_one(pool, org_id, id).await?;

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
        "proof_test_update",
        "proof_test",
        Some(id),
        payload,
    )
    .await;

    Ok(after)
}

// ---------------------------------------------------------------------------
// delete_proof_test
// ---------------------------------------------------------------------------

pub async fn delete_proof_test_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    id: i64,
) -> AppResult<u64> {
    let before = get_one(pool, org_id, id).await?;

    let n = sqlx::query("DELETE FROM proof_test WHERE org_id = ? AND id = ?")
        .bind(org_id)
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();

    if n == 0 {
        return Err(AppError::NotFound(format!("proof_test id={id}")));
    }

    let payload = entity_audit_payload(Some(snapshot(&before)), None, vec!["*".to_string()]);
    let _ = write_audit(
        pool,
        org_id,
        actor,
        "proof_test_delete",
        "proof_test",
        Some(id),
        payload,
    )
    .await;

    Ok(n)
}

// ---------------------------------------------------------------------------
// list_proof_test_history（复用 audit）
// ---------------------------------------------------------------------------

pub async fn list_proof_test_history_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    proof_test_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<crate::commands::audit::AuditEntry>> {
    list_history_for_target_inner(pool, org_id, "proof_test", proof_test_id, limit).await
}

// ---------------------------------------------------------------------------
// count_overdue_proof_tests —— 启动时扫逾期（IEC 61511-1 §16.3 合规缺口）
// ---------------------------------------------------------------------------

pub async fn count_overdue_proof_tests_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
) -> AppResult<OverdueProofTestStatus> {
    let row: (i64, Option<f64>) = sqlx::query_as(
        "SELECT
           COUNT(*) AS cnt,
           MAX(CAST((julianday('now') - julianday(pt.next_due_at)) AS REAL)) AS oldest_days
         FROM proof_test pt
         WHERE pt.org_id = ?
           AND date(pt.next_due_at) < date('now')
           AND pt.id = (SELECT MAX(id) FROM proof_test WHERE sif_id = pt.sif_id)",
    )
    .bind(org_id)
    .fetch_one(pool)
    .await?;

    Ok(OverdueProofTestStatus {
        count: row.0,
        oldest_overdue_days: row.1,
    })
}
