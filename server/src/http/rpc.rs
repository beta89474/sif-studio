//! 通用 RPC 分发器 —— 在线版对原 Tauri IPC 命令的承接层
//!
//! 协议：
//!   POST /api/rpc  { "cmd": "<command_name>", "args": { ...camelCase... } }
//!   - 命令名沿用 Tauri 时代的 snake_case 名
//!   - 标量参数名沿用前端 camelCase；嵌套 input 结构复用各业务模块的输入类型
//!   - 响应 JSON 键统一 snake_case → camelCase
//!   - 错误体统一为 {kind, message}
//!
//! 阶段 B：本端点要求登录（AuthUser 提取器，失败 401）。
//! org_id 一律取自会话；actor 为登录用户 displayName/email。
//!
//! 例外（文件语义，走专用 HTTP 端点）：
//!   - preview_import_instruments → POST /api/imports/preview（multipart）
//!   - export_audit_csv           → GET  /api/audit/export.csv（流式下载）

use axum::extract::State;
use axum::Json;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use crate::commands::{
    audit_export, bypasses, diagrams, imports, instruments, meta, projects, sifs, tags,
};
use crate::http::auth::AuthUser;
use crate::{AppError, AppResult, AppState};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RpcRequest {
    cmd: String,
    #[serde(default)]
    args: Value,
}

// ---------------------------------------------------------------------------
// 参数提取 helper
// ---------------------------------------------------------------------------

fn required<T: DeserializeOwned>(args: &Value, key: &str) -> AppResult<T> {
    match args.get(key) {
        None | Some(Value::Null) => Err(AppError::Validation(format!("missing arg: {key}"))),
        Some(v) => Ok(serde_json::from_value(v.clone())?),
    }
}

fn optional<T: DeserializeOwned>(args: &Value, key: &str) -> AppResult<Option<T>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(serde_json::from_value(v.clone())?)),
    }
}

/// 嵌套 input 结构（create/update/commit 类命令）
fn input_of<T: DeserializeOwned>(args: &Value) -> AppResult<T> {
    required(args, "input")
}

// ---------------------------------------------------------------------------
// 响应键 camelify（递归；已是 camelCase 的键不含 '_'，天然幂等）
// ---------------------------------------------------------------------------

