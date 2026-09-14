//! 阶段 C2/C3 —— 管理端点：整库备份 / 恢复 / legacy 桌面库导入
//!
//! 路由（均挂在 /api/admin 下，另有 200 MiB 专用 body 上限）：
//! - GET  /api/admin/backup        下载整库快照（**含全部组织**）
//! - POST /api/admin/restore       上传备份整库恢复
//! - POST /api/admin/import-legacy 组织 owner 导入旧桌面库（仅本组织）
//!
//! backup/restore 是服务器操作员能力（数据跨越所有租户），用静态
//! `X-Admin-Token` 头 + 环境变量 SIF_ADMIN_TOKEN 守护；未配置令牌时这两个
//! 端点直接 404（对外不暴露存在性）。
//! import-legacy 是组织内能力，走普通会话，且仅 owner 可调用。

use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;

use crate::backup::{backup_filename, backup_to_file, import_legacy_db, restore_from_file};
use crate::http::auth::AuthUser;
use crate::{AppError, AppResult, AppState};

/// 管理令牌头名。
const ADMIN_TOKEN_HEADER: &str = "x-admin-token";

/// 管理端点上传上限：200 MiB（旧桌面库可能含大量图纸 JSON 快照）。
pub const ADMIN_BODY_LIMIT: usize = 200 * 1024 * 1024;

/// 校验操作员令牌。未配置 → 404；配置了但不对 → 401。
fn require_admin(state: &AppState, headers: &HeaderMap) -> AppResult<()> {
    let configured = state.config.admin_token.is_some();
    let provided = headers
        .get(ADMIN_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !configured {
        return Err(AppError::NotFound(
            "管理端点未启用（服务器未配置 SIF_ADMIN_TOKEN）".into(),
        ));
    }
    if !state.config.check_admin_token(provided) {
        return Err(AppError::Unauthorized("管理员令牌无效".into()));
    }
    Ok(())
}

/// GET /api/admin/backup → application/vnd.sqlite3 附件流。
pub(crate) async fn backup(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Response> {
    require_admin(&state, &headers)?;

    // VACUUM INTO 要求目标文件不存在；tempdir 随 handler 结束自动清理。
    let dir = tempfile::tempdir()?;
    let dest = dir.path().join("sif-backup.db");
    backup_to_file(state.db.as_ref(), &dest).await?;
    let bytes = tokio::fs::read(&dest).await?;

    let disposition = format!("attachment; filename=\"{}\"", backup_filename());
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/vnd.sqlite3")
        .header(CONTENT_DISPOSITION, disposition)
        .header(CACHE_CONTROL, "no-store")
        .body(Body::from(bytes))
        .map_err(|e| AppError::BadRequest(format!("构建备份响应失败: {e}")))
}

/// POST /api/admin/restore（multipart 字段 file）→ 恢复报告 JSON。
pub(crate) async fn restore(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> AppResult<Json<serde_json::Value>> {
    require_admin(&state, &headers)?;

    let bytes = read_file_field(&mut multipart).await?;
    let dir = tempfile::tempdir()?;
    let upload = dir.path().join("restore.db");
    tokio::fs::write(&upload, &bytes).await?;

    let expected = crate::backup::latest_migration_version(state.db.as_ref()).await?;
    let report = restore_from_file(state.db.as_ref(), &upload, expected).await?;
    Ok(Json(serde_json::to_value(report)?))
}

/// POST /api/admin/import-legacy（multipart 字段 file）→ 导入计数 JSON。
/// 组织内能力：仅 owner；数据只落到自己组织。
pub(crate) async fn import_legacy(
    State(state): State<AppState>,
    auth: AuthUser,
    mut multipart: Multipart,
) -> AppResult<Json<serde_json::Value>> {
    if auth.role != "owner" {
        return Err(AppError::Forbidden("仅组织 owner 可导入旧版桌面库".into()));
    }

    let bytes = read_file_field(&mut multipart).await?;
    let dir = tempfile::tempdir()?;
    let upload = dir.path().join("legacy.db");
    tokio::fs::write(&upload, &bytes).await?;

    let report = import_legacy_db(state.db.as_ref(), auth.org_id, auth.actor(), &upload).await?;
    Ok(Json(serde_json::to_value(report)?))
}

/// 从 multipart 里取唯一文件字段 `file` 的全部字节。
async fn read_file_field(multipart: &mut Multipart) -> AppResult<Vec<u8>> {
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        return field
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| AppError::PayloadTooLarge(format!("读取上传文件失败: {e}")));
    }
    Err(AppError::BadRequest("缺少 multipart 字段 `file`".into()))
}
