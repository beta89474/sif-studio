//! 位号批量导入（M1.5：M0 编辑器自带 P&ID 位号粘贴 / CSV）
//!
//! 现实路径：
//!   1. 用户维护一份位号表（P&ID 截图或 DCS 数据库导出）
//!   2. 把 CSV / TSV 三段批量贴进导入框
//!   3. 后端 parse → 写库；若位号已存在就按策略 skip / overwrite / duplicate
//!
//! 阶段 B：org_id / project_id 作用域；只写当前组织、目标项目内的仪表。

use crate::AppResult;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TagRow {
    pub tag: String,
    pub service: String,
    pub kind: String,
    pub role: String,
    #[serde(default)]
    pub unit: String,
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    pub setpoint: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvImport {
    pub rows: Vec<TagRow>,
    /// 目标项目（必须属于当前组织）
    pub project_id: i64,
    /// skip | overwrite | duplicate
    #[serde(default = "default_strategy")]
    pub on_conflict: String,
}

fn default_strategy() -> String {
    "skip".into()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub inserted: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub errors: Vec<String>,
}

/// MVP：先实现 skip；其余策略 M1 后续迭代
pub async fn import_tags_csv_inner(
    pool: &sqlx::SqlitePool,
    org_id: i64,
    input: CsvImport,
) -> AppResult<ImportResult> {
    let mut out = ImportResult {
        inserted: 0,
        updated: 0,
        skipped: 0,
        failed: 0,
        errors: Vec::new(),
    };

    // 项目必须属于当前组织
    let proj_exists: Option<i64> =
        sqlx::query_scalar("SELECT id FROM project WHERE org_id = ? AND id = ?")
            .bind(org_id)
            .bind(input.project_id)
            .fetch_optional(pool)
            .await?;
    if proj_exists.is_none() {
        return Err(crate::AppError::Validation(format!(
            "project_id {} not found in current organization",
            input.project_id
        )));
    }

    for row in input.rows.iter() {
        if row.tag.trim().is_empty() {
            out.failed += 1;
            out.errors.push(format!("empty tag: {row:?}"));
            continue;
        }
        if !["detector", "final", "logic", "aux"].contains(&row.role.as_str()) {
            out.failed += 1;
            out.errors
                .push(format!("invalid role '{}' on {}", row.role, row.tag));
            continue;
        }

        let res = sqlx::query(
            "INSERT OR IGNORE INTO instrument
                (org_id, project_id, tag, service, kind, role, unit,
                 range_min, range_max, setpoint, sil_target, proof_interval,
                 installed_at, notes, psv_id, manufacturer, model)
             VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(org_id)
        .bind(input.project_id)
        .bind(&row.tag)
        .bind(&row.service)
        .bind(&row.kind)
        .bind(&row.role)
        .bind(&row.unit)
        .bind(row.range_min)
        .bind(row.range_max)
        .bind(row.setpoint)
        .bind("NA")
        .bind(0)
        .bind("")
        .bind("")
        .bind("")
        .bind("")
        .bind("")
        .execute(pool)
        .await;

        match res {
            Ok(r) => {
                if r.rows_affected() == 1 {
                    out.inserted += 1;
                } else {
                    out.skipped += 1;
                }
            }
            Err(e) => {
                out.failed += 1;
                out.errors.push(format!("{}: {}", row.tag, e));
            }
        }
    }

    Ok(out)
}
