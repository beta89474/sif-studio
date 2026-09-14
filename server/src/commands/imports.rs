//! 仪表批量导入命令（M2.1）
//!
//! 设计：
//!   1. `preview_import_instruments(file_path)` — 调 import::parse_file 返回 ParsedSheet
//!   2. `commit_import_instruments(input)` — 应用 mapping + 事务批插入 + 写 audit
//!
//! 去重策略：
//!   - skip：遇 tag 已存在 → 跳过，计数 +1
//!   - overwrite：遇 tag 已存在 → UPDATE（关键字段），计数 +1
//!   - create_with_suffix：遇 tag 已存在 → 改名 tag_N 插入，计数 +1
//!
//! 失败模式：
//!   - 解析失败 → AppError::Import
//!   - 单行字段缺失 → 整行失败（记入 errors[]），其他行继续
//!   - 必填字段缺失或全部行失败 → 整批事务回滚
//!
//! 大文件：
//!   - 每 500 行一次子事务（不嵌套——只是周期 commit）
//!   - 总上限 50,000 行（防恶意大文件卡死 UI）

use crate::commands::audit::write_audit_best_effort;
use crate::import::ParsedSheet;
use crate::AppResult;
use serde::{Deserialize, Serialize};

const MAX_ROWS: usize = 50_000;
const BATCH_SIZE: usize = 500;

