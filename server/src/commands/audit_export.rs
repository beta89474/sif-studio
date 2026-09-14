//! 审计包导出（M2.5）
//!
//! 目标：把 `audit_log` 表按用户选定的筛选条件导出为 CSV 文件，
//! 方便用户做项目移交、合规审计、外部分析。
//!
//! 设计要点：
//!   - **筛选维度**：target_table / action / actor / ts_from / ts_to / target_id（六维，全可选）
//!   - **WHERE 1=1 + 动态拼条件**：sqlx 0.8 编译期 SQL 校验不喜欢 `query_as_with` 的动态 SQL，
//!     所以改用 `sqlx::query` + 手动 bind（用 `query_as` 返 `Vec<(7 元组)>`，再 map 成 AuditEntry）
//!   - **SQLite TEXT 时间比较**：ts 列存的是**混合格式**（M2.1 import 写空格分隔；M2.3+ 写 RFC 3339），
//!     不能包 `datetime()` —— 改成直接字典序（裸 `ts >= ?`），理由见 `build_filter` 注释
//!   - **CSV 带 BOM**：写文件时先写 `\xEF\xBB\xBF`，Excel 打开中文不乱码
//!   - **payload_json 解析失败兜底为 null**，不阻断导出
//!   - **动作白名单（CSV 列）**：`{ts, actor, action, target_table, target_id,
//!                                    before_json, after_json, fields_changed, note, audit_id}`
//!     与 AuditEntry 字段一一对应，方便和审计历史抽屉互通
//!
//! 复用 helper：
//!   - `audit::AuditEntry`（解析后的 4 段 typed 字段）
//!   - `audit::list_history_for_target_inner`（单实体历史）—— **不**用这个做筛选
//!     因为它只支持 (table, id, limit)；本文件的 filter 需要更多维度
//!
//! 命令清单：
//!   - `list_audit_filters`         返 `{actions, actors, tables}` 给前端下拉用
//!   - `count_audit_filtered`       返 i64（导出前预览数量）
//!   - `preview_audit_filtered`     返 Vec<AuditEntry>（UI 实时预览前 N 条）
//!   - `export_audit_csv`           写文件返 `{ path, bytes, rows }`

use crate::commands::audit::AuditEntry;
use crate::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

// ===========================================================================
// 1. AuditFilter —— 与前端 TS 类型一一对应（camelCase 序列化）
// ===========================================================================

/// 审计筛选条件。所有字段都是 Option，None = 不筛选。
///
/// 前端字段名严格驼峰，与 Rust struct 的 `#[serde(rename_all = "camelCase")]` 对齐。
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditFilterInput {
    /// 'instrument' | 'sif' | 'project' | 'bypass_record' | 'diagram' | ...
    #[serde(default)]
    pub target_table: Option<String>,
    /// 'instrument_create' | 'sif_update' | 'sif_link' | ...
    #[serde(default)]
    pub action: Option<String>,
    /// OS 用户名（如 '张工'）
    #[serde(default)]
    pub actor: Option<String>,
    /// 起始时间 RFC 3339（如 '2026-09-01T00:00:00Z'）；闭区间
    #[serde(default)]
    pub ts_from: Option<String>,
    /// 结束时间 RFC 3339；闭区间
    #[serde(default)]
    pub ts_to: Option<String>,
    /// 单实体 ID（精确匹配）
    #[serde(default)]
    pub target_id: Option<i64>,
}

/// 把 `AuditFilterInput`（用户输入，未清洗）转成 `AuditFilterApplied`（已清洗 + 动态 SQL + 参数）。
///
/// 返回的 `(where_sql, binds)` 给所有 SQL 函数复用：
///   - `where_sql` 是「WHERE 1=1 AND ... AND ...」拼接串
///   - `binds` 是参数列表（顺序敏感），用于 sqlx::query.bind()
struct AuditFilterApplied {
    /// 「 AND ... AND ...」拼接串（不包含 WHERE 关键字）；空 = 无筛选
    where_sql: String,
    /// 与 where_sql 顺序一致的绑定值
    binds: Vec<String>,
}

