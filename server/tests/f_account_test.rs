//! F 阶段 HTTP / 服务测试：
//!   F1 运维破窗 CLI：admin_reset_password_cli（本地重置 + 强制改密 + 踢会话）
//!   F2 owner 重置成员密码后置 must_change_password；自助改密清除标记
//!   F3 viewer 不能导出审计 CSV（403），engineer 可导出（200）

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{json, Value};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::http;
use sif_studio_lib::http::ratelimit::RateLimiter;
use sif_studio_lib::{AppConfig, AppState};
use sqlx::SqlitePool;
use std::sync::Arc;
use tower::ServiceExt;

const PW: &str = "password123";

async fn make_app_with_pool() -> (axum::Router, SqlitePool) {
    let pool = open_in_memory().await.unwrap();
    let app = http::router(AppState {
        db: Arc::new(pool.clone()),
        config: Arc::new(AppConfig::for_test(None, false, false)),
        rate: Arc::new(RateLimiter::new()),
    });
    (app, pool)
}

async fn send_json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, Value, Option<String>) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json");
    let req = match cookie {
        Some(c) => req.header(COOKIE, c),
        None => req,
    };
    let req = req.body(Body::from(body.to_string())).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let set_cookie = resp
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap().to_string());
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, set_cookie)
}

