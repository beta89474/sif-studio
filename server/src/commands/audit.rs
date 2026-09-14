//! 审计写入 —— 任何修改 instrument/sif/project 的 command 都应该调用
//!
//! 设计：
//!   - 单一入口 `write_audit(...)`，失败不阻断主操作（写审计失败 ≠ 业务失败）
//!   - actor 默认取环境变量 USERNAME / USER / "unknown"
//!   - payload_json 装"做了什么"+关键字段前后值
//!
//! M2.2 旁路授权台账 / M2.3 仪表字段级 diff / M2.4 SIF+Project 历史 / M2.5+ 即将复用此表。
//!
//! M2.3/M2.4 字段级 diff 协议（统一；M2.2 bypass 历史 payload 暂不改，向后兼容）：
//!   - {verb}_create → {before: null, after: <full row>, fieldsChanged: ["*"]}
//!   - {verb}_update → {before: <full row>, after: <full row>, fieldsChanged: ["fieldA", ...]}
//!   - {verb}_delete → {before: <full row>, after: null, fieldsChanged: ["*"]}
//!   - {verb}_link / _unlink → {before: null|after, after: null|after, fieldsChanged: ["link"|"unlink"], description: "..."}
//!     （不是字段级 diff，是关联操作；前端据此显示「+关联 / −解除」chip + 描述文案）

use crate::AppResult;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::{FromRow, SqlitePool};

/// 写一条审计记录（org_id 由调用方从登录会话派生，禁止前端传入）。
pub async fn write_audit(
    pool: &SqlitePool,
    org_id: i64,
    actor: &str,
    action: &str,
    target_table: &str,
    target_id: Option<i64>,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (org_id, actor, action, target_table, target_id, payload_json)
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(org_id)
    .bind(actor)
    .bind(action)
    .bind(target_table)
    .bind(target_id)
    .bind(payload.to_string())
    .execute(pool)
    .await?;
    Ok(())
}

/// 写审计 + 静默吞错 —— 用于「业务成功 > 审计成功」的路径
pub async fn write_audit_best_effort(
    pool: &SqlitePool,
    org_id: i64,
    actor: &str,
    action: &str,
    target_table: &str,
    target_id: Option<i64>,
    payload: Value,
) {
    if let Err(e) = write_audit(
        pool,
        org_id,
        actor,
        action,
        target_table,
        target_id,
        payload,
    )
    .await
    {
        eprintln!("[audit] write failed ({action} on {target_table}): {e}");
    }
}

/// 字段级 diff —— 比较两个 JSON 对象的顶层字段，返回变了的字段名（驼峰）。
///
/// 设计要点：
///   - 只看 after 的键（create 时 before=null，按此规则返回 after 全键）
///   - 跳过 `id`（update 时 id 不变；create 时 id 必然"变"，但 create 走 ["*"] 路径）
///   - 值相等（`Value::eq`）就不算变 —— 数值 0 vs "0"、Option null vs JSON null 都按 JSON 语义比
///   - 返回值已排序（字典序），便于 UI 稳定展示
///
/// 返回 `["*"]` 时表示"全部字段"（create / delete 用）。
pub fn compute_field_diff(before: &Value, after: &Value) -> Vec<String> {
    let mut changed: Vec<String> = Vec::new();
    let after_obj = match after.as_object() {
        Some(m) => m,
        None => return changed,
    };
    let before_obj = before.as_object();

    for (k, after_v) in after_obj {
        if k == "id" {
            continue; // id 在 update 路径里必然不变
        }
        let differs = match before_obj.and_then(|m| m.get(k)) {
            None => true, // 新增字段（理论上不会出现，防御性）
            Some(before_v) => before_v != after_v,
        };
        if differs {
            changed.push(k.clone());
        }
    }
    changed.sort();
    changed
}

/// 把任意 Serialize-able 值序列化成 JSON Value（驼峰键由原 struct 的 serde 注解保证）。
pub fn snapshot<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// 构造 M2.3/M2.4 标准的 {verb}_* 审计 payload。
///
///   - `before` / `after` 至少一个为 None（None = JSON null）
///   - `fields_changed` 是字段名数组；用 ["*"] 表示 create / delete（全部字段）
///   - `description` 可选，link/unlink 等"非字段级变更"操作用这字段写自然语言描述
pub fn entity_audit_payload(
    before: Option<Value>,
    after: Option<Value>,
    fields_changed: Vec<String>,
) -> Value {
    let mut payload = Map::new();
    payload.insert("before".into(), before.unwrap_or(Value::Null));
    payload.insert("after".into(), after.unwrap_or(Value::Null));
    payload.insert("fieldsChanged".into(), json!(fields_changed));
    Value::Object(payload)
}

