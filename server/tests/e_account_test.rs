//! E 阶段 HTTP 测试：
//!   E1 viewer 垂直越权修复（写命令 403 / 读命令 200）
//!   E2 X-Forwarded-For 取信修正（默认忽略伪造头；SIF_TRUST_PROXY 才采信）
//!   E3 自助改密（旧密码校验 / 踢掉其它会话）+ owner 重置成员密码
//!   E4 邀请制开关（空库引导、之后 403、邀请仍可注册）
//!   E4 会话列表 / 退出其他设备 / 多组织切换（me.orgs + switch-org）

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{json, Value};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::http::ratelimit::RateLimiter;
use sif_studio_lib::{http, AppConfig, AppState};
use std::sync::Arc;
use tower::ServiceExt;

const PW: &str = "password123";

async fn make_app(trust_proxy: bool, invite_only: bool) -> axum::Router {
    let pool = open_in_memory().await.unwrap();
    http::router(AppState {
        db: Arc::new(pool),
        config: Arc::new(AppConfig::for_test(None, trust_proxy, invite_only)),
        rate: Arc::new(RateLimiter::new()),
    })
}

async fn send_raw(
    app: axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    extra_headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(COOKIE, c);
    }
    for (k, v) in extra_headers {
        builder = builder.header(*k, *v);
    }
    let req = match body {
        Some(b) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(b))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
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
    send_json_headers(app, method, uri, cookie, &[], body).await
}

async fn send_json_headers(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    extra_headers: &[(&str, &str)],
    body: Value,
) -> (StatusCode, Value, Option<String>) {
    let (status, headers, bytes) = send_raw(
        app.clone(),
        method,
        uri,
        cookie,
        extra_headers,
        Some(body.to_string().into_bytes()),
    )
    .await;
    let set_cookie = headers
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.split(';').next().unwrap().to_string());
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, set_cookie)
}

/// 注册并返回会话 cookie。
async fn register(app: &axum::Router, email: &str, org: &str) -> String {
    register_headers(app, email, org, &[]).await
}