fn build_filter(filter: &AuditFilterInput) -> AuditFilterApplied {
    let mut conds: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();

    if let Some(ref t) = filter.target_table {
        if !t.trim().is_empty() {
            conds.push("target_table = ?".into());
            binds.push(t.clone());
        }
    }
    if let Some(ref a) = filter.action {
        if !a.trim().is_empty() {
            conds.push("action = ?".into());
            binds.push(a.clone());
        }
    }
    if let Some(ref u) = filter.actor {
        if !u.trim().is_empty() {
            conds.push("actor = ?".into());
            binds.push(u.clone());
        }
    }
    // 时间比较：直接字典序（不包 datetime()）。
    //   原因：audit_log.ts 列里**两种格式并存**——
    //     M2.1 import 写的是 `datetime('now')` 空格分隔（`2026-09-13 06:07:35`）
    //     M2.3+ audit 写的是 RFC 3339 T 分隔（`2026-09-13T06:07:35Z`）
    //   若用 `datetime(ts) >= datetime(?)`：
    //     - RFC 3339 的 ts 经 datetime() 解析会返 NULL（不识别 T 分隔）→ 三值逻辑 UNKNOWN → 不命中
    //   若用 `ts >= ?`（裸字典序）：
    //     - 用户传 RFC 3339（如 `2026-09-01T00:00:00Z`）
    //     - 空格分隔的 ts 在第 11 位是 `' '`(0x20)，RFC 3339 是 `'T'`(0x54)
    //     - 空格分隔在字典序里 < RFC 3339 → ts_from 筛选时这些行被算作"更早"→ **仍命中**（不漏数据）
    //     - ts_to 筛选时同样不会漏（空格分隔 < RFC 3339 上界）
    //     - RFC 3339 内部的字典序就是时间序（ISO 8601 设计保证）
    if let Some(ref s) = filter.ts_from {
        if !s.trim().is_empty() {
            conds.push("ts >= ?".into());
            binds.push(s.clone());
        }
    }
    if let Some(ref s) = filter.ts_to {
        if !s.trim().is_empty() {
            conds.push("ts <= ?".into());
            binds.push(s.clone());
        }
    }
    if let Some(id) = filter.target_id {
        conds.push("target_id = ?".into());
        binds.push(id.to_string());
    }

    AuditFilterApplied {
        where_sql: if conds.is_empty() {
            String::new()
        } else {
            format!(" AND {}", conds.join(" AND "))
        },
        binds,
    }
}

// ===========================================================================
// 2. 列表筛选元数据 —— 给前端下拉框用
// ===========================================================================

/// 审计可筛选维度（actions / actors / tables）—— 给前端下拉框用
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditFilterOptions {
    pub actions: Vec<String>,
    pub actors: Vec<String>,
    pub tables: Vec<String>,
}

pub async fn list_audit_filters_inner(
    pool: &SqlitePool,
    org_id: i64,
) -> AppResult<AuditFilterOptions> {
    let actions: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT action FROM audit_log WHERE org_id = ? ORDER BY action")
            .bind(org_id)
            .fetch_all(pool)
            .await?;
    let actors: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT actor FROM audit_log WHERE org_id = ? AND actor != '' ORDER BY actor",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    let tables: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT target_table FROM audit_log WHERE org_id = ? ORDER BY target_table",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;
    Ok(AuditFilterOptions {
        actions: actions.into_iter().map(|(s,)| s).collect(),
        actors: actors.into_iter().map(|(s,)| s).collect(),
        tables: tables.into_iter().map(|(s,)| s).collect(),
    })
}

// ===========================================================================
// 3. 筛选后的统计与预览 —— 与 audit_log 一致的 SQL 路径
// ===========================================================================

/// 筛选命中数（导出前预览用）
pub async fn count_audit_filtered_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<i64> {
    let f = build_filter(filter);
    let sql = format!(
        "SELECT COUNT(*) AS n FROM audit_log WHERE org_id = ? AND 1=1{}",
        f.where_sql
    );
    let mut q = sqlx::query_as::<_, (i64,)>(&sql).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let row = q.fetch_one(pool).await?;
    Ok(row.0)
}

/// 筛选后命中行的汇总（按 action / table 维度分组）—— 给前端报表用
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditSummary {
    pub total: i64,
    pub by_action: HashMap<String, i64>,
    pub by_table: HashMap<String, i64>,
}

