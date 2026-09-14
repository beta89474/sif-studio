//! 认证与会话（阶段 B）
//!
//! 设计：
//!   - 不透明随机会话 token（32 字节 hex）存服务端 `session` 表，
//!     Cookie 只持 token；HttpOnly + SameSite=Lax（同源 POST 天然带 Cookie）。
//!   - 密码用 argon2id PHC 哈希；不自己造加密。
//!   - 多租户铁律：org_id 一律从会话派生，任何业务请求都不接受前端传 org。
//!   - 首个注册用户认领迁移 004 建的默认组织（id=1，承接存量库数据）；
//!     之后的注册自动开新组织，注册人即 owner。

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use axum::extract::{FromRequestParts, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

use crate::http::ratelimit::LimitKind;
use crate::{AppError, AppResult, AppState};

pub const SESSION_COOKIE: &str = "sif_session";
// 会话有效期由 AppConfig.session_ttl_days 驱动（SIF_SESSION_TTL_DAYS，默认 30 天），
// session 表 expires_at 与登录 cookie 的 Max-Age 共用同一口径。

// ---------------------------------------------------------------------------
// AuthUser 提取器 —— 所有受保护 handler 用它拿当前用户与组织
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub org_id: i64,
    pub email: String,
    pub display_name: String,
    /// owner | engineer | viewer（当前会话所在组织里的角色）
    pub role: String,
}

impl AuthUser {
    /// 审计 actor 用显示名（为空时回退邮箱）
    pub fn actor(&self) -> &str {
        if self.display_name.is_empty() {
            &self.email
        } else {
            &self.display_name
        }
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_session_cookie)
            .ok_or_else(|| AppError::Unauthorized("未登录或会话已失效".into()))?;

        let row = sqlx::query_as::<_, SessionRow>(
            "SELECT s.user_id AS user_id,
                    s.org_id  AS org_id,
                    u.email   AS email,
                    u.display_name AS display_name,
                    m.role    AS role
               FROM session s
               JOIN user u        ON u.id = s.user_id
               JOIN org_member m  ON m.user_id = s.user_id AND m.org_id = s.org_id
              WHERE s.token = ?1
                AND s.expires_at > datetime('now')
              LIMIT 1",
        )
        .bind(&token)
        .fetch_optional(state.db.as_ref())
        .await?
        .ok_or_else(|| AppError::Unauthorized("会话不存在或已过期".into()))?;

        // 最近访问时间：失败不阻断请求
        let _ = sqlx::query("UPDATE session SET last_seen_at = datetime('now') WHERE token = ?1")
            .bind(&token)
            .execute(state.db.as_ref())
            .await;

        Ok(AuthUser {
            user_id: row.user_id,
            org_id: row.org_id,
            email: row.email,
            display_name: row.display_name,
            role: row.role,
        })
    }
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    user_id: i64,
    org_id: i64,
    email: String,
    display_name: String,
    role: String,
}

