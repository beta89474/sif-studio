//! D 阶段 HTTP 测试：
//!   D1 组织邀请链接（第二人加入同组织、共享数据、非 owner 被 403）
//!   D1 成员管理（改角色 / 移除 / 最后一个 owner 保护 / 被移除后会话失效）
//!   D2 登录失败限流（11 次连错 → 429，正确密码也暂时被拒）
//!   D2 跨版本恢复（v4 备份恢复到当前服务时自动升级）

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{json, Value};
use sif_studio_lib::db::{open_file, open_in_memory};
use sif_studio_lib::http::ratelimit::RateLimiter;
use sif_studio_lib::{http, AppConfig, AppState};
use std::sync::Arc;
use tower::ServiceExt;

const PW: &str = "password123";
const TOKEN: &str = "d-admin-token";

async fn make_app(token: Option<&str>) -> axum::Router {
    let pool = open_in_memory().await.unwrap();
    http::router(AppState {
        db: Arc::new(pool),
        config: Arc::new(AppConfig::with_admin_token(token)),
        rate: Arc::new(RateLimiter::new()),
    })
}

/// 文件库 app + TempDir 守卫（备份/恢复用）。
async fn make_file_app(token: Option<&str>) -> (axum::Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = open_file(&dir.path().join("studio.db")).await.unwrap();
    let app = http::router(AppState {
        db: Arc::new(pool),
        config: Arc::new(AppConfig::with_admin_token(token)),
        rate: Arc::new(RateLimiter::new()),
    });
    (app, dir)
}

async fn send_raw(
    app: axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    extra_headers: &[(&str, &str)],
    content_type: Option<&str>,
    body: Option<Vec<u8>>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(COOKIE, c);
    }
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let req = match (content_type, body) {
        (Some(ct), Some(b)) => builder
            .header(CONTENT_TYPE, ct)
            .body(Body::from(b))
            .unwrap(),
        _ => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    (status, headers, bytes)
}

async fn send_json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, Value, Option<String>) {
    let (status, headers, bytes) = send_raw(
        app.clone(),
        method,
        uri,
        cookie,
        &[],
        Some("application/json"),
        Some(body.to_string().into_bytes()),
    )
    .await;
    let set_cookie = headers
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, set_cookie)
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
    set_cookie.unwrap().split(';').next().unwrap().to_string()
}

async fn register_with_invite(
    app: &axum::Router,
    email: &str,
    token: &str,
) -> (StatusCode, Value, Option<String>) {
    let (status, body, set_cookie) = send_json(
        app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": email, "password": PW, "displayName": email.split('@').next().unwrap(), "inviteToken": token}),
    )
    .await;
    (
        status,
        body,
        set_cookie.map(|c| c.split(';').next().unwrap().to_string()),
    )
}

async fn rpc(app: &axum::Router, cookie: &str, cmd: &str, args: Value) -> (StatusCode, Value) {
    let (status, body, _) = send_json(
        app,
        "POST",
        "/api/rpc",
        Some(cookie),
        json!({"cmd": cmd, "args": args}),
    )
    .await;
    (status, body)
}

fn multipart_file(field: &str, filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----sifdtestboundary";
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{field}\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    out.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), out)
}