/// M2.3 兼容别名 —— instrument 审计 payload 调用此函数即可
pub fn instrument_audit_payload(
    before: Option<Value>,
    after: Option<Value>,
    fields_changed: Vec<String>,
) -> Value {
    entity_audit_payload(before, after, fields_changed)
}

// ---------------------------------------------------------------------------
// M2.4：通用历史查询 helper + AuditEntry 结构
// ---------------------------------------------------------------------------

/// 一条审计记录的"解析版"——把 payload_json 拆成 before / after / fieldsChanged 三个
/// typed 字段，方便 UI 直接 `entry.fieldsChanged` / `entry.before?.tag`。
#[derive(Debug, Serialize, Deserialize, Clone, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub actor: String,
    pub action: String,
    pub target_table: String,
    pub target_id: Option<i64>,
    /// raw payload_json 字符串（前端可保留做兜底展示）
    pub payload_json: String,
    /// 解析后的 before 快照；create 时为 None（JSON null）
    pub before: Option<Value>,
    /// 解析后的 after 快照；delete 时为 None
    pub after: Option<Value>,
    /// 解析后的 fields_changed 数组
    pub fields_changed: Vec<String>,
    /// 解析后的 description 字段（自然语言，可选）；
    /// 通常用于 sif_link / sif_unlink 等"非字段级变更"的描述。
    /// 空字符串表示无 description。
    pub note: String,
}

/// 按 target_table + target_id 反查历史（通用 helper），按 id DESC（秒级精度稳定）。
///
/// - 任何列表（仪表/SIF/Project/未来 diagram/工单）共用
/// - 返回的每条都已把 payload_json 解析成 before/after/fieldsChanged/note 四段
pub async fn list_history_for_target_inner(
    pool: &SqlitePool,
    org_id: i64,
    target_table: &str,
    target_id: i64,
    limit: Option<i64>,
) -> AppResult<Vec<AuditEntry>> {
    let lim = limit.unwrap_or(200).max(1);
    let rows: Vec<(i64, String, String, String, String, Option<i64>, String)> = sqlx::query_as(
        "SELECT id, ts, actor, action, target_table, target_id, payload_json
         FROM audit_log
         WHERE org_id = ? AND target_table = ? AND target_id = ?
         ORDER BY id DESC
         LIMIT ?",
    )
    .bind(org_id)
    .bind(target_table)
    .bind(target_id)
    .bind(lim)
    .fetch_all(pool)
    .await?;

    let mut out: Vec<AuditEntry> = Vec::with_capacity(rows.len());
    for (id, ts, actor, action, target_table, target_id, payload_json) in rows {
        let parsed: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
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

// ---------------------------------------------------------------------------
// 单元测试 —— compute_field_diff
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn diff_returns_changed_field() {
        let before = json!({"id": 1, "tag": "PT-201", "service": "old", "setpoint": 100.0});
        let after = json!({"id": 1, "tag": "PT-201", "service": "new", "setpoint": 100.0});
        let d = compute_field_diff(&before, &after);
        assert_eq!(d, vec!["service".to_string()]);
    }

    #[test]
    fn diff_ignores_id_change() {
        let before = json!({"id": 1, "tag": "PT-201"});
        let after = json!({"id": 999, "tag": "PT-201"});
        assert!(compute_field_diff(&before, &after).is_empty());
    }

    #[test]
    fn diff_handles_null_vs_value() {
        let before = json!({"rangeMin": null, "setpoint": 100.0});
        let after = json!({"rangeMin": 50.0, "setpoint": 100.0});
        let d = compute_field_diff(&before, &after);
        assert_eq!(d, vec!["rangeMin".to_string()]);
    }

    #[test]
    fn diff_returns_sorted_multiple() {
        let before = json!({"a": 1, "b": 2, "c": 3});
        let after = json!({"a": 9, "b": 2, "c": 9});
        let d = compute_field_diff(&before, &after);
        // 按字典序：a, c
        assert_eq!(d, vec!["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn diff_returns_empty_when_equal() {
        let before = json!({"id": 1, "tag": "PT-201", "setpoint": 250.0});
        let after = json!({"id": 1, "tag": "PT-201", "setpoint": 250.0});
        assert!(compute_field_diff(&before, &after).is_empty());
    }

    #[test]
    fn diff_skips_when_after_not_object() {
        let before = json!({});
        let after = json!("not an object");
        assert!(compute_field_diff(&before, &after).is_empty());
    }
}
