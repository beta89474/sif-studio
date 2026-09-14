//! 组织成员与邀请（阶段 D1）
//!
//! 路由：
//! - GET  /api/auth/invite?token=…   公开：注册页预览邀请信息
//! - POST /api/org/invites           owner 签发邀请（多次使用直到过期/吊销）
//! - GET  /api/org/invites           owner 列出本组织邀请
//! - DELETE /api/org/invites/:token  owner 吊销邀请
//! - POST /api/org/invite/accept    已登录用户接受邀请（加入该组织）
//! - GET  /api/org/members           成员列表（任何成员可看）
//! - POST /api/org/members/:uid/role owner 改角色（不能摘掉最后一个 owner）
//! - DELETE /api/org/members/:uid    owner 移除成员 / 成员自己退出
//!
//! 多租户铁律：org_id 一律来自会话/邀请记录，不接受请求体传入。

use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::commands::audit::write_audit_best_effort;
use crate::http::auth::{hash_password, random_token, validate_password, AuthUser};
use crate::{AppError, AppResult, AppState};

const VALID_ROLES: [&str; 3] = ["owner", "engineer", "viewer"];
// 邀请有效期默认/上限已移至 AppConfig（SIF_INVITE_TTL_DEFAULT / _MAX，默认 7/30 天）。

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InviteRow {
    pub token: String,
    pub org_id: i64,
    pub role: String,
    pub invited_by: i64,
    pub created_at: String,
    pub expires_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteInfo {
    pub org_name: String,
    pub role: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteReq {
    /// 新成员角色，默认 engineer
    #[serde(default)]
    pub role: Option<String>,
    /// 有效期天数（1~30，默认 7）
    #[serde(default)]
    pub ttl_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenQuery {
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptReq {
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleReq {
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordReq {
    pub new_password: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MemberRow {
    pub user_id: i64,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub created_at: String,
}

fn require_owner(auth: &AuthUser) -> AppResult<()> {
    if auth.role != "owner" {
        return Err(AppError::Forbidden("仅组织 owner 可执行该操作".into()));
    }
    Ok(())
}

fn normalize_role(role: &Option<String>) -> AppResult<String> {
    let r = role.as_deref().unwrap_or("engineer");
    if !VALID_ROLES.contains(&r) {
        return Err(AppError::Validation(format!(
            "角色必须是 {} 之一",
            VALID_ROLES.join(" / ")
        )));
    }
    Ok(r.to_string())
}

/// 取一条有效（存在、未吊销、未过期）邀请。返回 (org_id, role, expires_at)。
pub(crate) async fn load_active_invite(
    state: &AppState,
    token: &str,
) -> AppResult<(i64, String, String)> {
    let row = sqlx::query_as::<_, InviteRow>(
        "SELECT token, org_id, role, invited_by, created_at, expires_at, revoked_at
           FROM org_invite WHERE token = ?1",
    )
    .bind(token)
    .fetch_optional(state.db.as_ref())
    .await?
    .ok_or_else(|| AppError::NotFound("邀请不存在或已失效".into()))?;

    if row.revoked_at.is_some() {
        return Err(AppError::NotFound("邀请已被吊销".into()));
    }
    let active: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_invite
          WHERE token = ?1 AND expires_at > datetime('now')",
    )
    .bind(token)
    .fetch_one(state.db.as_ref())
    .await?;
    if active == 0 {
        return Err(AppError::NotFound("邀请已过期".into()));
    }
    Ok((row.org_id, row.role, row.expires_at))
}

/// 统计组织内 owner 数量（最后一个 owner 保护）。
async fn owner_count(state: &AppState, org_id: i64) -> AppResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND role = 'owner'")
            .bind(org_id)
            .fetch_one(state.db.as_ref())
            .await?,
    )
}

// ---------------------------------------------------------------------------
// handlers
// ---------------------------------------------------------------------------

/// GET /api/auth/invite?token=…（公开，注册页用）
pub async fn invite_preview(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
) -> AppResult<Json<InviteInfo>> {
    let (org_id, role, expires_at) = load_active_invite(&state, &q.token).await?;
    let org_name: String = sqlx::query_scalar("SELECT name FROM org WHERE id = ?1")
        .bind(org_id)
        .fetch_one(state.db.as_ref())
        .await?;
    Ok(Json(InviteInfo {
        org_name,
        role,
        expires_at,
    }))
}

/// POST /api/org/invites
pub async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateInviteReq>,
) -> AppResult<Json<InviteRow>> {
    require_owner(&auth)?;
    let role = normalize_role(&req.role)?;
    // G3 —— 默认/上限由部署配置（SIF_INVITE_TTL_DEFAULT / _MAX）决定
    let ttl = match req.ttl_days {
        Some(d) if (1..=state.config.invite_ttl_max).contains(&d) => d,
        Some(_) => {
            return Err(AppError::Validation(format!(
                "有效期需在 1~{} 天之间",
                state.config.invite_ttl_max
            )))
        }
        None => state.config.invite_ttl_default,
    };

    let token = random_token();
    let row = sqlx::query_as::<_, InviteRow>(
        "INSERT INTO org_invite (token, org_id, role, invited_by, expires_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now', ?5))
         RETURNING token, org_id, role, invited_by, created_at, expires_at, revoked_at",
    )
    .bind(&token)
    .bind(auth.org_id)
    .bind(&role)
    .bind(auth.user_id)
    .bind(format!("+{ttl} days"))
    .fetch_one(state.db.as_ref())
    .await?;

    write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "org.invite_create",
        "org_invite",
        None,
        serde_json::json!({ "token": token, "role": role, "ttlDays": ttl }),
    )
    .await;
    Ok(Json(row))
}

/// GET /api/org/invites（owner 专属：有效 token 等于加入凭证，不对普通成员扩散）
pub async fn list_invites(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<InviteRow>>> {
    require_owner(&auth)?;
    let rows = sqlx::query_as::<_, InviteRow>(
        "SELECT token, org_id, role, invited_by, created_at, expires_at, revoked_at
           FROM org_invite WHERE org_id = ?1 ORDER BY created_at DESC",
    )
    .bind(auth.org_id)
    .fetch_all(state.db.as_ref())
    .await?;
    Ok(Json(rows))
}

/// DELETE /api/org/invites/:token
pub async fn revoke_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(token): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    require_owner(&auth)?;
    let result = sqlx::query(
        "UPDATE org_invite SET revoked_at = datetime('now')
          WHERE token = ?1 AND org_id = ?2 AND revoked_at IS NULL",
    )
    .bind(&token)
    .bind(auth.org_id)
    .execute(state.db.as_ref())
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("邀请不存在或已吊销".into()));
    }
    write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "org.invite_revoke",
        "org_invite",
        None,
        serde_json::json!({ "token": token }),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/org/invites/accept（已登录用户加入新组织；当前会话仍停留在原组织，
/// 需重新登录后进入新组织——D1 不做组织切换器）。
pub async fn accept_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<AcceptReq>,
) -> AppResult<Json<serde_json::Value>> {
    let (org_id, _role, _) = load_active_invite(&state, &req.token).await?;

    let exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND user_id = ?2")
            .bind(org_id)
            .bind(auth.user_id)
            .fetch_one(state.db.as_ref())
            .await?;
    if exists > 0 {
        return Err(AppError::Conflict("你已经是该组织成员".into()));
    }

    sqlx::query("INSERT INTO org_member (org_id, user_id, role) SELECT ?1, ?2, role FROM org_invite WHERE token = ?3")
        .bind(org_id)
        .bind(auth.user_id)
        .bind(&req.token)
        .execute(state.db.as_ref())
        .await?;

    let org_name: String = sqlx::query_scalar("SELECT name FROM org WHERE id = ?1")
        .bind(org_id)
        .fetch_one(state.db.as_ref())
        .await?;
    write_audit_best_effort(
        state.db.as_ref(),
        org_id,
        auth.actor(),
        "org.member_join",
        "org_member",
        Some(auth.user_id),
        serde_json::json!({ "via": "authed_accept" }),
    )
    .await;
    Ok(Json(serde_json::json!({ "ok": true, "orgName": org_name })))
}