pub async fn summarize_audit_filtered_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<AuditSummary> {
    let f = build_filter(filter);

    let sql_total = format!(
        "SELECT COUNT(*) FROM audit_log WHERE org_id = ? AND 1=1{}",
        f.where_sql
    );
    let mut q = sqlx::query_as::<_, (i64,)>(&sql_total).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let total: i64 = q.fetch_one(pool).await?.0;

    let sql_by_action = format!(
        "SELECT action, COUNT(*) FROM audit_log WHERE org_id = ? AND 1=1{} GROUP BY action ORDER BY action",
        f.where_sql
    );
    let mut q = sqlx::query(&sql_by_action).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let rows_action = q.fetch_all(pool).await?;
    let mut by_action: HashMap<String, i64> = HashMap::new();
    for row in rows_action {
        let action: String = row.try_get(0).unwrap_or_default();
        let n: i64 = row.try_get(1).unwrap_or(0);
        by_action.insert(action, n);
    }

    let sql_by_table = format!(
        "SELECT target_table, COUNT(*) FROM audit_log WHERE org_id = ? AND 1=1{} GROUP BY target_table ORDER BY target_table",
        f.where_sql
    );
    let mut q = sqlx::query(&sql_by_table).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let rows_table = q.fetch_all(pool).await?;
    let mut by_table: HashMap<String, i64> = HashMap::new();
    for row in rows_table {
        let t: String = row.try_get(0).unwrap_or_default();
        let n: i64 = row.try_get(1).unwrap_or(0);
        by_table.insert(t, n);
    }

    Ok(AuditSummary {
        total,
        by_action,
        by_table,
    })
}

/// 实时预览前 N 条 —— 与 EntityHistoryDrawer 的数据形状一致
pub async fn preview_audit_filtered_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
    limit: Option<i64>,
) -> AppResult<Vec<AuditEntry>> {
    let lim = limit.map(|n| n.clamp(1, 1000)).unwrap_or(100);
    let f = build_filter(filter);
    let sql = format!(
        "SELECT id, ts, actor, action, target_table, target_id, payload_json
         FROM audit_log WHERE org_id = ? AND 1=1{}
         ORDER BY id DESC LIMIT ?",
        f.where_sql
    );

    let mut q =
        sqlx::query_as::<_, (i64, String, String, String, String, Option<i64>, String)>(&sql)
            .bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    q = q.bind(lim);

    let rows = q.fetch_all(pool).await?;

    let mut out: Vec<AuditEntry> = Vec::with_capacity(rows.len());
    for (id, ts, actor, action, target_table, target_id, payload_json) in rows {
        // 与 audit::list_history_for_target_inner 的解析逻辑完全一致——抽离代价不大，
        // 但保持本地实现避免改 public API。
        let parsed: serde_json::Value =
            serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
        let obj = parsed.as_object().cloned().unwrap_or_default();
        let before = obj.get("before").cloned().filter(|v| !v.is_null());
        let after = obj.get("after").cloned().filter(|v| !v.is_null());
        let fields_changed = obj
            .get("fieldsChanged")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let note = obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();
        out.push(AuditEntry {
            id,
            ts,
            actor,
            action,
            target_table,
            target_id,
            payload_json,
            before,
            after,
            fields_changed,
            note,
        });
    }
    Ok(out)
}

// ===========================================================================
// 4. 导出 CSV
// ===========================================================================

/// 单次导出的最大数据行数（防审计表增长后全表进内存）。
/// 命中上限时导出**最新**的 MAX_EXPORT_ROWS 行（按 id DESC），并置 truncated 标志。
pub const MAX_EXPORT_ROWS: i64 = 100_000;

/// 导出结果（写入字节数 + 行数 + 文件路径 + 是否被行数上限截断）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub bytes: u64,
    pub rows: i64,
    pub truncated: bool,
}