// ---------------------------------------------------------------------------
// 请求 / 响应体
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterReq {
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub display_name: String,
    /// 新组织名；首个用户认领默认组织时用于改名（携带邀请时忽略）
    #[serde(default)]
    pub org_name: String,
    /// 邀请 token：携带则加入指定组织，不再开新组织（D1）
    #[serde(default)]
    pub invite_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrgMembership {
    pub org_id: i64,
    pub org_name: String,
    pub role: String,
    /// 是否为当前会话所在组织
    pub current: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeResp {
    pub user_id: i64,
    pub email: String,
    pub display_name: String,
    pub org_id: i64,
    pub org_name: String,
    pub role: String,
    /// 用户加入的全部组织（用于切换器；当前组织标 current=true）
    pub orgs: Vec<OrgMembership>,
    /// F2：管理员重置过密码，下次进入必须先修改自己的密码
    pub must_change_password: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordReq {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReq {
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchOrgReq {
    pub org_id: i64,
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

pub async fn me(auth: AuthUser, State(state): State<AppState>) -> AppResult<Json<MeResp>> {
    Ok(Json(
        load_me_body(state.db.as_ref(), auth.user_id, auth.org_id).await?,
    ))
}

pub async fn register(
    State(state): State<AppState>,
    parts: Parts,
    Json(req): Json<RegisterReq>,
) -> AppResult<Response> {
    let ip = client_ip(&parts, &state.config);
    let ua = user_agent(&parts);

    // 防批量开号：每 IP 每小时 10 次
    if !state.rate.check(LimitKind::Register, &ip, "") {
        return Err(AppError::TooManyRequests(
            "注册尝试过于频繁，请稍后再试".into(),
        ));
    }

    let email = normalize_email(&req.email)?;
    validate_password(&req.password)?;
    let display_name = req.display_name.trim().to_string();
    let org_name = if req.org_name.trim().is_empty() {
        format!("{} 的组织", display_name_or_email(&display_name, &email))
    } else {
        req.org_name.trim().to_string()
    };

    // 邀请 token 先解析（无效/过期直接 404）；邀请制开关据此判断
    let invite = match req
        .invite_token
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(token) => Some(crate::http::org::load_active_invite(&state, token).await?),
        None => None,
    };
    if state.config.invite_only && invite.is_none() {
        // 空库引导：系统中一个用户都没有时（全新部署），允许首个注册者落为
        // owner 认领默认组织；之后公开注册即关闭，只能走邀请。
        let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user")
            .fetch_one(state.db.as_ref())
            .await?;
        if total_users > 0 {
            return Err(AppError::Forbidden(
                "本系统已关闭公开注册，请使用组织邀请链接注册".into(),
            ));
        }
    }

    // 邮箱全局唯一
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user WHERE lower(email) = ?1")
        .bind(&email)
        .fetch_one(state.db.as_ref())
        .await?;
    if exists > 0 {
        return Err(AppError::Conflict("该邮箱已注册，请直接登录".into()));
    }

    let hash = hash_password(&req.password)?;
    let pool = state.db.as_ref();

    let user_id =
        sqlx::query("INSERT INTO user (email, password_hash, display_name) VALUES (?1, ?2, ?3)")
            .bind(&email)
            .bind(&hash)
            .bind(&display_name)
            .execute(pool)
            .await?
            .last_insert_rowid();

    let (user_id, org_id) = if let Some((inv_org_id, inv_role, _)) = invite {
        sqlx::query("INSERT INTO org_member (org_id, user_id, role) VALUES (?1, ?2, ?3)")
            .bind(inv_org_id)
            .bind(user_id)
            .bind(&inv_role)
            .execute(pool)
            .await?;
        crate::commands::audit::write_audit_best_effort(
            pool,
            inv_org_id,
            if display_name.is_empty() {
                &email
            } else {
                &display_name
            },
            "org.member_join",
            "org_member",
            Some(user_id),
            serde_json::json!({ "via": "register_invite" }),
        )
        .await;
        (user_id, inv_org_id)
    } else {
        // 首个注册用户认领 004 建的默认组织（可能装着存量桌面库数据）；之后的开新组织。
        let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user WHERE id <> ?1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
        if user_count == 0 {
            sqlx::query("UPDATE org SET name = ?1 WHERE id = 1")
                .bind(&org_name)
                .execute(pool)
                .await?;
            sqlx::query("INSERT INTO org_member (org_id, user_id, role) VALUES (1, ?1, 'owner')")
                .bind(user_id)
                .execute(pool)
                .await?;
            (user_id, 1_i64)
        } else {
            let org_id = sqlx::query("INSERT INTO org (name) VALUES (?1)")
                .bind(&org_name)
                .execute(pool)
                .await?
                .last_insert_rowid();
            sqlx::query("INSERT INTO org_member (org_id, user_id, role) VALUES (?1, ?2, 'owner')")
                .bind(org_id)
                .bind(user_id)
                .execute(pool)
                .await?;
            (user_id, org_id)
        }
    };

    let ttl = state.config.session_ttl_seconds();
    let token = issue_session(pool, ttl, user_id, org_id, &ua, &ip).await?;
    let resp_body = load_me_body(pool, user_id, org_id).await?;
    Ok(session_response(&token, ttl, Json(resp_body)).into_response())
}

pub async fn login(
    State(state): State<AppState>,
    parts: Parts,
    Json(req): Json<LoginReq>,
) -> AppResult<Response> {
    let email = normalize_email(&req.email)?;
    let ip = client_ip(&parts, &state.config);
    let pool = state.db.as_ref();

    // 防爆破：同一 IP+邮箱 15 分钟最多 10 次尝试（成功后清零）。
    if !state.rate.check(LimitKind::Login, &ip, &email) {
        return Err(AppError::TooManyRequests(
            "登录尝试过于频繁，请 15 分钟后再试".into(),
        ));
    }

    let row = sqlx::query_as::<_, CredRow>(
        "SELECT id AS user_id, password_hash AS password_hash
           FROM user WHERE lower(email) = ?1",
    )
    .bind(&email)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Unauthorized("邮箱或密码不正确".into()))?;

    let parsed = PasswordHash::new(&row.password_hash)
        .map_err(|_| AppError::Unauthorized("邮箱或密码不正确".into()))?;
    Argon2::default()
        .verify_password(req.password.as_bytes(), &parsed)
        .map_err(|_| AppError::Unauthorized("邮箱或密码不正确".into()))?;

    // 登录成功：清尝试计数
    state.rate.reset_login(&ip, &email);

    // 机会式清理：顺手删掉全部过期会话，避免 session 表无限累积
    // （退出/改密/重置只删活跃会话，自然过期无人回收）。
    sqlx::query("DELETE FROM session WHERE expires_at <= datetime('now')")
        .execute(pool)
        .await?;

    // 取该用户的一个组织：优先 owner，其次最早加入的（B 期不做组织切换器）
    let org_id: i64 = sqlx::query_scalar(
        "SELECT org_id FROM org_member
          WHERE user_id = ?1
          ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'engineer' THEN 1 ELSE 2 END,
                   created_at ASC, org_id ASC
          LIMIT 1",
    )
    .bind(row.user_id)
    .fetch_one(pool)
    .await?;

    let ttl = state.config.session_ttl_seconds();
    let token = issue_session(pool, ttl, row.user_id, org_id, &user_agent(&parts), &ip).await?;
    let resp_body = load_me_body(pool, row.user_id, org_id).await?;
    Ok(session_response(&token, ttl, Json(resp_body)).into_response())
}

pub async fn logout(State(state): State<AppState>, parts: Parts) -> AppResult<Response> {
    if let Some(token) = parts
        .headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_session_cookie)
    {
        sqlx::query("DELETE FROM session WHERE token = ?1")
            .bind(&token)
            .execute(state.db.as_ref())
            .await?;
    }
    let mut resp = Json(serde_json::json!({"ok": true})).into_response();
    resp.headers_mut().insert(
        SET_COOKIE,
        clear_cookie()
            .parse()
            .map_err(|_| AppError::BadRequest("bad cookie header".into()))?,
    );
    Ok(resp)
}

#[derive(sqlx::FromRow)]
struct CredRow {
    user_id: i64,
    password_hash: String,
}

async fn load_me_body(pool: &sqlx::SqlitePool, user_id: i64, org_id: i64) -> AppResult<MeResp> {
    let row = sqlx::query_as::<_, MeRow>(
        "SELECT u.id AS user_id, u.email AS email, u.display_name AS display_name,
                m.org_id AS org_id, o.name AS org_name, m.role AS role,
                u.must_change_password AS must_change_password
           FROM user u
           JOIN org_member m ON m.user_id = u.id
           JOIN org o        ON o.id = m.org_id
          WHERE u.id = ?1 AND m.org_id = ?2",
    )
    .bind(user_id)
    .bind(org_id)
    .fetch_one(pool)
    .await?;

    let orgs = sqlx::query_as::<_, OrgRow>(
        "SELECT m.org_id AS org_id, o.name AS org_name, m.role AS role
           FROM org_member m JOIN org o ON o.id = m.org_id
          WHERE m.user_id = ?1
          ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'engineer' THEN 1 ELSE 2 END,
                   m.created_at ASC, m.org_id ASC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| OrgMembership {
        org_id: r.org_id,
        org_name: r.org_name,
        role: r.role,
        current: r.org_id == org_id,
    })
    .collect();

    Ok(MeResp {
        user_id: row.user_id,
        email: row.email,
        display_name: row.display_name,
        org_id: row.org_id,
        org_name: row.org_name,
        role: row.role,
        orgs,
        must_change_password: row.must_change_password != 0,
    })
}

#[derive(sqlx::FromRow)]
struct MeRow {
    user_id: i64,
    email: String,
    display_name: String,
    org_id: i64,
    org_name: String,
    role: String,
    must_change_password: i64,
}

#[derive(sqlx::FromRow)]
struct OrgRow {
    org_id: i64,
    org_name: String,
    role: String,
}

// ---------------------------------------------------------------------------
// E3 —— 自助账号管理
// ---------------------------------------------------------------------------

/// POST /api/auth/change-password
pub async fn change_password(
    State(state): State<AppState>,
    parts: Parts,
    auth: AuthUser,
    Json(req): Json<ChangePasswordReq>,
) -> AppResult<Json<serde_json::Value>> {
    let pool = state.db.as_ref();
    let stored: String = sqlx::query_scalar("SELECT password_hash FROM user WHERE id = ?1")
        .bind(auth.user_id)
        .fetch_one(pool)
        .await?;
    let parsed = PasswordHash::new(&stored)
        .map_err(|e| AppError::Unauthorized(format!("账号密码数据异常，请联系管理员: {e}")))?;
    Argon2::default()
        .verify_password(req.old_password.as_bytes(), &parsed)
        .map_err(|_| AppError::Validation("原密码不正确".into()))?;
    validate_password(&req.new_password)?;
    if req.old_password == req.new_password {
        return Err(AppError::Validation("新密码不能与原密码相同".into()));
    }
    let new_hash = hash_password(&req.new_password)?;
    // F2：自助改密成功 → 清除"必须改密"标记（管理员重置后首次改密即闭环）
    sqlx::query("UPDATE user SET password_hash = ?1, must_change_password = 0 WHERE id = ?2")
        .bind(&new_hash)
        .bind(auth.user_id)
        .execute(pool)
        .await?;

    // 改密后撤销该用户**其它**会话（当前会话保留），逼失陷会话下线
    let current = session_token_from_parts(&parts);
    sqlx::query("DELETE FROM session WHERE user_id = ?1 AND (?2 IS NULL OR token <> ?2)")
        .bind(auth.user_id)
        .bind(&current)
        .execute(pool)
        .await?;

    crate::commands::audit::write_audit_best_effort(
        pool,
        auth.org_id,
        auth.actor(),
        "auth.password_change",
        "user",
        Some(auth.user_id),
        serde_json::json!({ "otherSessionsRevoked": true }),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// POST /api/auth/profile（目前只允许改显示名）
pub async fn update_profile(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ProfileReq>,
) -> AppResult<Json<MeResp>> {
    let display_name = req.display_name.trim().to_string();
    if display_name.chars().count() > 50 {
        return Err(AppError::Validation("显示名最多 50 个字符".into()));
    }
    sqlx::query("UPDATE user SET display_name = ?1 WHERE id = ?2")
        .bind(&display_name)
        .bind(auth.user_id)
        .execute(state.db.as_ref())
        .await?;
    Ok(Json(
        load_me_body(state.db.as_ref(), auth.user_id, auth.org_id).await?,
    ))
}

// ---------------------------------------------------------------------------
// E4 —— 会话管理 / 组织切换
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionInfo {
    org_id: i64,
    org_name: String,
    created_at: String,
    last_seen_at: String,
    expires_at: String,
    user_agent: String,
    ip: String,
    #[serde(skip)]
    token: String,
    current: bool,
}

/// GET /api/auth/sessions —— 我在所有组织的有效会话
pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    parts: Parts,
    auth: AuthUser,
) -> AppResult<Json<Vec<SessionInfo>>> {
    let current = session_token_from_parts(&parts);
    let mut rows = sqlx::query_as::<_, SessionInfo>(
        "SELECT s.org_id AS org_id, o.name AS org_name,
                s.created_at AS created_at, s.last_seen_at AS last_seen_at,
                s.expires_at AS expires_at, s.user_agent AS user_agent, s.ip AS ip,
                s.token AS token, 0 AS current
           FROM session s JOIN org o ON o.id = s.org_id
          WHERE s.user_id = ?1 AND s.expires_at > datetime('now')
          ORDER BY s.last_seen_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(state.db.as_ref())
    .await?;
    for r in &mut rows {
        r.current = Some(r.token.as_str()) == current.as_deref();
    }
    Ok(Json(rows))
}

/// POST /api/auth/sessions/logout-others —— 撤销除当前会话外的全部会话
pub async fn logout_other_sessions(
    State(state): State<AppState>,
    parts: Parts,
    auth: AuthUser,
) -> AppResult<Json<serde_json::Value>> {
    let current = session_token_from_parts(&parts);
    let result =
        sqlx::query("DELETE FROM session WHERE user_id = ?1 AND (?2 IS NULL OR token <> ?2)")
            .bind(auth.user_id)
            .bind(&current)
            .execute(state.db.as_ref())
            .await?;
    crate::commands::audit::write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "auth.sessions_revoke_others",
        "user",
        Some(auth.user_id),
        serde_json::json!({ "revoked": result.rows_affected() }),
    )
    .await;
    Ok(Json(
        serde_json::json!({ "ok": true, "revoked": result.rows_affected() }),
    ))
}

/// POST /api/auth/switch-org —— 切换当前会话到自己所属的另一个组织。
/// 旧会话失效，签发绑定目标组织的新会话。
pub async fn switch_org(
    State(state): State<AppState>,
    parts: Parts,
    auth: AuthUser,
    Json(req): Json<SwitchOrgReq>,
) -> AppResult<Response> {
    if req.org_id == auth.org_id {
        return Err(AppError::Validation("已经在该组织中".into()));
    }
    let pool = state.db.as_ref();
    let member: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE user_id = ?1 AND org_id = ?2")
            .bind(auth.user_id)
            .bind(req.org_id)
            .fetch_one(pool)
            .await?;
    if member == 0 {
        return Err(AppError::Forbidden("你不属于该组织".into()));
    }

    // 旧 token 作废
    if let Some(old) = session_token_from_parts(&parts) {
        sqlx::query("DELETE FROM session WHERE token = ?1")
            .bind(&old)
            .execute(pool)
            .await?;
    }
    let ip = client_ip(&parts, &state.config);
    let ttl = state.config.session_ttl_seconds();
    let token = issue_session(
        pool,
        ttl,
        auth.user_id,
        req.org_id,
        &user_agent(&parts),
        &ip,
    )
    .await?;
    let body = load_me_body(pool, auth.user_id, req.org_id).await?;
    Ok(session_response(&token, ttl, Json(body)).into_response())
}

/// 从请求头解析当前会话 token（不校验有效性，仅用于"保留自己"）。
fn session_token_from_parts(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_session_cookie)
}

async fn issue_session(
    pool: &sqlx::SqlitePool,
    ttl_seconds: i64,
    user_id: i64,
    org_id: i64,
    user_agent: &str,
    ip: &str,
) -> AppResult<String> {
    let token = random_token();
    sqlx::query(
        "INSERT INTO session (token, user_id, org_id, expires_at, user_agent, ip)
         VALUES (?1, ?2, ?3, datetime('now', ?4), ?5, ?6)",
    )
    .bind(&token)
    .bind(user_id)
    .bind(org_id)
    // datetime('now', '+30 days') 不接受变量，改用秒数修饰符
    .bind(format!("+{ttl_seconds} seconds"))
    .bind(user_agent)
    .bind(ip)
    .execute(pool)
    .await?;
    Ok(token)
}

// ---------------------------------------------------------------------------
// F1 —— 运维破窗：本地重置密码（sif-studio-server reset-password）
// ---------------------------------------------------------------------------

/// 不经过 HTTP / 管理员令牌的本地破窗重置：供服务器操作员在 owner 忘记密码、
/// 无法登录系统时直接在服务器上执行（二进制与数据库文件均在本机）。
///
/// - 复用注册同款密码强度校验与 argon2id 哈希；
/// - 置 must_change_password=1：临时密码必须由本人登录后改掉；
/// - 删除该用户全部会话（跨所有组织），立即生效。
pub async fn admin_reset_password_cli(
    pool: &sqlx::SqlitePool,
    raw_email: &str,
    new_password: &str,
) -> AppResult<()> {
    let email = normalize_email(raw_email)?;
    validate_password(new_password)?;

    let user_id: i64 = sqlx::query_scalar("SELECT id FROM user WHERE lower(email) = ?1")
        .bind(&email)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("未找到用户：{email}")))?;

    let new_hash = hash_password(new_password)?;
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE user SET password_hash = ?1, must_change_password = 1 WHERE id = ?2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM session WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

pub(crate) fn hash_password(plain: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Validation(format!("密码哈希失败: {e}")))
}

fn normalize_email(raw: &str) -> AppResult<String> {
    let email = raw.trim().to_lowercase();
    if !email.contains('@') || email.len() < 3 {
        return Err(AppError::Validation("邮箱格式不正确".into()));
    }
    Ok(email)
}

pub(crate) fn validate_password(pw: &str) -> AppResult<()> {
    if pw.chars().count() < 8 {
        return Err(AppError::Validation("密码至少 8 个字符".into()));
    }
    Ok(())
}

fn display_name_or_email<'a>(name: &'a str, email: &'a str) -> &'a str {
    if name.is_empty() {
        email.split('@').next().unwrap_or(email)
    } else {
        name
    }
}

pub(crate) fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// 从 Cookie 头里取 sif_session（手写极简解析，不引第三方 crate）
fn parse_session_cookie(header: &str) -> Option<String> {
    header
        .split(';')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k.trim() == SESSION_COOKIE).then(|| v.trim().to_string())
        })
        .next()
}

