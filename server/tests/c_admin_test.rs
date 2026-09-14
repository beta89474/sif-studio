//! C 阶段 HTTP 测试：
//!   C1 安全响应头（全站头 + editor.html 差异化 CSP）
//!   C2 备份/恢复（管理员令牌门控 + 跨 app 备份→恢复往返）
//!   C3 legacy 桌面库导入（owner、ID 重映射、幂等、组织隔离）
//!
//! 直接对 http::router() 用 oneshot 发请求（不起 TCP）。

use axum::body::Body;
use axum::http::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, Request, StatusCode};
use serde_json::{json, Value};
use sif_studio_lib::db::open_file;
use sif_studio_lib::{http, AppConfig, AppState};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::sync::Arc;
use tower::ServiceExt;

const PW: &str = "password123";
const ALICE: &str = "alice-c@test.local";
const BOB: &str = "bob-c@test.local";
const TOKEN: &str = "super-secret-admin-token";

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// 文件库起 app（返回 TempDir 守卫——drop 会删库，必须活到测试结束）。
/// 备份/恢复基于 VACUUM INTO，对 :memory: 源库无效，故不能用内存库。
async fn make_app_admin(token: Option<&str>) -> (axum::Router, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = open_file(&dir.path().join("studio.db")).await.unwrap();
    let app = http::router(AppState {
        db: Arc::new(pool),
        config: Arc::new(AppConfig::with_admin_token(token)),
        rate: Arc::new(sif_studio_lib::http::ratelimit::RateLimiter::new()),
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

/// 构造一个最小 multipart/form-data 文件包。
fn multipart_file(field: &str, filename: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    let boundary = "----sifctestboundary";
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

/// 旧 Tauri 桌面版库（001-003 形态：无 org_id/version，含 project_id）。
const OLD_SCHEMA: &str = r#"
CREATE TABLE instrument (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tag TEXT NOT NULL, service TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL,
    role TEXT NOT NULL, psv_id TEXT NOT NULL DEFAULT '',
    manufacturer TEXT NOT NULL DEFAULT '', model TEXT NOT NULL DEFAULT '',
    range_min REAL, range_max REAL, unit TEXT NOT NULL DEFAULT '',
    setpoint REAL, sil_target TEXT NOT NULL DEFAULT 'NA',
    proof_interval INTEGER NOT NULL DEFAULT 0,
    installed_at TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    project_id INTEGER REFERENCES project(id),
    CHECK (role IN ('detector','final','logic','aux')),
    CHECK (sil_target IN ('NA','A','B','C','D'))
);
CREATE UNIQUE INDEX idx_instrument_project_tag ON instrument(project_id, tag);
CREATE TABLE project (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL, name TEXT NOT NULL, client TEXT NOT NULL DEFAULT '',
    location TEXT NOT NULL DEFAULT '', phase TEXT NOT NULL DEFAULT 'design',
    started_at TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at TEXT NOT NULL DEFAULT '', notes TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (phase IN ('design','construction','commissioning','operation','closed'))
);
CREATE UNIQUE INDEX idx_project_code ON project(code);
CREATE TABLE diagram (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    code TEXT NOT NULL, name TEXT NOT NULL, sif_id INTEGER REFERENCES sif(id) ON DELETE SET NULL,
    sheet_size TEXT NOT NULL DEFAULT 'A1', revision TEXT NOT NULL DEFAULT 'A0',
    data TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (sheet_size IN ('A0','A1','A2','A3','A4'))
);
CREATE UNIQUE INDEX idx_diagram_project_code ON diagram(project_id, code);
CREATE TABLE sif (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    code TEXT NOT NULL, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
    sil_design TEXT NOT NULL DEFAULT 'NA', sil_verified TEXT NOT NULL DEFAULT 'NA',
    demand_mode TEXT NOT NULL DEFAULT 'low', pfdavg_target REAL,
    proof_interval INTEGER NOT NULL DEFAULT 12,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (sil_design IN ('NA','A','B','C','D')),
    CHECK (sil_verified IN ('NA','A','B','C','D')),
    CHECK (demand_mode IN ('low','high'))
);
CREATE UNIQUE INDEX idx_sif_project_code ON sif(project_id, code);
CREATE TABLE sif_instrument (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sif_id INTEGER NOT NULL REFERENCES sif(id) ON DELETE CASCADE,
    instrument_id INTEGER NOT NULL REFERENCES instrument(id) ON DELETE CASCADE,
    role TEXT NOT NULL, port_index INTEGER NOT NULL DEFAULT 0,
    diagram_id INTEGER REFERENCES diagram(id) ON DELETE SET NULL,
    note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK (role IN ('detector','final','logic','aux'))
);
CREATE TABLE bypass_record (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    sif_id INTEGER NOT NULL REFERENCES sif(id) ON DELETE CASCADE,
    bypassed_by TEXT NOT NULL DEFAULT '', bypassed_at TEXT NOT NULL DEFAULT (datetime('now')),
    reason TEXT NOT NULL DEFAULT '', planned_restore TEXT NOT NULL DEFAULT '',
    restored_at TEXT NOT NULL DEFAULT '', permit_no TEXT NOT NULL DEFAULT '',
    approved_by TEXT NOT NULL DEFAULT ''
);
CREATE TABLE service_ticket (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    sif_id INTEGER, instrument_id INTEGER,
    kind TEXT NOT NULL DEFAULT 'maintenance', title TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT '', opened_by TEXT NOT NULL DEFAULT '',
    opened_at TEXT NOT NULL DEFAULT (datetime('now')), closed_at TEXT NOT NULL DEFAULT '',
    severity TEXT NOT NULL DEFAULT 'normal',
    CHECK (kind IN ('maintenance','inspection','failure','audit')),
    CHECK (severity IN ('normal','warning','critical'))
);
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL DEFAULT (datetime('now')), actor TEXT NOT NULL DEFAULT '',
    action TEXT NOT NULL, target_table TEXT NOT NULL, target_id INTEGER,
    payload_json TEXT NOT NULL DEFAULT '{}', note TEXT NOT NULL DEFAULT ''
);
INSERT INTO project (id, code, name) VALUES (1, 'PRJ-OLD', '旧桌面项目');
INSERT INTO sif (id, project_id, code, name) VALUES (1, 1, 'SIF-900', '旧联锁');
INSERT INTO instrument (id, project_id, tag, kind, role, service)
    VALUES (1, 1, 'PT-900', 'PT', 'detector', '旧容器压力');
INSERT INTO diagram (id, project_id, sif_id, code, name, data)
    VALUES (1, 1, 1, 'DGM-900', '旧图', '{"legacy":true}');
INSERT INTO sif_instrument (sif_id, instrument_id, role, port_index, diagram_id)
    VALUES (1, 1, 'detector', 0, 1);
INSERT INTO bypass_record (project_id, sif_id, bypassed_by, reason, planned_restore)
    VALUES (1, 1, '王工', '旧库旁路记录', '2026-10-01T00:00:00Z');
INSERT INTO service_ticket (project_id, kind, title, opened_by)
    VALUES (1, 'maintenance', '旧工单', '王工');
INSERT INTO audit_log (ts, actor, action, target_table, target_id)
    VALUES ('2026-09-01 00:00:00', '王工', 'create', 'instrument', 1);
"#;

/// 造一个旧桌面库文件，返回其字节。
async fn make_legacy_db_bytes() -> Vec<u8> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-desktop.db");
    let pool = SqlitePoolOptions::new()
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    sqlx::raw_sql(OLD_SCHEMA).execute(&pool).await.unwrap();
    pool.close().await;
    tokio::fs::read(&path).await.unwrap()
}

// ---------------------------------------------------------------------------
// C1 —— 安全响应头
// ---------------------------------------------------------------------------

#[tokio::test]
async fn security_headers_on_api_and_strict_csp() {
    let (app, _tmp) = make_app_admin(None).await;
    let (status, h, _) = send_raw(app, "GET", "/api/healthz", None, &[], None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        h.get("x-content-type-options").unwrap().to_str().unwrap(),
        "nosniff"
    );
    assert_eq!(
        h.get("x-frame-options").unwrap().to_str().unwrap(),
        "SAMEORIGIN"
    );
    assert_eq!(
        h.get("referrer-policy").unwrap().to_str().unwrap(),
        "same-origin"
    );
    let csp = h.get("content-security-policy").unwrap().to_str().unwrap();
    assert!(csp.contains("frame-ancestors 'self'"), "{csp}");
    assert!(csp.contains("object-src 'none'"), "{csp}");
    // 主应用策略不得放行内联脚本
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "主应用 CSP 不应放行 unsafe-inline 脚本: {csp}"
    );
}

#[tokio::test]
async fn editor_page_gets_inline_friendly_csp() {
    let (app, _tmp) = make_app_admin(None).await;
    let (status, h, body) = send_raw(app, "GET", "/editor.html", None, &[], None, None).await;
    assert_eq!(status, StatusCode::OK, "public/editor.html 应可静态访问");
    assert!(!body.is_empty());
    let csp = h.get("content-security-policy").unwrap().to_str().unwrap();
    assert!(csp.contains("script-src 'self' 'unsafe-inline'"), "{csp}");
    // 即便放行内联脚本，仍禁止外站框架
    assert!(csp.contains("frame-ancestors 'self'"), "{csp}");
}

// ---------------------------------------------------------------------------
// C2 —— 备份 / 恢复
// ---------------------------------------------------------------------------

#[tokio::test]
async fn backup_requires_and_validates_admin_token() {
    // 未配置令牌 → 404（端点对外不可见）
    let (app, _tmp) = make_app_admin(None).await;
    let (status, _, _) = send_raw(app, "GET", "/api/admin/backup", None, &[], None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 配置了但不带/带错令牌 → 401
    let (app, _tmp) = make_app_admin(Some(TOKEN)).await;
    let (status, _, _) = send_raw(
        app.clone(),
        "GET",
        "/api/admin/backup",
        None,
        &[],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = send_raw(
        app,
        "GET",
        "/api/admin/backup",
        None,
        &[("x-admin-token", "wrong")],
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn backup_then_restore_roundtrip_between_apps() {
    // ── app A：注册 + 建一个项目 ──
    let (app_a, _tmp_a) = make_app_admin(Some(TOKEN)).await;
    let cookie_a = register(&app_a, ALICE, "甲组织").await;
    let (status, _) = rpc(
        &app_a,
        &cookie_a,
        "create_project",
        json!({"input": {"code": "PRJ-X", "name": "备份往返项目"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ── 备份（正确令牌）──
    let (status, h, bytes) = send_raw(
        app_a,
        "GET",
        "/api/admin/backup",
        None,
        &[("x-admin-token", TOKEN)],
        None,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "backup 500 body: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(
        h.get("content-type").unwrap().to_str().unwrap(),
        "application/vnd.sqlite3"
    );
    let disposition = h.get("content-disposition").unwrap().to_str().unwrap();
    assert!(disposition.contains("sif-studio-backup-"), "{disposition}");
    assert_eq!(&bytes[..16], b"SQLite format 3\0");

    // ── app B（全新空库）：恢复 ──
    let (app_b, _tmp_b) = make_app_admin(Some(TOKEN)).await;
    let (ct, body) = multipart_file("file", "restore.db", &bytes);
    let (status, h2, resp) = send_raw(
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
        status,
        StatusCode::OK,
        "恢复应成功: {}",
        String::from_utf8_lossy(&resp)
    );
    let report: Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(report["restored"], true);
    assert!(report["rowsCopied"]["project"].as_i64().unwrap() >= 1);
    let _ = h2;

    // 恢复后能用原账号登录（user 表整表回灌），且看到备份里的项目
    let (status, body, set_cookie) = send_json(
        &app_b,
        "POST",
        "/api/auth/login",
        None,
        json!({"email": ALICE, "password": PW}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "恢复后应可登录: {body}");
    let cookie_b = set_cookie.unwrap().split(';').next().unwrap().to_string();
    let (status, list) = rpc(&app_b, &cookie_b, "list_projects", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let codes: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"PRJ-X"), "恢复后应见到 PRJ-X: {codes:?}");
}

#[tokio::test]
async fn restore_rejects_non_sqlite_and_bad_token() {
    let (app, _tmp) = make_app_admin(Some(TOKEN)).await;
    let (ct, body) = multipart_file(
        "file",
        "junk.db",
        b"this is definitely not a sqlite database",
    );
    let (status, _, resp) = send_raw(
        app,
        "POST",
        "/api/admin/restore",
        None,
        &[("x-admin-token", TOKEN)],
        Some(&ct),
        Some(body),
    )
    .await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "垃圾文件应被拒，实际 {status}: {}",
        String::from_utf8_lossy(&resp)
    );

    // 无令牌 → 401
    let (app2, _tmp2) = make_app_admin(Some(TOKEN)).await;
    let (ct, body) = multipart_file("file", "x.db", b"x");
    let (status, _, _) = send_raw(
        app2,
        "POST",
        "/api/admin/restore",
        None,
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// ---------------------------------------------------------------------------
// C3 —— legacy 桌面库导入
// ---------------------------------------------------------------------------

#[tokio::test]
async fn legacy_import_requires_login() {
    let (app, _tmp) = make_app_admin(None).await;
    let bytes = make_legacy_db_bytes().await;
    let (ct, body) = multipart_file("file", "legacy.db", &bytes);
    let (status, _, _) = send_raw(
        app,
        "POST",
        "/api/admin/import-legacy",
        None,
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn legacy_import_remaps_ids_and_is_idempotent() {
    let (app, _tmp) = make_app_admin(None).await;
    let cookie = register(&app, ALICE, "甲组织").await;
    let legacy = make_legacy_db_bytes().await;

    // 第一次：8 类数据各导入 1 行
    let (ct, body) = multipart_file("file", "legacy.db", &legacy);
    let (status, _, resp) = send_raw(
        app.clone(),
        "POST",
        "/api/admin/import-legacy",
        Some(&cookie),
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "首次导入: {}",
        String::from_utf8_lossy(&resp)
    );
    let report: Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(report["projects"]["inserted"], 1, "{report}");
    assert_eq!(report["instruments"]["inserted"], 1, "{report}");
    assert_eq!(report["sifs"]["inserted"], 1, "{report}");
    assert_eq!(report["diagrams"]["inserted"], 1, "{report}");
    assert_eq!(report["links"]["inserted"], 1, "{report}");
    assert_eq!(report["bypasses"]["inserted"], 1, "{report}");
    assert_eq!(report["tickets"]["inserted"], 1, "{report}");
    assert_eq!(report["auditLogs"]["inserted"], 1, "{report}");
    assert_eq!(report["orphanRows"], 0, "{report}");

    // 数据落在 alice 组织，外键重映射后可经业务 RPC 读到
    let (_, projects) = rpc(&app, &cookie, "list_projects", json!({})).await;
    let codes: Vec<&str> = projects
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"PRJ-OLD"), "{codes:?}");
    let (_, instruments) = rpc(&app, &cookie, "list_instruments", json!({})).await;
    let tags: Vec<&str> = instruments
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["tag"].as_str().unwrap())
        .collect();
    assert!(tags.contains(&"PT-900"), "{tags:?}");

    // 第二次：全部跳过，不产生重复
    let (ct, body) = multipart_file("file", "legacy.db", &legacy);
    let (status, _, resp) = send_raw(
        app.clone(),
        "POST",
        "/api/admin/import-legacy",
        Some(&cookie),
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let report2: Value = serde_json::from_slice(&resp).unwrap();
    assert_eq!(report2["projects"]["inserted"], 0, "{report2}");
    assert_eq!(report2["projects"]["skipped"], 1, "{report2}");
    assert_eq!(report2["instruments"]["inserted"], 0, "{report2}");
    assert_eq!(report2["links"]["inserted"], 0, "{report2}");
    let (_, instruments2) = rpc(&app, &cookie, "list_instruments", json!({})).await;
    assert_eq!(instruments2.as_array().unwrap().len(), 1, "仪表不应重复");
}

#[tokio::test]
async fn legacy_import_stays_within_callers_org() {
    let (app, _tmp) = make_app_admin(None).await;

    // alice 导入 + 再建一个她独有的项目
    let cookie_a = register(&app, ALICE, "甲组织").await;
    let legacy = make_legacy_db_bytes().await;
    let (ct, body) = multipart_file("file", "legacy.db", &legacy);
    let (status, _, _) = send_raw(
        app.clone(),
        "POST",
        "/api/admin/import-legacy",
        Some(&cookie_a),
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, _) = rpc(
        &app,
        &cookie_a,
        "create_project",
        json!({"input": {"code": "PRJ-A-ONLY", "name": "甲独有"}}),
    )
    .await;

    // bob 是另一组织：导入同一份旧库，得到自己的副本
    let cookie_b = register(&app, BOB, "乙组织").await;
    let (ct, body) = multipart_file("file", "legacy.db", &legacy);
    let (status, _, _) = send_raw(
        app.clone(),
        "POST",
        "/api/admin/import-legacy",
        Some(&cookie_b),
        &[],
        Some(&ct),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (_, list_b) = rpc(&app, &cookie_b, "list_projects", json!({})).await;
    let codes_b: Vec<&str> = list_b
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["code"].as_str().unwrap())
        .collect();
    assert!(
        codes_b.contains(&"PRJ-OLD"),
        "bob 有自己的旧库副本: {codes_b:?}"
    );
    assert!(
        !codes_b.contains(&"PRJ-A-ONLY"),
        "bob 不得见 alice 独有项目: {codes_b:?}"
    );
}