/// 在线版：构造 CSV 字节（BOM + 表头 + 数据行），交由 HTTP 端点流式下载。
/// 返回 (csv_bytes, row_count, truncated)。
pub async fn build_audit_csv_bytes(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<(Vec<u8>, i64, bool)> {
    let f = build_filter(filter);
    // 多取 1 行探测是否还有更多（LIMIT 不含表头）
    let sql = format!(
        "SELECT id, ts, actor, action, target_table, target_id, payload_json
         FROM audit_log WHERE org_id = ? AND 1=1{}
         ORDER BY id DESC LIMIT ?",
        f.where_sql
    );
    let mut q =
        sqlx::query_as::<_, (i64, String, String, String, String, Option<i64>, String)>(&sql)
            .bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    q = q.bind(MAX_EXPORT_ROWS + 1);
    let all_rows = q.fetch_all(pool).await?;
    let truncated = all_rows.len() as i64 > MAX_EXPORT_ROWS;
    let rows: &[_] = if truncated {
        &all_rows[..MAX_EXPORT_ROWS as usize]
    } else {
        &all_rows
    };

    // BOM + 表头 + 数据行。用 `csv` crate 的 Writer 自动处理转义（逗号 / 引号 / 换行）。
    // BOM 不能作 record 写入（字段数不一致），直接 push 到初始 Vec<u8>。
    let mut raw: Vec<u8> = Vec::with_capacity(8 * 1024);
    raw.extend_from_slice(b"\xEF\xBB\xBF"); // UTF-8 BOM（让 Excel 把 CSV 识别为 UTF-8）
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false) // 自己写表头
        .from_writer(raw);
    wtr.write_record([
        "audit_id",
        "ts",
        "actor",
        "action",
        "target_table",
        "target_id",
        "fields_changed",
        "before_json",
        "after_json",
        "note",
    ])
    .map_err(|e| AppError::Validation(format!("csv header: {e}")))?;

    let mut count: i64 = 0;
    for (id, ts, actor, action, target_table, target_id, payload_json) in rows {
        let parsed: serde_json::Value =
            serde_json::from_str(payload_json).unwrap_or(serde_json::Value::Null);
        let obj = parsed.as_object().cloned().unwrap_or_default();
        let before = obj
            .get("before")
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    v.to_string()
                }
            })
            .unwrap_or_default();
        let after = obj
            .get("after")
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    v.to_string()
                }
            })
            .unwrap_or_default();
        let fields_changed = obj
            .get("fieldsChanged")
            .map(|v| {
                v.as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str())
                            .collect::<Vec<_>>()
                            .join("|")
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let note = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        wtr.write_record([
            &id.to_string(),
            ts,
            actor,
            action,
            target_table,
            &target_id.map(|n| n.to_string()).unwrap_or_default(),
            &fields_changed,
            &before,
            &after,
            note,
        ])
        .map_err(|e| AppError::Validation(format!("csv row: {e}")))?;
        count += 1;
    }
    let bytes = wtr
        .into_inner()
        .map_err(|e| AppError::Validation(format!("csv serialization failed: {e}")))?;

    Ok((bytes, count, truncated))
}

/// 遗留入口：导出筛选后的审计日志到本地 CSV 文件（测试 / 桌面兼容）。
///
/// - 自动加 UTF-8 BOM (`\xEF\xBB\xBF`)，Excel 双击中文不乱码
/// - 列：`audit_id, ts, actor, action, target_table, target_id, fields_changed, before_json, after_json, note`
/// - 失败：路径无效 → AppError::Validation；写盘失败 → AppError::Validation（带 io 错文本）
pub async fn export_audit_csv_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
    output_path: &str,
) -> AppResult<ExportResult> {
    if output_path.trim().is_empty() {
        return Err(AppError::Validation("output_path is required".into()));
    }
    // 防御性：路径必须有盘符或 / 开头（防误传相对路径污染工作目录）
    let p = std::path::Path::new(output_path);
    if !p.is_absolute() {
        return Err(AppError::Validation(format!(
            "output_path must be absolute, got '{output_path}'"
        )));
    }

    let (bytes, count, truncated) = build_audit_csv_bytes(pool, org_id, filter).await?;
    let bytes_len = bytes.len() as u64;

    // 写盘
    std::fs::write(p, &bytes)
        .map_err(|e| AppError::Validation(format!("write '{output_path}': {e}")))?;

    Ok(ExportResult {
        path: output_path.to_string(),
        bytes: bytes_len,
        rows: count,
        truncated,
    })
}

// ===========================================================================
// 5. 分析图表（M2.7）—— 三个聚合供 PDF 报告增补图表使用
// ===========================================================================

/// 每日审计活动量 —— 用于"每日活动趋势"柱图
///
/// 按 `substr(ts, 1, 10)` 切天（兼容 RFC 3339 `2026-09-13T...` 和 `datetime('now')` 空格分隔，
/// 两种格式前 10 字符都是 `YYYY-MM-DD`）。
///
/// 返回从最早日期到今天的有序数组，**缺失日期不补 0**（前端图表库补零更方便）。
/// 但若筛选时间范围内某日确实为 0，会从结果中消失 —— 调用方需自行决定补零策略。
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DailyActivity {
    pub date: String,
    pub count: i64,
}

