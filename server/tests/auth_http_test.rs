//! B6c — HTTP 层测试：认证 / 多租户越权 / 图保存 CAS 冲突
//!
//! 与 *_inner 测试互补：这里直接对 `http::router()` 组装好的 axum Router
//! 用 tower::ServiceExt::oneshot 发请求（不起 TCP、不走真实网络），
//! 验证路由守卫、Cookie 会话、错误码映射（401/404/409/422）与响应 camelCase 协议。

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use sif_studio_lib::db::open_in_memory;
use sif_studio_lib::{http, AppConfig, AppState};
use std::sync::Arc;
use tower::ServiceExt; // oneshot

const PW: &str = "password123";
const ALICE: &str = "alice@test.local";
const BOB: &str = "bob@test.local";

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

async fn make_app() -> axum::Router {
    let pool = open_in_memory().await.unwrap();
    http::router(AppState {
        db: Arc::new(pool),
        config: Arc::new(AppConfig::with_admin_token(None)),
        rate: Arc::new(sif_studio_lib::http::ratelimit::RateLimiter::new()),
    })
}

/// 发一个请求，返回 (状态码, JSON 体, Set-Cookie 头)
async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value, Option<String>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        builder = builder.header(COOKIE, c);
    }
    let req = match body {
        Some(v) => builder
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let resp = app.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let set_cookie = resp
        .headers()
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, json, set_cookie)
}