// ---------------------------------------------------------------------------
// D1 —— 邀请链接加入同一组织
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invite_link_lets_second_user_join_same_org() {
    let app = make_app(None).await;

    // owner 注册 + 建项目
    let cookie_a = register(&app, "alice-d@test.local", "甲设计公司").await;
    let (st, _) = rpc(
        &app,
        &cookie_a,
        "create_project",
        json!({"input": {"code": "PRJ-D1", "name": "邀请共享项目"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 签发 engineer 邀请
    let (st, inv, _) = send_json(
        &app,
        "POST",
        "/api/org/invites",
        Some(&cookie_a),
        json!({"role": "engineer"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "建邀请: {inv}");
    let token = inv["token"].as_str().unwrap().to_string();

    // 公开预览
    let (st, preview, _) = send_json(
        &app,
        "GET",
        &format!("/api/auth/invite?token={token}"),
        None,
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "预览: {preview}");
    assert_eq!(preview["orgName"], "甲设计公司");
    assert_eq!(preview["role"], "engineer");

    // 第二人凭邀请注册：不开新组织，直接进甲设计公司
    let (st, body, cookie_b) = register_with_invite(&app, "bob-d@test.local", &token).await;
    assert_eq!(st, StatusCode::OK, "邀请注册: {body}");
    let cookie_b = cookie_b.unwrap();
    assert_eq!(body["orgId"], 1);
    assert_eq!(body["orgName"], "甲设计公司");
    assert_eq!(body["role"], "engineer");

    // 看到同组织项目
    let (st, list) = rpc(&app, &cookie_b, "list_projects", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let codes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(
        codes.contains(&"PRJ-D1"),
        "新成员应看到组织内项目: {codes:?}"
    );

    // engineer 不能管理邀请
    let (st, _, _) = send_json(&app, "POST", "/api/org/invites", Some(&cookie_b), json!({})).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // 成员列表 2 人
    let (st, members, _) =
        send_json(&app, "GET", "/api/org/members", Some(&cookie_b), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(members.as_array().unwrap().len(), 2);

    // 无效/过期/乱填 token 注册 → 404
    let (st, _, _) = register_with_invite(&app, "eve-d@test.local", "deadbeef").await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// D1 —— 成员管理 + 最后一个 owner 保护
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owner_can_manage_members_but_not_last_owner() {
    let app = make_app(None).await;
    let cookie_a = register(&app, "alice-mg@test.local", "甲公司").await;

    let (_, inv, _) = send_json(
        &app,
        "POST",
        "/api/org/invites",
        Some(&cookie_a),
        json!({"role": "viewer"}),
    )
    .await;
    let token = inv["token"].as_str().unwrap().to_string();
    let (st, _, cookie_c) = register_with_invite(&app, "carol-mg@test.local", &token).await;
    assert_eq!(st, StatusCode::OK);
    let cookie_c = cookie_c.unwrap();

    // carol 的用户 id
    let (st, members, _) =
        send_json(&app, "GET", "/api/org/members", Some(&cookie_a), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let carol_id = members
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["email"] == "carol-mg@test.local")
        .unwrap()["userId"]
        .as_i64()
        .unwrap();
    let (st, me, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_a), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let alice_id = me["userId"].as_i64().unwrap();

    // 提升 carol 为 engineer
    let (st, body, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{carol_id}/role"),
        Some(&cookie_a),
        json!({"role": "engineer"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "改角色: {body}");

    // 非法角色 422
    let (st, _, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{carol_id}/role"),
        Some(&cookie_a),
        json!({"role": "superuser"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    // 不能摘掉最后一个 owner
    let (st, body, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{alice_id}/role"),
        Some(&cookie_a),
        json!({"role": "engineer"}),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "最后 owner 保护: {body}");

    // 最后一个 owner 不能退出
    let (st, _, _) = send_raw(
        app.clone(),
        "DELETE",
        &format!("/api/org/members/{alice_id}"),
        Some(&cookie_a),
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT);

    // 移除 carol：200，且其会话立即失效
    let (st, _, _) = send_raw(
        app.clone(),
        "DELETE",
        &format!("/api/org/members/{carol_id}"),
        Some(&cookie_a),
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_c), json!({})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// D2 —— 登录限流
// ---------------------------------------------------------------------------

#[tokio::test]
async fn login_rate_limited_after_10_failures() {
    let app = make_app(None).await;
    let _cookie = register(&app, "dave-rl@test.local", "限流公司").await;

    // 前 10 次错密码：401
    for i in 1..=10 {
        let (st, body, _) = send_json(
            &app,
            "POST",
            "/api/auth/login",
            None,
            json!({"email": "dave-rl@test.local", "password": "wrong-password"}),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "第 {i} 次应为 401: {body}");
    }
    // 第 11 次：429（即使密码正确也先被限流挡住）
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": "dave-rl@test.local", "password": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "应被限流: {body}");
}

// ---------------------------------------------------------------------------
// D2 —— 旧版本备份（v4）恢复到当前（v6）服务时自动升级
// ---------------------------------------------------------------------------

#[tokio::test]
async fn restore_auto_migrates_v4_backup() {
    // app A（当前版本 v6）：注册 + 建项目 + 备份
    let (app_a, _tmp_a) = make_file_app(Some(TOKEN)).await;
    let cookie = register(&app_a, "vera-v4@test.local", "升级公司").await;
    let (st, _) = rpc(
        &app_a,
        &cookie,
        "create_project",
        json!({"input": {"code": "PRJ-V4", "name": "跨版本恢复项目"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, _, bytes) = send_raw(
        app_a,
        "GET",
        "/api/admin/backup",
        None,
        &[("x-admin-token", TOKEN)],
        None,
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    // 把备份"降级"成 v4 形态：抹掉 005/006 账本行 + 丢弃 005 建的 org_invite
    // 表 + 移除 006 加的 user.must_change_password 列（恢复时副本应自动重放
    // 005/006 迁移；bundled SQLite ≥3.35 支持 DROP COLUMN）。
    let dir = tempfile::tempdir().unwrap();
    let v4_path = dir.path().join("backup-v4.db");
    std::fs::write(&v4_path, &bytes).unwrap();
    let opts = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(&v4_path)
        .foreign_keys(false);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    sqlx::query("DELETE FROM _sqlx_migrations WHERE version IN (5, 6)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DROP TABLE org_invite")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE user DROP COLUMN must_change_password")
        .execute(&pool)
        .await
        .unwrap();
    let latest: i64 = sqlx::query_scalar("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(latest, 4, "造出来的源备份应停留在 v4");
    pool.close().await;
    let v4_bytes = std::fs::read(&v4_path).unwrap();

    // app B（全新 v6 库）：恢复 v4 备份 → 应自动升级并 200
    let (app_b, _tmp_b) = make_file_app(Some(TOKEN)).await;
    let (ct, body) = multipart_file("file", "v4.db", &v4_bytes);
    let (st, _, resp) = send_raw(
        app_b.clone(),
        "POST",
        "/api/admin/restore",
        None,
        &[("x-admin-token", TOKEN)],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(
        st,
        StatusCode::OK,
        "v4 备份应自动升级恢复: {}",
        String::from_utf8_lossy(&resp)
    );
    let report: Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(report["migrationVersion"], 6);
    assert_eq!(report["tablesCopied"], 13);
    assert!(report["rowsCopied"]["org_invite"].as_i64().is_some());

    // 恢复后登录见到项目
    let (st, _, set_cookie) = send_json(
        &app_b,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": "vera-v4@test.local", "password": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let cookie_b = set_cookie.unwrap().split(';').next().unwrap().to_string();
    let (st, list) = rpc(&app_b, &cookie_b, "list_projects", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let codes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"PRJ-V4"), "升级恢复后应见到项目: {codes:?}");
}