pub async fn compute_daily_activity_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<Vec<DailyActivity>> {
    let f = build_filter(filter);
    // 注意：ts 列混合格式（M2.1 写空格；M2.3+ 写 RFC 3339），但两种前 10 字符都是日期。
    // 不能用 datetime(ts) —— RFC 3339 会返 NULL → 行丢失（与 M2.5 build_filter 同理）。
    let sql = format!(
        "SELECT substr(ts, 1, 10) AS d, COUNT(*) AS n
         FROM audit_log
         WHERE org_id = ? AND 1=1{}
         GROUP BY d
         ORDER BY d ASC",
        f.where_sql
    );
    let mut q = sqlx::query_as::<_, (String, i64)>(&sql).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(date, count)| DailyActivity { date, count })
        .collect())
}

/// 旁路时长分布 —— 用于"旁路时长柱图"
///
/// 按 5 个固定桶分：<1h / 1-8h / 8-24h / 1-7d / >7d
/// 仅统计 `restored_at != ''` 的已恢复旁路（未恢复的不算"时长"）。
/// 桶边界为 0/1h/8h/24h/7d，按天计算。
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DurationBucket {
    pub label: String,
    pub count: i64,
    /// 桶下界（小时，用于排序 / 排序稳定）
    pub lower_hours: f64,
}

const BYPASS_BUCKETS: &[(&str, f64, f64)] = &[
    ("<1 小时", 0.0, 1.0),           // 下界 0h，上界 1h
    ("1–8 小时", 1.0, 8.0),          // 1h-8h
    ("8–24 小时", 8.0, 24.0),        // 8h-24h
    ("1–7 天", 24.0, 168.0),         // 1d-7d
    (">7 天", 168.0, f64::INFINITY), // >7d
];

pub async fn compute_bypass_duration_buckets_inner(
    pool: &SqlitePool,
    org_id: i64,
    _filter: &AuditFilterInput,
) -> AppResult<Vec<DurationBucket>> {
    // 不复用 build_filter：bypass_record 是独立表，不在 audit_log 维度里。
    // 该函数返回的是**组织级**汇总（覆盖该组织所有项目/旁路），与 audit_log 筛选器语义正交。
    let rows: Vec<(f64,)> = sqlx::query_as(
        "SELECT CAST((julianday(restored_at) - julianday(bypassed_at)) * 24 AS REAL) AS hours
         FROM bypass_record
         WHERE org_id = ? AND restored_at != '' AND bypassed_at != ''",
    )
    .bind(org_id)
    .fetch_all(pool)
    .await?;

    let mut counts = [0i64; 5];
    for (hours,) in rows {
        // hours: f64（Copy）；lo/hi: &f64（要 deref）
        let idx = BYPASS_BUCKETS
            .iter()
            .position(|(_, lo, hi)| hours >= *lo && hours < *hi)
            .unwrap_or(4); // 兜底 >7d
        counts[idx] += 1;
    }

    Ok(BYPASS_BUCKETS
        .iter()
        .enumerate()
        .map(|(i, (label, lo, _))| DurationBucket {
            label: (*label).to_string(),
            count: counts[i],
            lower_hours: *lo,
        })
        .collect())
}

/// SIL 等级变更时间线 —— 用于"SIL/PFDavg 趋势"图
///
/// 审计 `sif_create` / `sif_update` 事件，提取 `sil_verified` 字段的变化。
/// join `sif` 表拿 code。
/// 注：当前 schema 无独立 PFDavg 字段，sil_verified 是 SIL 验算等级（A/B/C/D/NA）的代理指标，
/// 与 PFDavg 量级强相关（A→10⁻⁵, B→10⁻⁴, C→10⁻³, D→10⁻²）；前端图表标注为"SIL 验算等级变化"。
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SilChangeEvent {
    pub ts: String,
    pub sif_id: i64,
    pub sif_code: String,
    pub from_sil: String,
    pub to_sil: String,
    pub action: String,
}

