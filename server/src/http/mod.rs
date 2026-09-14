//! HTTP 路由层（在线版）
//!
//! - `/api/auth/*`：注册 / 登录 / 登出 / 当前会话（阶段 B）
//! - `/api/admin/*`：整库备份 / 恢复（X-Admin-Token）+ legacy 桌面库导入（owner）
//! - `/api/rpc`：通用 JSON-RPC 风格端点，承接原 Tauri commands（见 rpc.rs）
//! - `/api/imports/preview`：multipart 文件上传解析（阶段 A5）
//! - `/api/audit/export.csv`：CSV 流式下载（阶段 A6）
//! - 其余路径：静态资源（Vite 构建产物 dist/ + public/，SPA fallback 到 index.html）
//! - 全站过安全响应头中间件（阶段 C1：nosniff / frame-options / CSP / …）

pub mod admin;
pub mod audit;
pub mod auth;
pub mod imports;
pub mod org;
pub mod ratelimit;
pub mod rpc;
pub mod security;

use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use std::path::PathBuf;
use tower_http::services::{ServeDir, ServeFile};

use crate::AppState;

/// 请求体上限（10 MiB）：覆盖仪表导入文件上传；既有行数上限保留在业务层。
pub const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

fn env_path(key: &str, default: &str) -> PathBuf {
    PathBuf::from(std::env::var(key).unwrap_or_else(|_| default.to_string()))
}

/// 静态资源：public/（editor.html、favicon）优先匹配，未命中再走 dist/，
/// dist 也未命中时回退 index.html（vue-router 前端路由）——用 fallback 而非
/// not_found_service，后者会把响应状态强制改成 404（SPA 深链应返回 200）。
fn static_service() -> Router {
    let dist_dir = env_path("SIF_WEB_DIST", "../dist");
    let public_dir = env_path("SIF_WEB_PUBLIC", "../public");
    let index = ServeFile::new(dist_dir.join("index.html"));

    let dist = ServeDir::new(&dist_dir).fallback(index);
    let public = ServeDir::new(&public_dir).fallback(dist);

    Router::new().fallback_service(public)
}

pub fn router(state: AppState) -> Router {
    // 受保护端点：handler 内用 AuthUser 提取器，未登录统一 401 {kind,message}
    let api = Router::new()
        .route("/rpc", post(rpc::rpc_handler))
        .route("/imports/preview", post(imports::preview_imports))
        .route("/audit/export.csv", get(audit::export_audit_csv))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state.clone());

    // 管理端点：上传上限 200 MiB（图纸 JSON 快照可能很大）；鉴权在 handler 内
    // （backup/restore 查 X-Admin-Token，import-legacy 查 owner 会话）。
    let admin_api = Router::new()
        .route("/admin/backup", get(admin::backup))
        .route("/admin/restore", post(admin::restore))
        .route("/admin/import-legacy", post(admin::import_legacy))
        .layer(DefaultBodyLimit::max(admin::ADMIN_BODY_LIMIT))
        .with_state(state.clone());

    // 组织成员 / 邀请：org_id 全部来自会话或邀请记录（D1）
    let org_api = Router::new()
        .route(
            "/org/invites",
            get(org::list_invites).post(org::create_invite),
        )
        // 注意：accept 不能放在 /org/invites/accept（与 :token 参数段冲突），
        // 用独立动作路径。
        .route("/org/invite/accept", post(org::accept_invite))
        .route(
            "/org/invites/{token}",
            axum::routing::delete(org::revoke_invite),
        )
        .route("/org/members", get(org::list_members))
        .route("/org/members/{user_id}/role", post(org::set_member_role))
        .route(
            "/org/members/{user_id}/reset-password",
            post(org::reset_member_password),
        )
        .route(
            "/org/members/{user_id}",
            axum::routing::delete(org::remove_member),
        )
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state.clone());

    // 公开端点：认证 + 健康检查 + 邀请预览（注册页用）
    let pub_api = Router::new()
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        // E3 自助账号（登录态，鉴权在 AuthUser 提取器）
        .route("/auth/change-password", post(auth::change_password))
        .route("/auth/profile", post(auth::update_profile))
        // E4 会话管理 / 组织切换
        .route("/auth/sessions", get(auth::list_sessions))
        .route(
            "/auth/sessions/logout-others",
            post(auth::logout_other_sessions),
        )
        .route("/auth/switch-org", post(auth::switch_org))
        .route("/auth/invite", get(org::invite_preview))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(state.clone());

    Router::new()
        .nest("/api", pub_api.merge(api).merge(admin_api).merge(org_api))
        .fallback_service(static_service())
        // 安全头对 /api 与静态资源统一生效（最外层）。
        .layer(middleware::from_fn(security::security_headers))
        .with_state(state)
}