/// 从 Set-Cookie 头取 "sif_session=…" 段作为后续请求的 Cookie 值
fn cookie_of(set_cookie: &Option<String>) -> String {
    set_cookie
        .as_deref()
        .expect("register/login 应下发 Set-Cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

async fn register(app: &axum::Router, email: &str, org_name: &str) -> (Value, String) {
    let (status, body, set_cookie) = send(
        app.clone(),
        "POST",
        "/api/auth/register",
        None,
        Some(json!({
            "email": email,
            "password": PW,
            "displayName": email.split('@').next().unwrap(),
            "orgName": org_name,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "注册应成功: {body}");
    (body, cookie_of(&set_cookie))
}

async fn rpc(app: &axum::Router, cookie: &str, cmd: &str, args: Value) -> (StatusCode, Value) {
    let (status, body, _) = send(
        app.clone(),
        "POST",
        "/api/rpc",
        Some(cookie),
        Some(json!({"cmd": cmd, "args": args})),
    )
    .await;
    (status, body)
}

// ---------------------------------------------------------------------------
// 认证流程
// ---------------------------------------------------------------------------

#[tokio::test]
async fn healthz_is_public() {
    let app = make_app().await;
    let (status, _, _) = send(app, "GET", "/api/healthz", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn rpc_requires_login() {
    let app = make_app().await;
    let (status, body, _) = send(
        app,
        "POST",
        "/api/rpc",
        None,
        Some(json!({"cmd": "list_projects", "args": {}})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["kind"], "unauthorized");
}

#[tokio::test]
async fn me_needs_cookie_and_logout_kills_session() {
    let app = make_app().await;

    let (_, cookie) = register(&app, ALICE, "化一班组").await;

    // 未登录 me → 401
    let (status, body, _) = send(app.clone(), "GET", "/api/auth/me", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["kind"], "unauthorized");

    // 登录态 me → 200
    let (status, body, _) = send(app.clone(), "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], ALICE);
    assert_eq!(body["orgId"], 1);

    // 登出 → Cookie 失效
    let (status, _, _) = send(app.clone(), "POST", "/api/auth/logout", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body, _) = send(app.clone(), "GET", "/api/auth/me", Some(&cookie), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["kind"], "unauthorized");
}

#[tokio::test]
async fn login_rejects_wrong_password() {
    let app = make_app().await;
    register(&app, ALICE, "组织A").await;

    let (status, body, _) = send(
        app.clone(),
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"email": ALICE, "password": "wrong-password"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["kind"], "unauthorized");

    // 正确密码 → 200 + 新会话
    let (status, body, set_cookie) = send(
        app.clone(),
        "POST",
        "/api/auth/login",
        None,
        Some(json!({"email": ALICE, "password": PW})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], ALICE);
    assert!(set_cookie.is_some());
}

#[tokio::test]
async fn register_duplicate_email_conflicts() {
    let app = make_app().await;
    register(&app, ALICE, "组织A").await;

    // 大小写不敏感
    let (status, body, _) = send(
        app.clone(),
        "POST",
        "/api/auth/register",
        None,
        Some(json!({"email": "ALICE@test.local", "password": PW, "orgName": "再次注册"})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["kind"], "conflict");
}

#[tokio::test]
async fn register_validates_password_and_email() {
    let app = make_app().await;

    let (status, body, _) = send(
        app.clone(),
        "POST",
        "/api/auth/register",
        None,
        Some(json!({"email": ALICE, "password": "short", "orgName": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["kind"], "validation");

    let (status, body, _) = send(
        app.clone(),
        "POST",
        "/api/auth/register",
        None,
        Some(json!({"email": "not-an-email", "password": PW, "orgName": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["kind"], "validation");
}

#[tokio::test]
async fn second_user_gets_fresh_org() {
    let app = make_app().await;
    let (alice_me, _) = register(&app, ALICE, "甲组织").await;
    let (bob_me, _) = register(&app, BOB, "乙组织").await;

    assert_eq!(alice_me["orgId"], 1, "首用户认领默认组织");
    assert_ne!(bob_me["orgId"], alice_me["orgId"], "次用户应开新组织");
    assert_eq!(bob_me["orgId"], 2);
    assert_eq!(bob_me["role"], "owner");
    assert_eq!(alice_me["orgName"], "甲组织", "首用户改名默认组织");
    assert_eq!(bob_me["orgName"], "乙组织");
}

// ---------------------------------------------------------------------------
// 多租户越权
// ---------------------------------------------------------------------------

#[tokio::test]
async fn projects_are_isolated_across_orgs() {
    let app = make_app().await;
    let (_, alice_cookie) = register(&app, ALICE, "甲组织").await;
    let (_, bob_cookie) = register(&app, BOB, "乙组织").await;

    // 各建各的项目
    let (status, pa) = rpc(
        &app,
        &alice_cookie,
        "create_project",
        json!({"input": {"code": "PRJ-A", "name": "甲的项目"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pa}");
    let (status, pb) = rpc(
        &app,
        &bob_cookie,
        "create_project",
        json!({"input": {"code": "PRJ-B", "name": "乙的项目"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pb}");
    let pa_id = pa["id"].as_i64().expect("project id");
    let pb_id = pb["id"].as_i64().expect("project id");

    // list 各自只见自己的（003 迁移预置的 PRJ-LEGACY 由首用户 org 1 承接）
    let (_, alice_list) = rpc(&app, &alice_cookie, "list_projects", json!({})).await;
    let (_, bob_list) = rpc(&app, &bob_cookie, "list_projects", json!({})).await;
    let mut alice_codes: Vec<&str> = alice_list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    alice_codes.sort_unstable();
    let bob_codes: Vec<&str> = bob_list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert_eq!(alice_codes, vec!["PRJ-A", "PRJ-LEGACY"]);
    assert_eq!(bob_codes, vec!["PRJ-B"]);

    // 跨 org 读 → 404（不是 403，避免暴露存在性）
    let (status, body) = rpc(&app, &bob_cookie, "get_project", json!({"id": pa_id})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["kind"], "not_found");
    let (status, _) = rpc(&app, &alice_cookie, "get_project", json!({"id": pb_id})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 跨 org 改/删同样不可达
    let (status, _) = rpc(
        &app,
        &bob_cookie,
        "update_project",
        json!({"id": pa_id, "input": {"code": "PRJ-A", "name": "被乙篡改"}}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = rpc(&app, &bob_cookie, "delete_project", json!({"id": pa_id})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 甲的项目安然无恙
    let (status, pa_now) = rpc(&app, &alice_cookie, "get_project", json!({"id": pa_id})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pa_now["name"], "甲的项目");
}

#[tokio::test]
async fn cross_org_rpc_ids_resolve_within_own_org() {
    let app = make_app().await;
    let (_, alice_cookie) = register(&app, ALICE, "甲组织").await;
    let (_, bob_cookie) = register(&app, BOB, "乙组织").await;

    // org2 (bob) 建项目 —— 验证"对方 org 的 id"在本 org 不存在（404），而非串库
    let (status, pa) = rpc(
        &app,
        &alice_cookie,
        "create_project",
        json!({"input": {"code": "PRJ-A", "name": "甲的项目"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let pa_id = pa["id"].as_i64().unwrap();

    // bob 用 alice 的 project id 建图 → 项目归属校验应拒绝（Validation/NotFound）
    let (status, _) = rpc(
        &app,
        &bob_cookie,
        "create_diagram",
        json!({"input": {"projectId": pa_id, "code": "D1", "name": "偷建图"}}),
    )
    .await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::UNPROCESSABLE_ENTITY,
        "跨 org 建图应被归属校验拦截，实得 {status}"
    );

    // bob 自己的项目里建图正常
    let (_, pb) = rpc(
        &app,
        &bob_cookie,
        "create_project",
        json!({"input": {"code": "PRJ-B", "name": "乙的项目"}}),
    )
    .await;
    let (status, _) = rpc(
        &app,
        &bob_cookie,
        "create_diagram",
        json!({"input": {"projectId": pb["id"], "code": "D-B", "name": "乙的图"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 图保存 CAS（乐观锁 409）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn save_diagram_stale_version_conflicts_409() {
    let app = make_app().await;
    let (_, cookie) = register(&app, ALICE, "甲组织").await;

    let (status, pj) = rpc(
        &app,
        &cookie,
        "create_project",
        json!({"input": {"code": "PRJ-CAS", "name": "CAS 测试"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{pj}");

    let (status, dg) = rpc(
        &app,
        &cookie,
        "create_diagram",
        json!({"input": {"projectId": pj["id"], "code": "D1", "name": "第一图"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{dg}");
    let diagram_id = dg["id"].as_i64().unwrap();
    assert_eq!(dg["version"], 0, "新图初始 version=0");

    // 第一次保存：version 0 → 1
    let (status, r1) = rpc(
        &app,
        &cookie,
        "save_diagram_data",
        json!({"id": diagram_id, "version": 0, "data": "{\"nodes\":[1]}"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{r1}");
    assert_eq!(r1["version"], 1);

    // 第二个标签页拿旧版本 0 再存 → 409 conflict
    let (status, r2) = rpc(
        &app,
        &cookie,
        "save_diagram_data",
        json!({"id": diagram_id, "version": 0, "data": "{\"nodes\":[2]}"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(r2["kind"], "conflict");

    // 拿最新版本 1 存 → 成功推进到 2
    let (status, r3) = rpc(
        &app,
        &cookie,
        "save_diagram_data",
        json!({"id": diagram_id, "version": 1, "data": "{\"nodes\":[3]}"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{r3}");
    assert_eq!(r3["version"], 2);

    // get_diagram 读回最新数据与版本
    let (status, got) = rpc(&app, &cookie, "get_diagram", json!({"id": diagram_id})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got["version"], 2);
    assert!(got["data"].as_str().unwrap().contains("3"));
}

#[tokio::test]
async fn diagram_ops_isolated_across_orgs() {
    let app = make_app().await;
    let (_, alice_cookie) = register(&app, ALICE, "甲组织").await;
    let (_, bob_cookie) = register(&app, BOB, "乙组织").await;

    let (_, pj) = rpc(
        &app,
        &alice_cookie,
        "create_project",
        json!({"input": {"code": "PRJ-A", "name": "甲的项目"}}),
    )
    .await;
    let (_, dg) = rpc(
        &app,
        &alice_cookie,
        "create_diagram",
        json!({"input": {"projectId": pj["id"], "code": "D1", "name": "甲的图"}}),
    )
    .await;
    let diagram_id = dg["id"].as_i64().unwrap();

    // bob 读/写甲的图 → 404 / 409 不可达（get 404）
    let (status, _) = rpc(&app, &bob_cookie, "get_diagram", json!({"id": diagram_id})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // bob 的 save 即使猜中 id 也无法覆盖（WHERE org_id 不命中 → NotFound）
    let (status, body) = rpc(
        &app,
        &bob_cookie,
        "save_diagram_data",
        json!({"id": diagram_id, "version": 0, "data": "{}"}),
    )
    .await;
    assert!(
        status == StatusCode::NOT_FOUND || status == StatusCode::CONFLICT,
        "跨 org 保存不应命中，实得 {status} {body}"
    );
}