async fn register_headers(
    app: &axum::Router,
    email: &str,
    org: &str,
    headers: &[(&str, &str)],
) -> String {
    let (status, body, set_cookie) = send_json_headers(
        app,
        "POST",
        "/api/auth/register",
        None,
        headers,
        json!({"email": email, "password": PW, "displayName": email.split('@').next().unwrap(), "orgName": org}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "注册应成功: {body}");
    set_cookie.unwrap()
}

async fn register_with_invite(
    app: &axum::Router,
    email: &str,
    token: &str,
) -> (StatusCode, Value, Option<String>) {
    send_json(
        app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": email, "password": PW, "displayName": email.split('@').next().unwrap(), "inviteToken": token}),
    )
    .await
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

async fn create_invite_token(app: &axum::Router, owner_cookie: &str, role: &str) -> String {
    let (st, inv, _) = send_json(
        app,
        "POST",
        "/api/org/invites",
        Some(owner_cookie),
        json!({"role": role}),
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
// E1 —— viewer 只读
// ---------------------------------------------------------------------------

#[tokio::test]
async fn viewer_cannot_write_but_can_read() {
    let app = make_app(false, false).await;
    let owner = register(&app, "alice-e1@test.local", "甲公司").await;

    let token = create_invite_token(&app, &owner, "viewer").await;
    let (st, body, viewer_cookie) = register_with_invite(&app, "vic-e1@test.local", &token).await;
    assert_eq!(st, StatusCode::OK, "viewer 注册: {body}");
    let viewer = viewer_cookie.unwrap();

    // owner 先建一个项目（保证读命令有数据）
    let (st, p) = rpc(
        &app,
        &owner,
        "create_project",
        json!({"input": {"code": "PRJ-E1", "name": "越权验证项目"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "owner 建项目: {p}");

    // viewer 各类写命令 → 403（参数残缺也没关系：权限检查在分发之前）
    for cmd in [
        "create_project",
        "update_project",
        "delete_project",
        "create_instrument",
        "save_diagram_data",
        "create_sif",
        "link_instrument_to_sif",
        "create_bypass",
        "restore_bypass",
        "commit_import_instruments",
        "import_tags_csv",
    ] {
        let (st, err) = rpc(&app, &viewer, cmd, json!({})).await;
        assert_eq!(st, StatusCode::FORBIDDEN, "viewer 执行 {cmd} 应 403: {err}");
        assert_eq!(err["kind"], "forbidden");
    }

    // viewer 读命令 → 200
    let (st, list) = rpc(&app, &viewer, "list_projects", json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let codes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"PRJ-E1"), "viewer 应能读项目: {codes:?}");
    let (st, _) = rpc(&app, &viewer, "db_version", json!({})).await;
    assert_eq!(st, StatusCode::OK);

    // engineer 写命令不受影响
    let etoken = create_invite_token(&app, &owner, "engineer").await;
    let (st, _, ecookie) = register_with_invite(&app, "eng-e1@test.local", &etoken).await;
    assert_eq!(st, StatusCode::OK);
    let (st, body) = rpc(
        &app,
        &ecookie.unwrap(),
        "create_project",
        json!({"input": {"code": "PRJ-ENG", "name": "工程师项目"}}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "engineer 可写: {body}");
}

// ---------------------------------------------------------------------------
// E2 —— XFF / X-Real-IP 取信
// ---------------------------------------------------------------------------

#[tokio::test]
async fn xff_ignored_without_trust_proxy() {
    // 默认（直连）：伪造 X-Forwarded-For / X-Real-IP 不进会话 IP
    let app = make_app(false, false).await;
    let cookie = register_headers(
        &app,
        "mallory-e2@test.local",
        "直连公司",
        &[("x-forwarded-for", "6.6.6.6"), ("x-real-ip", "7.7.7.7")],
    )
    .await;
    let (st, sessions, _) =
        send_json(&app, "GET", "/api/auth/sessions", Some(&cookie), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let ip = &sessions.as_array().unwrap()[0]["ip"];
    assert_eq!(ip.as_str().unwrap(), "", "未开 trust_proxy 时必须忽略伪头");
}

#[tokio::test]
async fn trusted_proxy_headers_used() {
    let app = make_app(true, false).await;

    // X-Real-IP 优先
    let cookie = register_headers(
        &app,
        "trusted-e2@test.local",
        "反代公司",
        &[("x-real-ip", "9.9.9.9"), ("x-forwarded-for", "6.6.6.6")],
    )
    .await;
    let (st, sessions, _) =
        send_json(&app, "GET", "/api/auth/sessions", Some(&cookie), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(sessions.as_array().unwrap()[0]["ip"], "9.9.9.9");

    // 无 X-Real-IP 时取 XFF 末跳（客户端伪造的首段不被采信）
    let (st, _, cookie2) = send_json_headers(
        &app,
        "POST",
        "/api/auth/login",
        None,
        &[("x-forwarded-for", "1.2.3.4, 5.5.5.5")],
        json!({"email": "trusted-e2@test.local", "password": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let (st, sessions, _) = send_json(
        &app,
        "GET",
        "/api/auth/sessions",
        Some(&cookie2.unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ips: Vec<&str> = sessions
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["ip"].as_str().unwrap())
        .collect();
    assert!(
        ips.contains(&"5.5.5.5"),
        "XFF 末跳应被采信，首段伪造值不应出现: {ips:?}"
    );
    assert!(!ips.contains(&"1.2.3.4"));
}

// ---------------------------------------------------------------------------
// E3 —— 自助改密 + 改显示名
// ---------------------------------------------------------------------------

#[tokio::test]
async fn change_password_kills_other_sessions() {
    let app = make_app(false, false).await;
    let cookie_a = register(&app, "carl-e3@test.local", "改密公司").await;
    // 第二个设备登录
    let (st, _, cookie_b) = login(&app, "carl-e3@test.local", PW).await;
    assert_eq!(st, StatusCode::OK);
    let cookie_b = cookie_b.unwrap();

    // 原密码错误 → 422
    let (st, _, _) = send_json(
        &app,
        "POST",
        "/api/auth/change-password",
        Some(&cookie_a),
        json!({"oldPassword": "nope-nope", "newPassword": "new-password1"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    // 新旧相同 → 422
    let (st, _, _) = send_json(
        &app,
        "POST",
        "/api/auth/change-password",
        Some(&cookie_a),
        json!({"oldPassword": PW, "newPassword": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    // 正常改密
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/auth/change-password",
        Some(&cookie_a),
        json!({"oldPassword": PW, "newPassword": "new-password1"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "改密: {body}");

    // 当前会话保留；其它会话被踢
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_a), json!({})).await;
    assert_eq!(st, StatusCode::OK, "改密的当前会话应保持登录");
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_b), json!({})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "其它设备会话应失效");

    // 旧密码登录失败、新密码登录成功
    let (st, _, _) = login(&app, "carl-e3@test.local", PW).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _, _) = login(&app, "carl-e3@test.local", "new-password1").await;
    assert_eq!(st, StatusCode::OK);

    // 改显示名
    let (st, me, _) = send_json(
        &app,
        "POST",
        "/api/auth/profile",
        Some(&cookie_a),
        json!({"displayName": "卡尔改名"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me["displayName"], "卡尔改名");
}

// ---------------------------------------------------------------------------
// E3 —— owner 重置成员密码
// ---------------------------------------------------------------------------

#[tokio::test]
async fn owner_resets_member_password() {
    let app = make_app(false, false).await;
    let owner = register(&app, "ora-e3@test.local", "重置公司").await;
    let token = create_invite_token(&app, &owner, "engineer").await;
    let (st, _, member_cookie) = register_with_invite(&app, "moe-e3@test.local", &token).await;
    assert_eq!(st, StatusCode::OK);
    let member_cookie = member_cookie.unwrap();
    let member_id = member_id_by_email(&app, &owner, "moe-e3@test.local").await;

    // 成员自己不能重置别人（路由要 owner）—— 对自己调 reset 也应 403
    let (st, _, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{member_id}/reset-password"),
        Some(&member_cookie),
        json!({"newPassword": "whatever-123"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // owner 重置：弱密码 422
    let (st, _, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{member_id}/reset-password"),
        Some(&owner),
        json!({"newPassword": "short"}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY);

    // 正式重置
    let (st, body, _) = send_json(
        &app,
        "POST",
        &format!("/api/org/members/{member_id}/reset-password"),
        Some(&owner),
        json!({"newPassword": "reset-pw-123"}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "重置: {body}");

    // 成员旧会话全失效（跨组织），旧密码登录失败、新密码成功
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&member_cookie), json!({})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _, _) = login(&app, "moe-e3@test.local", PW).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, _, _) = login(&app, "moe-e3@test.local", "reset-pw-123").await;
    assert_eq!(st, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// E4 —— 邀请制开关
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invite_only_blocks_public_registration() {
    let app = make_app(false, true).await;

    // 空库引导：首个用户允许落 owner
    let owner = register(&app, "boot-e4@test.local", "邀请制公司").await;

    // 第二个公开注册 → 403
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": "stranger-e4@test.local", "password": PW, "orgName": "野组织"}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "公开注册应关闭: {body}");

    // 乱填邀请 → 404（而不是被放过）
    let (st, _, _) = register_with_invite(&app, "fake-e4@test.local", "nope-nope").await;
    assert_eq!(st, StatusCode::NOT_FOUND);

    // 有效邀请仍可注册
    let token = create_invite_token(&app, &owner, "engineer").await;
    let (st, body, _) = register_with_invite(&app, "guest-e4@test.local", &token).await;
    assert_eq!(st, StatusCode::OK, "邀请注册应放行: {body}");
}

// ---------------------------------------------------------------------------
// E4 —— 会话列表 + 退出其他设备
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sessions_list_and_revoke_others() {
    let app = make_app(false, false).await;
    let cookie_a = register(&app, "sam-e4@test.local", "会话公司").await;
    let (st, _, cookie_b) = login(&app, "sam-e4@test.local", PW).await;
    assert_eq!(st, StatusCode::OK);
    let cookie_b = cookie_b.unwrap();

    let (st, sessions, _) = send_json(
        &app,
        "GET",
        "/api/auth/sessions",
        Some(&cookie_a),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let sessions = sessions.as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions.iter().filter(|s| s["current"] == true).count(),
        1,
        "恰有一个当前会话标记"
    );

    // A 上退出其他设备
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/auth/sessions/logout-others",
        Some(&cookie_a),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "退出其他设备: {body}");
    assert_eq!(body["revoked"], 1);

    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_a), json!({})).await;
    assert_eq!(st, StatusCode::OK, "当前会话保留");
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&cookie_b), json!({})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED, "其他设备应已下线");
}

// ---------------------------------------------------------------------------
// E4 —— 多组织成员资格 + 组织切换
// ---------------------------------------------------------------------------

#[tokio::test]
async fn user_can_switch_between_orgs() {
    let app = make_app(false, false).await;

    // alice 先注册认领 org 1
    let alice = register(&app, "alice-org@test.local", "甲组织").await;
    // bob 独立注册 → 自动建 org 2
    let bob = register(&app, "bob-org@test.local", "乙组织").await;

    // bob 邀请 alice 进乙组织；alice 在已登录状态下接受
    let token = create_invite_token(&app, &bob, "engineer").await;
    let (st, body, _) = send_json(
        &app,
        "POST",
        "/api/org/invite/accept",
        Some(&alice),
        json!({"token": token}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "alice 接受邀请: {body}");

    // me.orgs 含两个组织，当前仍在甲
    let (st, me, _) = send_json(&app, "GET", "/api/auth/me", Some(&alice), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    let orgs = me["orgs"].as_array().unwrap();
    assert_eq!(orgs.len(), 2, "alice 应属两个组织: {orgs:?}");
    let cur: Vec<&Value> = orgs.iter().filter(|o| o["current"] == true).collect();
    assert_eq!(cur.len(), 1);
    assert_eq!(cur[0]["orgName"], "甲组织");
    let org2_id = orgs.iter().find(|o| o["orgName"] == "乙组织").unwrap()["orgId"]
        .as_i64()
        .unwrap();

    // 切换到乙组织：旧会话作废、新会话绑定 org2
    let (st, body, new_cookie) = send_json(
        &app,
        "POST",
        "/api/auth/switch-org",
        Some(&alice),
        json!({"orgId": org2_id}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "切换组织: {body}");
    assert_eq!(body["orgId"], org2_id);
    assert_eq!(body["orgName"], "乙组织");
    let new_cookie = new_cookie.expect("应下发新会话 cookie");
    assert_ne!(new_cookie, alice, "切换后应轮换会话 token");

    // 旧会话已失效；新会话在乙组织
    let (st, _, _) = send_json(&app, "GET", "/api/auth/me", Some(&alice), json!({})).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let (st, me2, _) = send_json(&app, "GET", "/api/auth/me", Some(&new_cookie), json!({})).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(me2["orgId"], org2_id);

    // 不能切到不属于的组织：再注册一个丙组织
    register(&app, "carol-org@test.local", "丙组织").await;
    let (st, members, _) = send_json(
        &app,
        "GET",
        "/api/org/members",
        Some(&new_cookie),
        json!({}),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    // 直接拿丙组织 id（甲=1，乙=2，丙=3）
    let (st, _, _) = send_json(
        &app,
        "POST",
        "/api/auth/switch-org",
        Some(&new_cookie),
        json!({"orgId": 3}),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "非成员组织必须 403");
    // 请求体 members 仅用于确认新 cookie 在乙组织上下文（2 人）
    assert!(!members.as_array().unwrap().is_empty());
}