pub async fn compute_sil_change_timeline_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<Vec<SilChangeEvent>> {
    let f = build_filter(filter);
    let sql = format!(
        "SELECT a.id, a.ts, a.target_id, a.action, a.payload_json, s.code
         FROM audit_log a
         LEFT JOIN sif s ON s.id = a.target_id AND s.org_id = a.org_id
         WHERE a.org_id = ?
           AND a.target_table = 'sif' AND a.action IN ('sif_create', 'sif_update'){}
         ORDER BY a.ts ASC",
        f.where_sql
    );
    let mut q =
        sqlx::query_as::<_, (i64, String, i64, String, String, Option<String>)>(&sql).bind(org_id);
    for b in &f.binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await?;

    let mut out: Vec<SilChangeEvent> = Vec::with_capacity(rows.len());
    for (_id, ts, target_id, action, payload_json, sif_code_opt) in rows {
        let sif_code = sif_code_opt.unwrap_or_else(|| format!("SIF-?#{target_id}"));
        let parsed: serde_json::Value =
            serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
        let obj = parsed.as_object().cloned().unwrap_or_default();
        let to_sil = obj
            .get("after")
            .and_then(|v| v.get("silVerified"))
            .and_then(|v| v.as_str())
            .unwrap_or("NA")
            .to_string();
        let from_sil = obj
            .get("before")
            .and_then(|v| v.get("silVerified"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // create 事件的 from 是空，update 时 from==to 也写（"打开关闭"也是工程行为，与 M2.3 一致）
        out.push(SilChangeEvent {
            ts,
            sif_id: target_id,
            sif_code,
            from_sil,
            to_sil,
            action,
        });
    }
    Ok(out)
}

/// 三合一聚合 —— 前端一次拿全
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuditCharts {
    pub daily_activity: Vec<DailyActivity>,
    pub bypass_duration_buckets: Vec<DurationBucket>,
    pub sil_change_timeline: Vec<SilChangeEvent>,
}

pub async fn fetch_audit_charts_inner(
    pool: &SqlitePool,
    org_id: i64,
    filter: &AuditFilterInput,
) -> AppResult<AuditCharts> {
    let daily = compute_daily_activity_inner(pool, org_id, filter).await?;
    let buckets = compute_bypass_duration_buckets_inner(pool, org_id, filter).await?;
    let sil = compute_sil_change_timeline_inner(pool, org_id, filter).await?;
    Ok(AuditCharts {
        daily_activity: daily,
        bypass_duration_buckets: buckets,
        sil_change_timeline: sil,
    })
}

// ===========================================================================
// 6. 单元测试 —— build_filter 行为
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_no_where() {
        let f = AuditFilterInput::default();
        let a = build_filter(&f);
        assert_eq!(a.where_sql, "");
        assert!(a.binds.is_empty());
    }

    #[test]
    fn single_target_table() {
        let f = AuditFilterInput {
            target_table: Some("sif".into()),
            ..Default::default()
        };
        let a = build_filter(&f);
        assert_eq!(a.where_sql, " AND target_table = ?");
        assert_eq!(a.binds, vec!["sif".to_string()]);
    }

    #[test]
    fn full_filter_all_six() {
        let f = AuditFilterInput {
            target_table: Some("instrument".into()),
            action: Some("instrument_update".into()),
            actor: Some("张工".into()),
            ts_from: Some("2026-09-01T00:00:00Z".into()),
            ts_to: Some("2026-09-30T23:59:59Z".into()),
            target_id: Some(42),
        };
        let a = build_filter(&f);
        assert_eq!(
            a.where_sql,
            " AND target_table = ? AND action = ? AND actor = ? \
             AND ts >= ? AND ts <= ? \
             AND target_id = ?"
        );
        assert_eq!(a.binds.len(), 6);
        assert!(a.binds[3].contains("2026-09-01"));
        assert_eq!(a.binds[5], "42");
    }

    #[test]
    fn whitespace_treated_as_none() {
        let f = AuditFilterInput {
            target_table: Some("   ".into()),
            actor: Some("".into()),
            ts_from: Some("\t".into()),
            ..Default::default()
        };
        let a = build_filter(&f);
        assert_eq!(a.where_sql, "");
        assert!(a.binds.is_empty());
    }

    #[test]
    fn empty_string_ts_skipped() {
        // 空字符串不应进入 binds（避免 SQL 误匹配空串）
        let f = AuditFilterInput {
            ts_from: Some("".into()),
            ts_to: Some("".into()),
            ..Default::default()
        };
        let a = build_filter(&f);
        assert_eq!(a.where_sql, "");
        assert!(a.binds.is_empty());
    }
}