async fn send_get(
    app: &axum::Router,
    uri: &str,
    cookie: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(COOKIE, c);
    }
    let resp = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn register(app: &axum::Router, email: &str, org: &str) -> String {
    let (status, body, set_cookie) = send_json(
        app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": email, "password": PW, "displayName": email.split('@').next().unwrap(), "orgName": org}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "注册应成功: {body}");
    set_cookie.unwrap()
}

async fn login(
    app: &axum::Router,
    email: &str,
    password: &str,
) -> (StatusCode, Value, Option<String>) {
    send_json(
        app,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": email, "password": password}),
    )
    .await
}

async fn me(app: &axum::Router, cookie: &str) -> (StatusCode, Value) {
    let (st, body, _) = send_json(app, "GET", "/api/auth/me", Some(cookie), json!({})).await;
    (st, body)
}

async fn create_invite_token(app: &axum::Router, owner_cookie: &str, role: &str) -> String {
    let (st, inv, _) = send_json(
        app,
        "POST",
        "/api/org/invites",
        Some(owner_cookie),
        json!({"role": role, "ttlDays": 7}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "建邀请: {inv}");
    inv["token"].as_str().unwrap().to_string()
}

async fn member_id_by_email(app: &axum::Router, owner_cookie: &str, email: &str) -> i64 {
    let (st, members, _) = send_json(
        app,
        "GET",
        "/api/org/members",
        Some(owner_cookie),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == email)
        .unwrap()["userId"]
        .as_i64()
        .unwrap()
}

// ---------------------------------------------------------------------------
// F2：owner 重置 → 强制改密标记 → 自助改密清除
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owner_reset_forces_password_change() {
    let (app, _pool) = make_app_with_pool().await;
    let owner = register(&app, "owner@test.local", "甲组织").await;

    // 邀请 engineer 成员（有写权限，专门验证强制改密不依赖 viewer 角色）
    let token = create_invite_token(&app, &owner, "engineer").await;
    let (st, reg, _) = send_json(
        &app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": "eng@test.local", "password": PW, "inviteToken": token}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "凭邀请注册: {reg}");

    let uid = member_id_by_email(&app, &owner, "eng@test.local").await;

    // 普通注册用户默认不强制改密
    let (st, body, cookie) = login(&app, "eng@test.local", PW).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["mustChangePassword"], false);
    let eng_cookie = cookie.unwrap();

    // owner 重置密码
    let temp_pw = "temp-pass-9";
    let (st, body, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{uid}/reset-password"),
        Some(&owner),
        json!({"newPassword": temp_pw}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "owner 重置: {body}");

    // 旧会话立即失效
    let (st, _) = me(&app, &eng_cookie).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // 旧密码登录失败
    let (st, _, _) = login(&app, "eng@test.local", PW).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    // 临时密码登录成功，且 me 带 mustChangePassword=true
    let (st, body, new_cookie) = login(&app, "eng@test.local", temp_pw).await;
    assert_eq!(st, StatusCode::OK, "临时密码登录: {body}");
    assert_eq!(body["mustChangePassword"], true);
    let new_cookie = new_cookie.unwrap();

    // 本人自助改密后标记清除
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/auth/change-password",
        Some(&new_cookie),
        json!({"oldPassword": temp_pw, "newPassword": "my-own-secret1"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "自助改密: {body}");

    let (st, body) = me(&app, &new_cookie).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["mustChangePassword"], false);
}

// ---------------------------------------------------------------------------
// F1：CLI 破窗通道
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cli_reset_password_break_glass() {
    let (app, pool) = make_app_with_pool().await;
    register(&app, "boss@test.local", "甲组织").await;

    // 邮箱不存在 → NotFound（404 语义）
    let err = http::auth::admin_reset_password_cli(&pool, "nobody@test.local", "newpass123")
        .await
        .expect_err("不存在的用户应报错");
    assert!(
        matches!(err, sif_studio_lib::AppError::NotFound(_)),
        "{err:?}"
    );

    // 弱密码被同一套校验拦下
    let err = http::auth::admin_reset_password_cli(&pool, "boss@test.local", "short")
        .await
        .expect_err("弱密码应报错");
    assert!(
        matches!(err, sif_studio_lib::AppError::Validation(_)),
        "{err:?}"
    );

    // 邮箱大小写 / 空白归一化
    http::auth::admin_reset_password_cli(&pool, "  BOSS@test.local ", "breakglass1")
        .await
        .unwrap();

    // 旧密码失效、新密码可登录且带强制改密标记
    let (st, _, _) = login(&app, "boss@test.local", PW).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, body, _) = login(&app, "boss@test.local", "breakglass1").await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(body["mustChangePassword"], true);
}

// ---------------------------------------------------------------------------
// F3：审计导出角色门
// ---------------------------------------------------------------------------

#[tokio::test]
async fn viewer_cannot_export_audit_csv() {
    let (app, _pool) = make_app_with_pool().await;
    let owner = register(&app, "owner@test.local", "甲组织").await;

    // engineer
    let eng_token = create_invite_token(&app, &owner, "engineer").await;
    let (st, _, _) = send_json(
        &app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": "eng@test.local", "password": PW, "inviteToken": eng_token}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, eng_cookie) = login(&app, "eng@test.local", PW).await;
    let eng_cookie = eng_cookie.unwrap();

    // viewer
    let v_token = create_invite_token(&app, &owner, "viewer").await;
    let (st, _, _) = send_json(
        &app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": "viewer@test.local", "password": PW, "inviteToken": v_token}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (_, _, viewer_cookie) = login(&app, "viewer@test.local", PW).await;
    let viewer_cookie = viewer_cookie.unwrap();

    // viewer → 403
    let (st, headers, bytes) = send_get(&app, "/api/audit/export.csv", Some(&viewer_cookie)).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    assert_ne!(
        headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()),
        Some("text/csv; charset=utf-8"),
        "viewer 不应拿到 CSV 响应体"
    );
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["kind"], "forbidden");

    // engineer → 200 text/csv（空审计也带表头/BOM）
    let (st, headers, bytes) = send_get(&app, "/api/audit/export.csv", Some(&eng_cookie)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .strip_prefix("text/csv"),
        Some("; charset=utf-8")
    );
    assert!(!bytes.is_empty(), "CSV 至少包含 BOM/表头");

    // owner 同样可导出
    let (st, _, _) = send_get(&app, "/api/audit/export.csv", Some(&owner)).await;
    assert_eq!(st, StatusCode::OK);
}