pub(crate) fn camelify(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::with_capacity(map.len());
            for (k, v) in map {
                out.insert(snake_to_camel(&k), camelify(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.into_iter().map(camelify).collect()),
        other => other,
    }
}

fn snake_to_camel(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut upper_next = false;
    for ch in key.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 分发器
// ---------------------------------------------------------------------------

/// E1：viewer（只读角色）禁止执行的写命令。
/// 名单显式维护而非按前缀推断，避免新增读命令（如 import_tags_csv 式的命名）
/// 时被误判；新增写命令时必须同步加入本表。
const WRITE_CMDS: &[&str] = &[
    // instruments / projects / diagrams
    "create_instrument",
    "update_instrument",
    "delete_instrument",
    "create_project",
    "update_project",
    "delete_project",
    "ensure_default_project",
    "create_diagram",
    "ensure_default_diagram",
    "save_diagram_data",
    // sifs + 链接
    "create_sif",
    "update_sif",
    "delete_sif",
    "link_instrument_to_sif",
    "unlink_instrument_from_sif",
    // 旁路
    "create_bypass",
    "update_bypass",
    "restore_bypass",
    "delete_bypass",
    // 批量导入（会落库）
    "import_tags_csv",
    "commit_import_instruments",
];

#[allow(clippy::too_many_lines)]
pub(crate) async fn rpc_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RpcRequest>,
) -> AppResult<Json<Value>> {
    let pool = state.db.as_ref();
    let args = &req.args;
    let org_id = auth.org_id;
    let actor = auth.actor();

    // E1：垂直越权修复——viewer 只可读，写命令一律 403。
    // 前端虽会隐藏写入口，但服务端才是真正的权限边界（旧版只查登录态）。
    if auth.role == "viewer" && WRITE_CMDS.contains(&req.cmd.as_str()) {
        crate::commands::audit::write_audit_best_effort(
            pool,
            org_id,
            actor,
            "authz.viewer_write_denied",
            "rpc",
            None,
            serde_json::json!({ "cmd": req.cmd }),
        )
        .await;
        return Err(AppError::Forbidden(format!(
            "只读角色（viewer）无权执行写操作：{}",
            req.cmd
        )));
    }

    let result = match req.cmd.as_str() {
        // ---- meta ----
        "db_version" => serde_json::to_value(meta::db_version_inner(pool).await?)?,

        // ---- instruments ----
        "list_instruments" => {
            serde_json::to_value(instruments::list_instruments_inner(pool, org_id).await?)?
        }
        "list_instruments_by_project" => serde_json::to_value(
            instruments::list_instruments_by_project_inner(
                pool,
                org_id,
                required(args, "projectId")?,
            )
            .await?,
        )?,
        "create_instrument" => serde_json::to_value(
            instruments::create_instrument_inner(pool, org_id, actor, input_of(args)?).await?,
        )?,
        "get_instrument" => serde_json::to_value(
            instruments::get_instrument_inner(pool, org_id, required(args, "id")?).await?,
        )?,
        "update_instrument" => serde_json::to_value(
            instruments::update_instrument_inner(
                pool,
                org_id,
                actor,
                required(args, "id")?,
                input_of(args)?,
            )
            .await?,
        )?,
        "delete_instrument" => serde_json::to_value(
            instruments::delete_instrument_inner(pool, org_id, actor, required(args, "id")?)
                .await?,
        )?,
        "count_instruments" => {
            serde_json::to_value(instruments::count_instruments_inner(pool, org_id).await?)?
        }
        "count_instruments_by_project" => serde_json::to_value(
            instruments::count_instruments_by_project_inner(
                pool,
                org_id,
                required(args, "projectId")?,
            )
            .await?,
        )?,
        "list_instrument_history" => serde_json::to_value(
            instruments::list_instrument_history_inner(
                pool,
                org_id,
                required(args, "instrumentId")?,
                optional(args, "limit")?,
            )
            .await?,
        )?,

        // ---- projects ----
        "list_projects" => {
            serde_json::to_value(projects::list_projects_inner(pool, org_id).await?)?
        }
        "get_project" => serde_json::to_value(
            projects::get_project_inner(pool, org_id, required(args, "id")?).await?,
        )?,
        "create_project" => serde_json::to_value(
            projects::create_project_inner(pool, org_id, &input_of(args)?, actor).await?,
        )?,
        "update_project" => serde_json::to_value(
            projects::update_project_inner(
                pool,
                org_id,
                required(args, "id")?,
                &input_of(args)?,
                actor,
            )
            .await?,
        )?,
        "delete_project" => serde_json::to_value(
            projects::delete_project_inner(pool, org_id, required(args, "id")?, actor).await?,
        )?,
        "list_project_history" => serde_json::to_value(
            projects::list_project_history_inner(
                pool,
                org_id,
                required(args, "projectId")?,
                optional(args, "limit")?,
            )
            .await?,
        )?,
        "ensure_default_project" => {
            serde_json::to_value(projects::ensure_default_project_inner(pool, org_id).await?)?
        }

        // ---- diagrams ----
        "list_diagrams" => serde_json::to_value(
            diagrams::list_diagrams_inner(pool, org_id, optional(args, "projectId")?).await?,
        )?,
        "get_diagram" => serde_json::to_value(
            diagrams::get_diagram_inner(pool, org_id, required(args, "id")?).await?,
        )?,
        "create_diagram" => serde_json::to_value(
            diagrams::create_diagram_inner(pool, org_id, &input_of(args)?).await?,
        )?,
        "ensure_default_diagram" => serde_json::to_value(
            diagrams::ensure_default_diagram_inner(pool, org_id, required(args, "projectId")?)
                .await?,
        )?,
        "save_diagram_data" => serde_json::to_value(
            diagrams::save_diagram_data_inner(
                pool,
                org_id,
                required(args, "id")?,
                required(args, "version")?,
                &required::<String>(args, "data")?,
            )
            .await?,
        )?,

        // ---- sifs ----
        "list_sifs" => serde_json::to_value(
            sifs::list_sifs_inner(pool, org_id, optional(args, "projectId")?).await?,
        )?,
        "create_sif" => serde_json::to_value(
            sifs::create_sif_inner(pool, org_id, &input_of(args)?, actor).await?,
        )?,
        "update_sif" => serde_json::to_value(
            sifs::update_sif_inner(pool, org_id, required(args, "id")?, &input_of(args)?, actor)
                .await?,
        )?,
        "delete_sif" => serde_json::to_value(
            sifs::delete_sif_inner(pool, org_id, required(args, "id")?, actor).await?,
        )?,
        "get_sif" => {
            serde_json::to_value(sifs::get_sif_inner(pool, org_id, required(args, "id")?).await?)?
        }
        "list_sif_links" => serde_json::to_value(
            sifs::list_sif_links_inner(pool, org_id, required(args, "sifId")?).await?,
        )?,
        "link_instrument_to_sif" => serde_json::to_value(
            sifs::link_instrument_to_sif_inner(
                pool,
                org_id,
                required(args, "sifId")?,
                required(args, "instrumentId")?,
                required(args, "role")?,
                required(args, "portIndex")?,
                optional(args, "diagramId")?,
                required(args, "note")?,
                actor,
            )
            .await?,
        )?,
        "unlink_instrument_from_sif" => serde_json::to_value(
            sifs::unlink_instrument_from_sif_inner(pool, org_id, required(args, "linkId")?, actor)
                .await?,
        )?,
        "list_sif_history" => serde_json::to_value(
            sifs::list_sif_history_inner(
                pool,
                org_id,
                required(args, "sifId")?,
                optional(args, "limit")?,
            )
            .await?,
        )?,

        // ---- bypasses ----
        "list_bypasses" => serde_json::to_value(
            bypasses::list_bypasses_inner(
                pool,
                org_id,
                optional(args, "projectId")?,
                optional::<String>(args, "status")?.as_deref(),
            )
            .await?,
        )?,
        "create_bypass" => serde_json::to_value(
            bypasses::create_bypass_inner(pool, org_id, actor, input_of(args)?).await?,
        )?,
        "update_bypass" => serde_json::to_value(
            bypasses::update_bypass_inner(
                pool,
                org_id,
                actor,
                required(args, "id")?,
                input_of(args)?,
            )
            .await?,
        )?,
        "restore_bypass" => {
            let restored: Option<String> = optional(args, "restoredAt")?;
            serde_json::to_value(
                bypasses::restore_bypass_inner(
                    pool,
                    org_id,
                    actor,
                    required(args, "id")?,
                    restored.as_deref(),
                )
                .await?,
            )?
        }
        "delete_bypass" => serde_json::to_value(
            bypasses::delete_bypass_inner(pool, org_id, actor, required(args, "id")?).await?,
        )?,
        "count_overdue_bypasses" => {
            serde_json::to_value(bypasses::count_overdue_bypasses_inner(pool, org_id).await?)?
        }

        // ---- tags（M1.5 位号批量导入，保留可用）----
        "import_tags_csv" => {
            serde_json::to_value(tags::import_tags_csv_inner(pool, org_id, input_of(args)?).await?)?
        }

        // ---- M2.1 仪表批量导入（文件预览走 /api/imports/preview）----
        "commit_import_instruments" => serde_json::to_value(
            imports::commit_import_instruments_inner(pool, org_id, actor, input_of(args)?).await?,
        )?,

        // ---- M2.5/M2.7 审计查询与图表（CSV 导出走 /api/audit/export.csv）----
        "list_audit_filters" => {
            serde_json::to_value(audit_export::list_audit_filters_inner(pool, org_id).await?)?
        }
        "count_audit_filtered" => serde_json::to_value(
            audit_export::count_audit_filtered_inner(pool, org_id, &required(args, "filter")?)
                .await?,
        )?,
        "summarize_audit_filtered" => serde_json::to_value(
            audit_export::summarize_audit_filtered_inner(pool, org_id, &required(args, "filter")?)
                .await?,
        )?,
        "preview_audit_filtered" => serde_json::to_value(
            audit_export::preview_audit_filtered_inner(
                pool,
                org_id,
                &required(args, "filter")?,
                optional(args, "limit")?,
            )
            .await?,
        )?,
        "compute_daily_activity" => serde_json::to_value(
            audit_export::compute_daily_activity_inner(pool, org_id, &required(args, "filter")?)
                .await?,
        )?,
        "compute_bypass_duration_buckets" => serde_json::to_value(
            audit_export::compute_bypass_duration_buckets_inner(
                pool,
                org_id,
                &required(args, "filter")?,
            )
            .await?,
        )?,
        "compute_sil_change_timeline" => serde_json::to_value(
            audit_export::compute_sil_change_timeline_inner(
                pool,
                org_id,
                &required(args, "filter")?,
            )
            .await?,
        )?,
        "fetch_audit_charts" => serde_json::to_value(
            audit_export::fetch_audit_charts_inner(pool, org_id, &required(args, "filter")?)
                .await?,
        )?,

        other => {
            return Err(AppError::NotFound(format!("unknown rpc command: {other}")));
        }
    };

    Ok(Json(camelify(result)))
}