// ---------------------------------------------------------------------------
// 在线版：文件预览（parse）走 multipart 端点 POST /api/imports/preview（见 http/imports.rs），
// 不再有 preview_import_instruments 命令。
// commit_import_instruments 保留 RPC（前端把预览结果回传提交）。
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitImportInput {
    /// 解析好的 sheet（前端从 preview 拿到的同一份）
    pub sheet: ParsedSheet,
    /// 用户确认/覆盖过的字段映射：list of (column index, target field)
    pub mapping: Vec<MappingEntry>,
    /// skip | overwrite | create_with_suffix
    pub on_conflict: String,
    /// 已废弃（阶段 B 起多租户铁律：actor 只从登录会话派生，忽略前端传值）。
    /// 字段保留仅为旧客户端反序列化兼容。
    #[serde(default)]
    pub actor: Option<String>,
    /// M2.9 — 导入目标项目（必须属于当前登录组织）。
    pub project_id: i64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MappingEntry {
    pub column: usize,
    /// instrument 字段名：tag / service / kind / role / manufacturer / model / psv_id
    ///              / range_min / range_max / unit / setpoint / sil_target
    ///              / proof_interval / installed_at / notes
    pub target: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitImportResult {
    pub batch_id: i64, // audit_log 行 ID（用于回溯整批导入）
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<RowError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowError {
    pub row: usize,
    pub reason: String,
}

/// 不依赖 Tauri 的 inner 版本 —— HTTP RPC / 集成测试直接拿 SqlitePool 调它
///
/// 阶段 B：org_id / actor 均由 RPC 层从登录会话注入，不信任前端。
pub async fn commit_import_instruments_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    actor: &str,
    mut input: CommitImportInput,
) -> AppResult<CommitImportResult> {
    // 前端 actor 覆盖一律无视
    input.actor = None;
    // 校验输入
    if input.sheet.rows.len() > MAX_ROWS {
        return Err(crate::AppError::Import(format!(
            "too many rows: {} (max {MAX_ROWS})",
            input.sheet.rows.len()
        )));
    }
    if !["skip", "overwrite", "create_with_suffix"].contains(&input.on_conflict.as_str()) {
        return Err(crate::AppError::Import(format!(
            "invalid on_conflict: {}",
            input.on_conflict
        )));
    }
    if input.mapping.iter().filter(|m| m.target == "tag").count() == 0 {
        return Err(crate::AppError::Import(
            "mapping must include at least one `tag` column".into(),
        ));
    }
    // M2.9 — 导入必须指定项目
    if input.project_id <= 0 {
        return Err(crate::AppError::Import(
            "project_id is required (>=1)".into(),
        ));
    }
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(crate::AppError::Import(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    let mut out = CommitImportResult {
        batch_id: 0,
        inserted: 0,
        updated: 0,
        skipped: 0,
        failed: 0,
        errors: Vec::new(),
    };

    // 解析整批 → instrument draft
    let mut drafts: Vec<Option<InstrumentDraft>> = Vec::with_capacity(input.sheet.rows.len());
    for (row_idx, row) in input.sheet.rows.iter().enumerate() {
        match row_to_draft(row, &input.mapping, org_id, input.project_id) {
            Ok(d) => drafts.push(Some(d)),
            Err(reason) => {
                out.failed += 1;
                out.errors.push(RowError {
                    row: row_idx + 2, // +1 表头，+1 转 1-based
                    reason,
                });
                drafts.push(None);
            }
        }
    }

    if out.failed == drafts.len() {
        // 全失败 → 没必要开事务
        return Ok(out);
    }

    // ──────────────────── 写库（事务批插入） ────────────────────
    let mut tx = pool.begin().await?;

    // 先写 audit（批头），拿到 batch_id
    let audit_payload = serde_json::json!({
        "file": input.sheet.source_name,
        "format": input.sheet.format,
        "totalRows": input.sheet.rows.len(),
        "onConflict": input.on_conflict,
        "mapping": input.mapping,
    });

    // 用一个特殊 audit 行标记批次开始（org_id / actor 来自会话）
    sqlx::query(
        "INSERT INTO audit_log (org_id, actor, action, target_table, target_id, payload_json)
         VALUES (?, ?, 'import', 'instrument_batch', NULL, ?)",
    )
    .bind(org_id)
    .bind(actor)
    .bind(audit_payload.to_string())
    .execute(&mut *tx)
    .await?;
    out.batch_id = sqlx::query("SELECT last_insert_rowid() AS id")
        .fetch_one(&mut *tx)
        .await?
        .get::<i64, _>(0);

    // ── 批插入 ──
    let mut processed = 0usize;
    for (row_idx, draft_opt) in drafts.into_iter().enumerate() {
        let Some(draft) = draft_opt else { continue };

        let outcome = match input.on_conflict.as_str() {
            "skip" => insert_skip(&mut tx, &draft).await,
            "overwrite" => insert_overwrite(&mut tx, &draft).await,
            "create_with_suffix" => insert_with_suffix(&mut tx, &draft).await,
            _ => unreachable!(),
        };

        match outcome {
            Ok(InsertOutcome::Inserted(id)) => {
                out.inserted += 1;
                write_per_row_audit(&mut tx, &draft, actor, "create", Some(id)).await;
            }
            Ok(InsertOutcome::Updated(id)) => {
                out.updated += 1;
                write_per_row_audit(&mut tx, &draft, actor, "update", Some(id)).await;
            }
            Ok(InsertOutcome::Skipped) => {
                out.skipped += 1;
            }
            Err(e) => {
                out.failed += 1;
                out.errors.push(RowError {
                    row: row_idx + 2,
                    reason: format!("db: {e}"),
                });
            }
        }

        processed += 1;
        if processed % BATCH_SIZE == 0 {
            // 周期 commit 子批，避免锁过久
            tx.commit().await?;
            tx = pool.begin().await?;
        }
    }

    tx.commit().await?;

    // ── 写批次汇总审计（best-effort） ──
    let summary = serde_json::json!({
        "batchId": out.batch_id,
        "inserted": out.inserted,
        "updated": out.updated,
        "skipped": out.skipped,
        "failed": out.failed,
    });
    write_audit_best_effort(
        pool,
        org_id,
        actor,
        "import_done",
        "instrument_batch",
        Some(out.batch_id),
        summary,
    )
    .await;

    Ok(out)
}

// ---------------------------------------------------------------------------
// Row → InstrumentDraft
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct InstrumentDraft {
    tag: String,
    service: String,
    kind: String,
    role: String,
    manufacturer: String,
    model: String,
    psv_id: String,
    range_min: Option<f64>,
    range_max: Option<f64>,
    unit: String,
    setpoint: Option<f64>,
    sil_target: String,
    proof_interval: i64,
    installed_at: String,
    notes: String,
    /// 阶段 B — 组织归属（会话派生）
    org_id: i64,
    /// M2.9 — 归属项目（从 CommitImportInput 透传到每行 draft）
    project_id: i64,
}

fn row_to_draft(
    row: &[String],
    mapping: &[MappingEntry],
    org_id: i64,
    project_id: i64,
) -> Result<InstrumentDraft, String> {
    let mut d = InstrumentDraft {
        tag: String::new(),
        service: String::new(),
        kind: String::new(),
        role: String::new(),
        manufacturer: String::new(),
        model: String::new(),
        psv_id: String::new(),
        range_min: None,
        range_max: None,
        unit: String::new(),
        setpoint: None,
        sil_target: "NA".into(),
        proof_interval: 0,
        installed_at: String::new(),
        notes: String::new(),
        org_id,
        project_id,
    };

    for m in mapping {
        let raw = row.get(m.column).map(String::as_str).unwrap_or("").trim();
        apply_field(&mut d, &m.target, raw).map_err(|e| format!("col {}: {e}", m.column))?;
    }

    if d.tag.is_empty() {
        return Err("tag is empty".into());
    }
    if !["detector", "final", "logic", "aux"].contains(&d.role.as_str()) {
        return Err(format!(
            "invalid role '{}' (need detector/final/logic/aux)",
            d.role
        ));
    }
    if !["NA", "A", "B", "C", "D"].contains(&d.sil_target.as_str()) {
        return Err(format!("invalid sil_target '{}'", d.sil_target));
    }
    if d.kind.is_empty() {
        return Err("kind is empty".into());
    }
    Ok(d)
}

fn apply_field(d: &mut InstrumentDraft, target: &str, raw: &str) -> Result<(), String> {
    match target {
        "tag" => d.tag = raw.into(),
        "service" => d.service = raw.into(),
        "kind" => d.kind = raw.into(),
        "role" => d.role = raw.into(),
        "manufacturer" => d.manufacturer = raw.into(),
        "model" => d.model = raw.into(),
        "psv_id" => d.psv_id = raw.into(),
        "unit" => d.unit = raw.into(),
        "sil_target" => d.sil_target = raw.into(),
        "installed_at" => d.installed_at = raw.into(),
        "notes" => d.notes = raw.into(),
        "range_min" => d.range_min = parse_opt_f64(raw, "range_min")?,
        "range_max" => d.range_max = parse_opt_f64(raw, "range_max")?,
        "setpoint" => d.setpoint = parse_opt_f64(raw, "setpoint")?,
        "proof_interval" => {
            d.proof_interval = parse_opt_i64(raw, "proof_interval")?.unwrap_or(0);
        }
        // 兼容 range 单列：解析 "0-100" / "0~100" / "0,100"
        "range" => {
            let (lo, hi) = parse_range(raw)?;
            d.range_min = lo;
            d.range_max = hi;
        }
        _ => {
            // 忽略未知 target，不报错（让用户多选无伤大雅）
        }
    }
    Ok(())
}

fn parse_opt_f64(raw: &str, name: &str) -> Result<Option<f64>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<f64>()
        .map(Some)
        .map_err(|_| format!("{name} not a number: '{raw}'"))
}

fn parse_opt_i64(raw: &str, name: &str) -> Result<Option<i64>, String> {
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<i64>()
        .map(Some)
        .map_err(|_| format!("{name} not an integer: '{raw}'"))
}

fn parse_range(raw: &str) -> Result<(Option<f64>, Option<f64>), String> {
    if raw.is_empty() {
        return Ok((None, None));
    }
    let sep = raw
        .find(['-', '~', ',', '/', '—'])
        .ok_or_else(|| format!("range needs separator (-,~,/,): '{raw}'"))?;
    let (lo_s, hi_s) = raw.split_at(sep);
    let hi_s = &hi_s[1..]; // skip separator
    let parse_one = |s: &str| -> Result<Option<f64>, String> {
        let t = s.trim();
        if t.is_empty() {
            Ok(None)
        } else {
            t.parse::<f64>()
                .map(Some)
                .map_err(|e| format!("range parse '{t}': {e}"))
        }
    };
    Ok((parse_one(lo_s)?, parse_one(hi_s)?))
}

// ---------------------------------------------------------------------------
// 三种插入策略
// ---------------------------------------------------------------------------

enum InsertOutcome {
    Inserted(i64),
    Updated(i64),
    Skipped,
}

async fn insert_skip(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    d: &InstrumentDraft,
) -> AppResult<InsertOutcome> {
    let r = sqlx::query(
        "INSERT OR IGNORE INTO instrument
           (org_id, tag, service, kind, role, psv_id, manufacturer, model,
            range_min, range_max, unit, setpoint, sil_target,
            proof_interval, installed_at, notes, project_id)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(d.org_id)
    .bind(&d.tag)
    .bind(&d.service)
    .bind(&d.kind)
    .bind(&d.role)
    .bind(&d.psv_id)
    .bind(&d.manufacturer)
    .bind(&d.model)
    .bind(d.range_min)
    .bind(d.range_max)
    .bind(&d.unit)
    .bind(d.setpoint)
    .bind(&d.sil_target)
    .bind(d.proof_interval)
    .bind(&d.installed_at)
    .bind(&d.notes)
    .bind(d.project_id)
    .execute(&mut **tx)
    .await?;
    if r.rows_affected() == 1 {
        let id = r.last_insert_rowid();
        Ok(InsertOutcome::Inserted(id))
    } else {
        Ok(InsertOutcome::Skipped)
    }
}

async fn insert_overwrite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    d: &InstrumentDraft,
) -> AppResult<InsertOutcome> {
    // M2.9 — (project_id, tag) 联合唯一：用 org_id + project_id + tag 定位被覆盖的行
    let r = sqlx::query(
        "UPDATE instrument SET
           service=?, kind=?, role=?, psv_id=?, manufacturer=?, model=?,
           range_min=?, range_max=?, unit=?, setpoint=?, sil_target=?,
           proof_interval=?, installed_at=?, notes=?, updated_at=datetime('now')
         WHERE org_id=? AND project_id=? AND tag=?",
    )
    .bind(&d.service)
    .bind(&d.kind)
    .bind(&d.role)
    .bind(&d.psv_id)
    .bind(&d.manufacturer)
    .bind(&d.model)
    .bind(d.range_min)
    .bind(d.range_max)
    .bind(&d.unit)
    .bind(d.setpoint)
    .bind(&d.sil_target)
    .bind(d.proof_interval)
    .bind(&d.installed_at)
    .bind(&d.notes)
    .bind(d.org_id)
    .bind(d.project_id)
    .bind(&d.tag)
    .execute(&mut **tx)
    .await?;
    if r.rows_affected() == 0 {
        // 该 (project_id, tag) 不存在 → 当成 insert
        let r2 = sqlx::query(
            "INSERT INTO instrument
               (org_id, tag, service, kind, role, psv_id, manufacturer, model,
                range_min, range_max, unit, setpoint, sil_target,
                proof_interval, installed_at, notes, project_id)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(d.org_id)
        .bind(&d.tag)
        .bind(&d.service)
        .bind(&d.kind)
        .bind(&d.role)
        .bind(&d.psv_id)
        .bind(&d.manufacturer)
        .bind(&d.model)
        .bind(d.range_min)
        .bind(d.range_max)
        .bind(&d.unit)
        .bind(d.setpoint)
        .bind(&d.sil_target)
        .bind(d.proof_interval)
        .bind(&d.installed_at)
        .bind(&d.notes)
        .bind(d.project_id)
        .execute(&mut **tx)
        .await?;
        Ok(InsertOutcome::Inserted(r2.last_insert_rowid()))
    } else {
        // 找到被 UPDATE 的行 ID
        let id: i64 =
            sqlx::query("SELECT id FROM instrument WHERE org_id=? AND project_id=? AND tag=?")
                .bind(d.org_id)
                .bind(d.project_id)
                .bind(&d.tag)
                .fetch_one(&mut **tx)
                .await?
                .get::<i64, _>(0);
        Ok(InsertOutcome::Updated(id))
    }
}

async fn insert_with_suffix(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    d: &InstrumentDraft,
) -> AppResult<InsertOutcome> {
    // 测一遍是否需要 suffix（按 org + project_id + tag 查）
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM instrument WHERE org_id=? AND project_id=? AND tag=?")
            .bind(d.org_id)
            .bind(d.project_id)
            .bind(&d.tag)
            .fetch_optional(&mut **tx)
            .await?;

    let final_tag = if existing.is_none() {
        d.tag.clone()
    } else {
        // 找下一个不冲突的 _N 后缀（同组织同项目内）
        find_available_suffix(tx, d.org_id, d.project_id, &d.tag).await?
    };

    let r = sqlx::query(
        "INSERT INTO instrument
           (org_id, tag, service, kind, role, psv_id, manufacturer, model,
            range_min, range_max, unit, setpoint, sil_target,
            proof_interval, installed_at, notes, project_id)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(d.org_id)
    .bind(&final_tag)
    .bind(&d.service)
    .bind(&d.kind)
    .bind(&d.role)
    .bind(&d.psv_id)
    .bind(&d.manufacturer)
    .bind(&d.model)
    .bind(d.range_min)
    .bind(d.range_max)
    .bind(&d.unit)
    .bind(d.setpoint)
    .bind(&d.sil_target)
    .bind(d.proof_interval)
    .bind(&d.installed_at)
    .bind(&d.notes)
    .bind(d.project_id)
    .execute(&mut **tx)
    .await?;
    Ok(InsertOutcome::Inserted(r.last_insert_rowid()))
}

async fn find_available_suffix(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    org_id: i64,
    project_id: i64,
    base_tag: &str,
) -> AppResult<String> {
    let mut n = 1u32;
    loop {
        let candidate = format!("{base_tag}_{n}");
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM instrument WHERE org_id=? AND project_id=? AND tag=?",
        )
        .bind(org_id)
        .bind(project_id)
        .bind(&candidate)
        .fetch_optional(&mut **tx)
        .await?;
        if exists.is_none() {
            return Ok(candidate);
        }
        n += 1;
        if n > 9999 {
            return Err(crate::AppError::Import(format!(
                "too many suffix attempts for tag {base_tag}"
            )));
        }
    }
}

async fn write_per_row_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    d: &InstrumentDraft,
    actor: &str,
    action: &str,
    id: Option<i64>,
) {
    let payload = serde_json::json!({
        "tag": d.tag,
        "kind": d.kind,
        "role": d.role,
        "silTarget": d.sil_target,
    });
    let _ = sqlx::query(
        "INSERT INTO audit_log (org_id, actor, action, target_table, target_id, payload_json)
         VALUES (?, ?, ?, 'instrument', ?, ?)",
    )
    .bind(d.org_id)
    .bind(actor)
    .bind(action)
    .bind(id)
    .bind(payload.to_string())
    .execute(&mut **tx)
    .await;
}

// sqlx 0.8 中需要这个 trait 才能 fetch_one().get()
use sqlx::Row;

// ---------------------------------------------------------------------------
// 单元测试（解析 + draft）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_range_variants() {
        assert_eq!(parse_range("0-100").unwrap(), (Some(0.0), Some(100.0)));
        assert_eq!(parse_range("0~500").unwrap(), (Some(0.0), Some(500.0)));
        assert_eq!(parse_range("0,1000").unwrap(), (Some(0.0), Some(1000.0)));
        assert_eq!(parse_range("0/200").unwrap(), (Some(0.0), Some(200.0)));
        assert_eq!(parse_range("").unwrap(), (None, None));
        assert!(parse_range("abc").is_err());
    }

    #[test]
    fn row_to_draft_validates_role() {
        let mapping = vec![
            MappingEntry {
                column: 0,
                target: "tag".into(),
            },
            MappingEntry {
                column: 1,
                target: "kind".into(),
            },
            MappingEntry {
                column: 2,
                target: "role".into(),
            },
        ];
        let row = vec!["PT-201".into(), "PT".into(), "detector".into()];
        assert!(row_to_draft(&row, &mapping, 1, 1).is_ok());

        let bad = vec!["PT-201".into(), "PT".into(), "weird".into()];
        assert!(row_to_draft(&bad, &mapping, 1, 1).is_err());
    }

    #[test]
    fn row_to_draft_requires_tag() {
        let mapping = vec![MappingEntry {
            column: 0,
            target: "tag".into(),
        }];
        let row = vec![String::new()];
        let err = row_to_draft(&row, &mapping, 1, 1).unwrap_err();
        assert!(err.contains("empty"));
    }
}
