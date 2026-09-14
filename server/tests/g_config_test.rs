//! G 阶段 —— 运维安全策略配置化测试：
//!   G1 SIF_SESSION_TTL_DAYS：session 表过期时间与登录 cookie Max-Age 同步生效
//!   G2 SIF_RATE_LIMIT_LOGIN / _REGISTER：限流阈值按配置收紧（默认 10 不变）
//!   G3 SIF_INVITE_TTL_DEFAULT / _MAX：邀请有效期默认值与上限按配置生效
//!
//! 环境变量解析逻辑（parse_num）在 config.rs 单测覆盖；这里直接改 AppConfig
//! 字段（pub），避免并行测试下 env 的进程级竞争。

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

/// 用显式 AppConfig 构建测试应用（不用 env，保证并行安全）。
async fn make_app(cfg: AppConfig) -> (axum::Router, SqlitePool) {
    let pool = open_in_memory().await.unwrap();
    let rate = Arc::new(RateLimiter::with_limits(
        cfg.rate_limit_login,
        cfg.rate_limit_register,
    ));
    let app = http::router(AppState {
        db: Arc::new(pool.clone()),
        config: Arc::new(cfg),
        rate,
    });
    (app, pool)
}

/// 返回完整响应：状态码 + 全部响应头 + JSON 体（cookie 断言需要原始 SET_COOKIE）。
async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    cookie: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
    let req = Request::builder().method(method).uri(uri);
    let req = match cookie {
        Some(c) => req.header(COOKIE, c),
        None => req,
    };
    let req = req
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap()
        .to_vec();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, json)
}

fn set_cookie_of(headers: &HeaderMap) -> String {
    headers
        .get(SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

async fn register(app: &axum::Router, email: &str, org: &str) -> String {
    let (st, headers, body) = send(
        app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": email, "password": PW, "displayName": "测试员", "orgName": org}),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "注册应成功: {body}");
    set_cookie_of(&headers)
}

// ---------------------------------------------------------------------------
// G1 —— 会话有效期
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_ttl_drives_cookie_and_db_expiry() {
    // 自定义 2 天
    let mut cfg = AppConfig::for_test(None, false, false);
    cfg.session_ttl_days = 2;
    let (app, pool) = make_app(cfg).await;

    let (_st, headers, _body) = send(
        &app,
        "POST",
        "/api/auth/register",
        None,
        json!({"email": "ttl2@test.local", "password": PW, "displayName": "测试员", "orgName": "两天公司"}),
    )
    .await;
    let cookie = set_cookie_of(&headers);
    assert!(
        cookie.contains("Max-Age=172800"),
        "cookie Max-Age 应为 2 天（172800 秒）: {cookie}"
    );

    // session 表过期时间 ≈ now + 2 天（julianday 差值，容差 0.01 天 ≈ 14 分钟）
    let diff_days: f64 =
        sqlx::query_scalar("SELECT julianday(expires_at) - julianday('now') FROM session")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        (diff_days - 2.0).abs() < 0.01,
        "session 过期应为 2 天后，实际 {diff_days} 天"
    );

    // 默认 30 天（缺省路径回归）
    let (app30, _p30) = make_app(AppConfig::for_test(None, false, false)).await;
    let cookie30 = register(&app30, "ttl30@test.local", "三十天公司").await;
    assert!(
        cookie30.contains("Max-Age=2592000"),
        "缺省 Max-Age 应为 30 天（2592000 秒）: {cookie30}"
    );
}

// ---------------------------------------------------------------------------
// G2 —— 限流阈值
// ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limit_thresholds_configurable() {
    // 登录阈值收紧到 2：前 2 次 401，第 3 次 429
    let mut cfg = AppConfig::for_test(None, false, false);
    cfg.rate_limit_login = 2;
    let (app, _pool) = make_app(cfg).await;
    register(&app, "brute@test.local", "被爆破公司").await;

    for i in 1..=2 {
        let (st, _, _) = send(
            &app,
            "POST",
            "/api/auth/login",
            None,
            json!({"email": "brute@test.local", "password": "wrong-wrong-1"}),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "第 {i} 次应只是密码错误");
    }
    let (st, _, body) = send(
        &app,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": "brute@test.local", "password": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "第 3 次应被限流: {body}");

    // 默认阈值 10：前 10 次 401，第 11 次 429（缺省路径回归）
    let (app10, _p10) = make_app(AppConfig::for_test(None, false, false)).await;
    register(&app10, "default@test.local", "默认公司").await;
    for i in 1..=10 {
        let (st, _, _) = send(
            &app10,
            "POST",
            "/api/auth/login",
            None,
            json!({"email": "default@test.local", "password": "wrong-wrong-1"}),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED, "第 {i} 次应只是密码错误");
    }
    let (st, _, _) = send(
        &app10,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": "default@test.local", "password": PW}),
    )
    .await;
    assert_eq!(st, StatusCode::TOO_MANY_REQUESTS, "第 11 次应被限流");
}

// ---------------------------------------------------------------------------
// G3 —— 邀请有效期
// ---------------------------------------------------------------------------

async fn create_invite(app: &axum::Router, owner_cookie: &str, body: Value) -> (StatusCode, Value) {
    let (st, _, json) = send(app, "POST", "/api/org/invites", Some(owner_cookie), body).await;
    (st, json)
}

#[tokio::test]
async fn invite_ttl_default_and_max_configurable() {
    let mut cfg = AppConfig::for_test(None, false, false);
    cfg.invite_ttl_default = 3;
    cfg.invite_ttl_max = 5;
    let (app, pool) = make_app(cfg).await;

    let owner_cookie = register(&app, "boss@cfg.local", "配置公司").await;

    // 不带 ttlDays → 默认 3 天
    let (st, body) = create_invite(&app, &owner_cookie, json!({"role": "engineer"})).await;
    assert_eq!(st, StatusCode::OK, "默认 TTL 建邀应成功: {body}");
    let diff: f64 =
        sqlx::query_scalar("SELECT julianday(expires_at) - julianday('now') FROM org_invite")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        (diff - 3.0).abs() < 0.01,
        "默认有效期应为 3 天，实际 {diff}"
    );

    // 超上限 6 天 → 422
    let (st, body) = create_invite(
        &app,
        &owner_cookie,
        json!({"role": "engineer", "ttlDays": 6}),
    )
    .await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["message"].as_str().unwrap().contains("1~5"));

    // 恰好上限 5 天 → 200
    let (st, body) =
        create_invite(&app, &owner_cookie, json!({"role": "viewer", "ttlDays": 5})).await;
    assert_eq!(st, StatusCode::OK, "上限值应被接受: {body}");

    // 缺省配置回归：默认 7 / 上限 30
    let (app30, _p30) = make_app(AppConfig::for_test(None, false, false)).await;
    let c30 = register(&app30, "boss30@cfg.local", "缺省公司").await;
    let (st, body) = create_invite(&app30, &c30, json!({"role": "engineer", "ttlDays": 30})).await;
    assert_eq!(st, StatusCode::OK, "缺省上限 30 天应被接受: {body}");
}
