//! 仪表批量导入 —— 文件预览端点（A5）
//!
//! POST /api/imports/preview（multipart/form-data，字段名 `file`）
//! 不落盘：字节流直接交 import::parse_bytes 解析，返回 ParsedSheet。
//! 真正写库仍走 RPC commit_import_instruments（前端把同一份 sheet 回传）。

use axum::extract::{Multipart, State};
use axum::Json;
use serde_json::Value;

use super::rpc::camelify;
use crate::http::auth::AuthUser;
use crate::{AppError, AppResult, AppState};

/// multipart 字段名固定为 file；单文件上限沿用全局 DefaultBodyLimit（10 MiB）。
/// 阶段 B：要求登录（预览本身不碰 DB，但上传能力仅对认证用户开放）。
pub(crate) async fn preview_imports(
    State(state): State<AppState>,
    _auth: AuthUser,
    mut multipart: Multipart,
) -> AppResult<Json<Value>> {
    let _ = &state; // 预览纯解析，不碰 DB；保留 State 以维持路由签名一致

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("multipart: {e}")))?
    {
        if field.name() != Some("file") {
            continue;
        }
        let name = field
            .file_name()
            .unwrap_or("upload")
            .replace(['\\', '/'], "_"); // 防路径痕迹，仅显示用
        let bytes = field
            .bytes()
            .await
            .map_err(|e| AppError::PayloadTooLarge(format!("read upload: {e}")))?;

        let sheet = crate::import::parse_bytes(&name, &bytes)?;
        return Ok(Json(camelify(serde_json::to_value(sheet)?)));
    }

    Err(AppError::BadRequest(
        "missing multipart field 'file'".into(),
    ))
}
