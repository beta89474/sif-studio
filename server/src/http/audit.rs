//! 审计 CSV 流式下载（A6）
//!
//! GET /api/audit/export.csv?targetTable=&action=&actor=&tsFrom=&tsTo=&targetId=
//! 查询参数即 AuditFilterInput 的 camelCase 版（空字符串等价于不筛选）。
//!
//! 响应：text/csv; charset=utf-8 + BOM，Content-Disposition 附件下载。
//! 错误仍走 AppError → {kind,message}（Query 反序列化失败 = 422）。

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::Response;
use serde::Deserialize;

use crate::commands::audit_export::{build_audit_csv_bytes, AuditFilterInput};
use crate::http::auth::AuthUser;
use crate::{AppError, AppResult, AppState};

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportQuery {
    #[serde(default)]
    pub target_table: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub ts_from: Option<String>,
    #[serde(default)]
    pub ts_to: Option<String>,
    #[serde(default)]
    pub target_id: Option<i64>,
}

impl ExportQuery {
    /// 空字符串视为未筛选（与前端"全部"下拉提交空串的历史行为对齐）。
    fn blank_to_none(v: Option<String>) -> Option<String> {
        v.filter(|s| !s.trim().is_empty())
    }

    fn into_filter(self) -> AuditFilterInput {
        AuditFilterInput {
            target_table: Self::blank_to_none(self.target_table),
            action: Self::blank_to_none(self.action),
            actor: Self::blank_to_none(self.actor),
            ts_from: Self::blank_to_none(self.ts_from),
            ts_to: Self::blank_to_none(self.ts_to),
            target_id: self.target_id,
        }
    }
}

pub(crate) async fn export_audit_csv(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ExportQuery>,
) -> AppResult<Response> {
    // F3：审计包含字段级 diff 快照，属组织敏感数据外流面。viewer 可在线查看，
    // 但全量 CSV 导出（含 BOM、可离线流转）仅限 engineer / owner。
    if auth.role == "viewer" {
        crate::commands::audit::write_audit_best_effort(
            state.db.as_ref(),
            auth.org_id,
            auth.actor(),
            "authz.viewer_audit_export_denied",
            "audit_log",
            None,
            serde_json::json!({ "endpoint": "GET /api/audit/export.csv" }),
        )
        .await;
        return Err(AppError::Forbidden(
            "只读角色不能导出审计包，如需导出请联系工程师或所有者".into(),
        ));
    }

    let filter = q.into_filter();
    let (bytes, _rows, truncated) =
        build_audit_csv_bytes(state.db.as_ref(), auth.org_id, &filter).await?;

    // 文件名带时间戳，浏览器另存友好；ASCII 回退避免头里出现裸中文。
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let disposition = format!("attachment; filename=\"audit-{stamp}.csv\"");

    match Response::builder()
        .header(CONTENT_TYPE, "text/csv; charset=utf-8")
        .header(CONTENT_DISPOSITION, disposition)
        .header(
            "x-audit-truncated",
            if truncated { "true" } else { "false" },
        )
        .body(Body::from(bytes))
    {
        Ok(resp) => Ok(resp),
        Err(e) => Err(AppError::Io(std::io::Error::other(e.to_string()))),
    }
}