/// GET /api/org/members
pub async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<MemberRow>>> {
    let rows = sqlx::query_as::<_, MemberRow>(
        "SELECT u.id AS user_id, u.email AS email, u.display_name AS display_name,
                m.role AS role, m.created_at AS created_at
           FROM org_member m
           JOIN user u ON u.id = m.user_id
          WHERE m.org_id = ?1
          ORDER BY CASE m.role WHEN 'owner' THEN 0 WHEN 'engineer' THEN 1 ELSE 2 END,
                   m.created_at ASC",
    )
    .bind(auth.org_id)
    .fetch_all(state.db.as_ref())
    .await?;
    Ok(Json(rows))
}

/// POST /api/org/members/:user_id/role
pub async fn set_member_role(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<i64>,
    Json(req): Json<RoleReq>,
) -> AppResult<Json<serde_json::Value>> {
    require_owner(&auth)?;
    if !VALID_ROLES.contains(&req.role.as_str()) {
        return Err(AppError::Validation(format!(
            "角色必须是 {} 之一",
            VALID_ROLES.join(" / ")
        )));
    }

    let in_org: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND user_id = ?2")
            .bind(auth.org_id)
            .bind(user_id)
            .fetch_one(state.db.as_ref())
            .await?;
    if in_org == 0 {
        return Err(AppError::NotFound("该用户不在本组织".into()));
    }

    // 仅当目标当前是 owner、且要改成非 owner 时，才需要检查剩余 owner 数；
    // viewer/engineer 之间互转与提权不受影响。
    if req.role != "owner" {
        let current_role: String =
            sqlx::query_scalar("SELECT role FROM org_member WHERE org_id = ?1 AND user_id = ?2")
                .bind(auth.org_id)
                .bind(user_id)
                .fetch_one(state.db.as_ref())
                .await?;
        if current_role == "owner" && owner_count(&state, auth.org_id).await? <= 1 {
            return Err(AppError::Conflict("组织必须保留至少一个 owner".into()));
        }
    }

    sqlx::query("UPDATE org_member SET role = ?1 WHERE org_id = ?2 AND user_id = ?3")
        .bind(&req.role)
        .bind(auth.org_id)
        .bind(user_id)
        .execute(state.db.as_ref())
        .await?;

    write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "org.member_role_change",
        "org_member",
        Some(user_id),
        serde_json::json!({ "role": req.role }),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// POST /api/org/members/:user_id/reset-password（E3）
/// owner 为成员重置密码。密码是用户的全局凭证，重置后撤销该用户在
/// **所有组织**的会话，迫使用新密码重新登录。
pub async fn reset_member_password(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<i64>,
    Json(req): Json<ResetPasswordReq>,
) -> AppResult<Json<serde_json::Value>> {
    require_owner(&auth)?;
    validate_password(&req.new_password)?;

    let in_org: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND user_id = ?2")
            .bind(auth.org_id)
            .bind(user_id)
            .fetch_one(state.db.as_ref())
            .await?;
    if in_org == 0 {
        return Err(AppError::NotFound("该用户不在本组织".into()));
    }

    let new_hash = hash_password(&req.new_password)?;
    let mut tx = state.db.begin().await?;
    // F2：管理员设定的是临时密码，置 must_change_password 逼用户首次登录后改掉
    sqlx::query("UPDATE user SET password_hash = ?1, must_change_password = 1 WHERE id = ?2")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    // 撤销该用户全部会话（跨组织）——密码被重置通常意味着账号需要重新认证
    sqlx::query("DELETE FROM session WHERE user_id = ?1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "org.member_password_reset",
        "user",
        Some(user_id),
        serde_json::json!({ "allSessionsRevoked": true, "mustChangePassword": true }),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}

/// DELETE /api/org/members/:user_id（owner 移除他人；成员可移除自己=退出）
pub async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(user_id): Path<i64>,
) -> AppResult<Json<serde_json::Value>> {
    if user_id != auth.user_id {
        require_owner(&auth)?;
    }

    let in_org: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND user_id = ?2")
            .bind(auth.org_id)
            .bind(user_id)
            .fetch_one(state.db.as_ref())
            .await?;
    if in_org == 0 {
        return Err(AppError::NotFound("该用户不在本组织".into()));
    }

    let is_owner: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM org_member WHERE org_id = ?1 AND user_id = ?2 AND role = 'owner'",
    )
    .bind(auth.org_id)
    .bind(user_id)
    .fetch_one(state.db.as_ref())
    .await?;
    if is_owner > 0 && owner_count(&state, auth.org_id).await? <= 1 {
        return Err(AppError::Conflict("组织必须保留至少一个 owner".into()));
    }

    // 会话跟着成员关系走：删掉该用户在本组织的所有会话（含自己退出后强制重登）
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM session WHERE org_id = ?1 AND user_id = ?2")
        .bind(auth.org_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM org_member WHERE org_id = ?1 AND user_id = ?2")
        .bind(auth.org_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    write_audit_best_effort(
        state.db.as_ref(),
        auth.org_id,
        auth.actor(),
        "org.member_remove",
        "org_member",
        Some(user_id),
        serde_json::json!({ "self": user_id == auth.user_id }),
    )
    .await;
    Ok(Json(serde_json::json!({"ok": true})))
}