fn session_cookie(token: &str, ttl_seconds: i64) -> String {
    let secure = std::env::var("SIF_COOKIE_SECURE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    format!(
        "{SESSION_COOKIE}={token}; Path=/; Max-Age={ttl_seconds}; HttpOnly; SameSite=Lax{}",
        if secure { "; Secure" } else { "" }
    )
}

fn clear_cookie() -> String {
    let secure = std::env::var("SIF_COOKIE_SECURE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    format!(
        "{SESSION_COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{}",
        if secure { "; Secure" } else { "" }
    )
}

fn session_response<T>(token: &str, ttl_seconds: i64, body: T) -> (axum::http::HeaderMap, T) {
    let mut headers = axum::http::HeaderMap::new();
    if let Ok(v) = session_cookie(token, ttl_seconds).parse() {
        headers.insert(SET_COOKIE, v);
    }
    (headers, body)
}

/// 提取客户端真实 IP。
///
/// 安全模型（E2）：
/// - 未开启 `SIF_TRUST_PROXY`（默认）：**只认 TCP 对端地址**，完全忽略
///   `X-Forwarded-For` / `X-Real-IP`——否则直连者每请求换一个伪造头就能
///   拿到无限个限流桶。
/// - 开启（部署在受控反代之后）：优先取反代写入的 `X-Real-IP`（Caddyfile
///   中配置），否则取 `X-Forwarded-For` **末跳**（可信代理追加的那一段；
///   首段可被客户端任意伪造，绝不能用）。
fn client_ip(parts: &Parts, config: &crate::config::AppConfig) -> String {
    if config.trust_proxy {
        if let Some(v) = parts
            .headers
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return v.to_string();
        }
        if let Some(last) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(',').map(str::trim).rfind(|x| !x.is_empty()))
        {
            return last.to_string();
        }
    }
    // serve() 用 into_make_service_with_connect_info 注入；测试 oneshot 下缺失。
    parts
        .extensions
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_default()
}

fn user_agent(parts: &Parts) -> String {
    parts
        .headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}
