//! 阶段 C2/C3 —— 整库备份 / 恢复 / legacy 桌面库导入
//!
//! ## 备份（服务器操作员）
//! `VACUUM INTO '<tmp>'` 在单条 SQL 内产出事务一致的快照文件（自动处理 WAL
//! checkpoint），随后以附件下载。备份内容是**全组织**的，HTTP 层用
//! SIF_ADMIN_TOKEN 限定为服务器操作员。
//!
//! ## 恢复（服务器操作员）
//! 上传任意同代或更早版本备份 → 只读完整性预检 → **复制到临时可写文件并把
//! 副本前向迁移到当前迁移版本**（旧备份可随服务升级继续使用）→ ATTACH 进当前
//! 库，在单连接单事务里按"子表先删、父表先灌"的顺序对 13 张表（含
//! org/user/session/org_invite）整表回灌（外键约束全程保持开启），再
//! foreign_key_check 兜底；任何异常随事务析构回滚，现网数据不受影响。
//!
//! ## legacy 导入（组织 owner）
//! 旧 Tauri 桌面版 studio.db（无 org_id、id 是另一套命名空间）上传后：
//! - 只导业务表，不导 user/org/session（账号体系不混用）；
//! - 所有主键/外键**重新映射**，冲突按业务唯一键跳过 → 可重复执行；
//! - 全部行挂到调用者组织。
//!
//! 安全要点：
//! - 上传原件只读打开预检，绝不写入；需要迁移时迁移的是临时副本；
//! - 路径进入 SQL 前按 SQLite 字符串字面量规则转义（防注入）；
//! - 回灌全过程在事务内，失败整体回滚。

use chrono::Utc;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::{AppError, AppResult};

// ---------------------------------------------------------------------------
// 通用 helpers
// ---------------------------------------------------------------------------

/// 把文件路径转成 SQLite 字符串字面量（' 翻倍）。
fn sql_path_lit(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\'', "''");
    format!("'{s}'")
}

/// read-only 打开一个外部 SQLite 文件（不经过 URL 解析，避开 Windows 盘符歧义）。
async fn open_readonly(path: &Path) -> AppResult<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .foreign_keys(false)
        .busy_timeout(std::time::Duration::from_secs(5));
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|e| {
            AppError::BadRequest(format!(
                "无法打开上传的数据库文件（不是有效的 SQLite 库？）: {e}"
            ))
        })
}

/// 读外部上传库时的底层 SQL 错误（典型：code 26 not a database）归类为
/// 422"文件不可读"，避免冒泡成 500；已是业务错误的原样保留。
fn upload_not_readable(e: AppError) -> AppError {
    match e {
        AppError::Sqlx(inner) => AppError::BadRequest(format!(
            "无法读取上传的数据库文件（已损坏或不是有效的 SQLite 库）：{inner}"
        )),
        other => other,
    }
}

/// 某张表在库里是否存在。
async fn table_exists(pool: &SqlitePool, name: &str) -> AppResult<bool> {
    let n: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?1")
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(n > 0)
}

/// 取一张表的列名集合（兼容旧桌面库缺列）。
async fn columns_of(pool: &SqlitePool, table: &str) -> AppResult<HashSet<String>> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})"))
        .fetch_all(pool)
        .await?;
    Ok(rows.iter().map(|r| r.get::<String, _>("name")).collect())
}

/// `PRAGMA integrity_check`（restore 用，严格）/ quick_check（legacy 用，快）。
async fn pragma_check(pool: &SqlitePool, quick: bool) -> AppResult<()> {
    let sql = if quick {
        "PRAGMA quick_check"
    } else {
        "PRAGMA integrity_check"
    };
    let result: String = sqlx::query_scalar(sql).fetch_one(pool).await?;
    if result != "ok" {
        return Err(AppError::BadRequest(format!(
            "数据库完整性校验失败：{result}"
        )));
    }
    Ok(())
}

/// 当前库已应用的迁移版本集合（_sqlx_migrations）。
async fn migration_versions(pool: &SqlitePool) -> AppResult<Vec<i64>> {
    if !table_exists(pool, "_sqlx_migrations").await? {
        return Ok(vec![]);
    }
    Ok(sqlx::query_scalar(
        "SELECT version FROM _sqlx_migrations WHERE success = 1 ORDER BY version",
    )
    .fetch_all(pool)
    .await?)
}

// ---------------------------------------------------------------------------
// C2 —— 备份
// ---------------------------------------------------------------------------

/// VACUUM INTO 一致性快照到 dest（dest 必须尚不存在）。
pub async fn backup_to_file(pool: &SqlitePool, dest: &Path) -> AppResult<()> {
    let sql = format!("VACUUM INTO {}", sql_path_lit(dest));
    sqlx::query(&sql)
        .execute(pool)
        .await
        .map_err(|e| AppError::BadRequest(format!("VACUUM INTO 备份失败: {e}")))?;
    Ok(())
}

/// 备份附件文件名（UTC 时间戳）。
pub fn backup_filename() -> String {
    format!(
        "sif-studio-backup-{}.db",
        Utc::now().format("%Y%m%d-%H%M%S")
    )
}

/// 当前库已应用的最新迁移版本（恢复时与备份做同版本校验）。
pub async fn latest_migration_version(pool: &SqlitePool) -> AppResult<i64> {
    Ok(migration_versions(pool).await?.last().copied().unwrap_or(0))
}

// ---------------------------------------------------------------------------
// C2 —— 恢复
// ---------------------------------------------------------------------------

/// 恢复必需的 13 张表（与迁移 001-005 对应）。
/// 顺序即回灌顺序（父表在前）；清表时反向迭代（子表在前），
/// 这样全过程外键约束保持开启也不冲突，无需 PRAGMA foreign_keys=OFF。
const RESTORE_TABLES: [&str; 13] = [
    "org",
    "user",
    "org_member",
    "session",
    "org_invite",
    "project",
    "sif",
    "instrument",
    "diagram",
    "sif_instrument",
    "bypass_record",
    "service_ticket",
    "audit_log",
];

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: bool,
    pub migration_version: i64,
    pub tables_copied: usize,
    pub rows_copied: HashMap<String, i64>,
}

/// 迁移后校验备份：完整性 OK + 13 张表齐全；返回已应用的最新迁移版本。
async fn validate_backup(pool: &SqlitePool) -> AppResult<i64> {
    pragma_check(pool, false)
        .await
        .map_err(upload_not_readable)?;

    for t in RESTORE_TABLES {
        if !table_exists(pool, t).await.map_err(upload_not_readable)? {
            return Err(AppError::BadRequest(format!(
                "备份文件缺少必需表 `{t}`，可能不是 SIF Studio 备份"
            )));
        }
    }

    let latest = migration_versions(pool)
        .await
        .map_err(upload_not_readable)?
        .last()
        .copied()
        .unwrap_or(0);
    Ok(latest)
}

/// 整张表回灌（显式列名，避免列序依赖）。返回复制行数。
macro_rules! copy_table {
    ($conn:expr, $cols:literal, $table:expr) => {{
        let sql = concat!(
            "INSERT INTO main.",
            $table,
            " (",
            $cols,
            ") SELECT ",
            $cols,
            " FROM src.",
            $table
        );
        let n = sqlx::query(sql).execute(&mut *$conn).await?.rows_affected() as i64;
        n
    }};
}

/// 事务内清库 + 回灌。独立 async fn（不能是内嵌 `async {}`）：那会借用
/// `&mut conn` 生成不满足 HRTB Send 的 Future，axum handler 编译报
/// "Executor is not general enough"；sqlx 的 raw_sql 在 &mut 连接上有
/// 同样问题，故 BEGIN/COMMIT 一律走 Connection::begin / Transaction。
async fn copy_restore_tables(
    conn: &mut sqlx::SqliteConnection,
    rows_copied: &mut HashMap<String, i64>,
) -> AppResult<()> {
    // sqlx 事务管理器自己发 BEGIN IMMEDIATE（worker 通道，Future: Send）。
    let mut tx = sqlx::Connection::begin(conn).await?;

    // 先清空：子表 → 父表（外键全程保持开启）。
    for t in RESTORE_TABLES.into_iter().rev() {
        sqlx::query(&format!("DELETE FROM main.{t}"))
            .execute(&mut *tx)
            .await?;
    }

    // 回灌（父表 → 子表，列名与 001-004 schema 一一对应）
    rows_copied.insert("org".into(), copy_table!(tx, "id, name, created_at", "org"));
    rows_copied.insert(
        "user".into(),
        copy_table!(
            tx,
            "id, email, password_hash, display_name, created_at",
            "user"
        ),
    );
    rows_copied.insert(
        "org_member".into(),
        copy_table!(tx, "org_id, user_id, role, created_at", "org_member"),
    );
    rows_copied.insert(
        "session".into(),
        copy_table!(
            tx,
            "token, user_id, org_id, created_at, last_seen_at, expires_at, user_agent, ip",
            "session"
        ),
    );
    rows_copied.insert(
        "org_invite".into(),
        copy_table!(
            tx,
            "token, org_id, role, invited_by, created_at, expires_at, revoked_at",
            "org_invite"
        ),
    );
    rows_copied.insert(
        "project".into(),
        copy_table!(
            tx,
            "id, code, name, client, location, phase, started_at, finished_at, notes, created_at, updated_at, org_id",
            "project"
        ),
    );
    rows_copied.insert(
        "sif".into(),
        copy_table!(
            tx,
            "id, project_id, code, name, description, sil_design, sil_verified, demand_mode, pfdavg_target, proof_interval, created_at, updated_at, org_id",
            "sif"
        ),
    );
    rows_copied.insert(
        "instrument".into(),
        copy_table!(
            tx,
            "id, tag, service, kind, role, psv_id, manufacturer, model, range_min, range_max, unit, setpoint, sil_target, proof_interval, installed_at, notes, created_at, updated_at, project_id, org_id",
            "instrument"
        ),
    );
    rows_copied.insert(
        "diagram".into(),
        copy_table!(
            tx,
            "id, project_id, code, name, sif_id, sheet_size, revision, data, updated_at, created_at, org_id, version",
            "diagram"
        ),
    );
    rows_copied.insert(
        "sif_instrument".into(),
        copy_table!(
            tx,
            "id, sif_id, instrument_id, role, port_index, diagram_id, note, created_at, org_id",
            "sif_instrument"
        ),
    );
    rows_copied.insert(
        "bypass_record".into(),
        copy_table!(
            tx,
            "id, project_id, sif_id, bypassed_by, bypassed_at, reason, planned_restore, restored_at, permit_no, approved_by, org_id",
            "bypass_record"
        ),
    );
    rows_copied.insert(
        "service_ticket".into(),
        copy_table!(
            tx,
            "id, project_id, sif_id, instrument_id, kind, title, detail, opened_by, opened_at, closed_at, severity, org_id",
            "service_ticket"
        ),
    );
    rows_copied.insert(
        "audit_log".into(),
        copy_table!(
            tx,
            "id, ts, actor, action, target_table, target_id, payload_json, note, org_id",
            "audit_log"
        ),
    );

    // AUTOINCREMENT 序列表（若备份里有）
    let seq_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM src.sqlite_master WHERE type='table' AND name='sqlite_sequence'",
    )
    .fetch_one(&mut *tx)
    .await?;
    if seq_exists > 0 {
        sqlx::query("DELETE FROM main.sqlite_sequence")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO main.sqlite_sequence(name, seq) SELECT name, seq FROM src.sqlite_sequence",
        )
        .execute(&mut *tx)
        .await?;
    }

    // 迁移账本
    sqlx::query("DELETE FROM main._sqlx_migrations")
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO main._sqlx_migrations(version, description, installed_on, success, checksum, execution_time)
         SELECT version, description, installed_on, success, checksum, execution_time FROM src._sqlx_migrations",
    )
    .execute(&mut *tx)
    .await?;

    // 提交前外键一致性兜底（PRAGMA 可在事务内跑）；有问题 tx 随 Err 析构回滚。
    let fk_issues = sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *tx)
        .await?;
    if !fk_issues.is_empty() {
        return Err(AppError::BadRequest(format!(
            "恢复后外键一致性检查发现 {} 处问题，已回滚",
            fk_issues.len()
        )));
    }

    tx.commit().await?;
    Ok(())
}

/// 从备份文件整库恢复。调用方负责：已鉴权（服务器操作员）、文件已落临时目录。
///
/// 旧版本备份会先在**临时副本**上前向迁移到当前版本，再回灌——上传原件不被
/// 修改。比当前服务更新的备份拒绝恢复，避免账本错乱。
pub async fn restore_from_file(
    main_pool: &SqlitePool,
    src_path: &Path,
    expected_latest: i64,
) -> AppResult<RestoreReport> {
    // 1) 只读预检：垃圾文件在此 422 出局（不触碰上传原件）。
    let pre = open_readonly(src_path).await?;
    pragma_check(&pre, false)
        .await
        .map_err(upload_not_readable)?;
    pre.close().await;

    // 2) 复制为可写临时文件，在副本上跑前向迁移。
    let dir = tempfile::tempdir()?;
    let migrated_path = dir.path().join("restore-migrated.db");
    std::fs::copy(src_path, &migrated_path)?;

    let opts = SqliteConnectOptions::new()
        .filename(&migrated_path)
        .create_if_missing(false)
        .foreign_keys(false)
        // 必须用 rollback journal：主库随后直接 ATTACH 这个文件，WAL 模式下
        // 未 checkpoint 的页在 -wal 侧车文件里，ATTACH 会漏读。
        .journal_mode(SqliteJournalMode::Delete)
        .busy_timeout(std::time::Duration::from_secs(5));
    let up_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|e| upload_not_readable(AppError::Sqlx(e)))?;

    let before = latest_migration_version(&up_pool).await?;
    if before < expected_latest {
        crate::db::MIGRATOR
            .run(&up_pool)
            .await
            .map_err(|e| AppError::BadRequest(format!("备份升级到当前版本失败：{e}")))?;
    } else if before > expected_latest {
        return Err(AppError::BadRequest(format!(
            "备份来自更新版本（v{before}），当前服务仅 v{expected_latest}，请先升级服务"
        )));
    }

    // 3) 迁移后完整性 + 13 表齐全 + 版本一致校验。
    let src_latest = validate_backup(&up_pool).await?;
    if src_latest != expected_latest {
        return Err(AppError::BadRequest(format!(
            "备份迁移版本 v{src_latest} 与当前服务 v{expected_latest} 不一致"
        )));
    }
    // 写连接必须先关闭，确保 journal 全部落盘后再让主库 ATTACH 同一文件。
    up_pool.close().await;

    // 4) ATTACH 副本 + 单事务回灌。
    let mut rows_copied: HashMap<String, i64> = HashMap::new();
    let mut conn = main_pool.acquire().await?;

    // ATTACH 必须在事务外（DETACH 同理）；路径字面量已做 ' 翻倍转义。
    sqlx::query(&format!(
        "ATTACH DATABASE {} AS src",
        sql_path_lit(&migrated_path)
    ))
    .execute(&mut *conn)
    .await?;

    // 事务在 helper 内已提交（失败则随 tx 析构回滚）；连接回来后 DETACH。
    let copy_result = copy_restore_tables(&mut conn, &mut rows_copied).await;
    if let Err(e) = copy_result {
        let _ = sqlx::query("DETACH DATABASE src").execute(&mut *conn).await;
        return Err(e);
    }
    sqlx::query("DETACH DATABASE src")
        .execute(&mut *conn)
        .await?;

    let tables_copied = rows_copied.len();
    Ok(RestoreReport {
        restored: true,
        migration_version: src_latest,
        tables_copied,
        rows_copied,
    })
}

// ---------------------------------------------------------------------------
// C3 —— legacy 桌面库导入
// ---------------------------------------------------------------------------

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyTableReport {
    pub inserted: i64,
    pub skipped: i64,
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegacyReport {
    pub projects: LegacyTableReport,
    pub instruments: LegacyTableReport,
    pub sifs: LegacyTableReport,
    pub diagrams: LegacyTableReport,
    pub links: LegacyTableReport,
    pub bypasses: LegacyTableReport,
    pub tickets: LegacyTableReport,
    pub audit_logs: LegacyTableReport,
    /// 找不到归属项目而整体丢弃的仪表/SIF/图…数量（理论上不应出现）
    pub orphan_rows: i64,
}

/// 把上传的旧桌面库导入指定组织。可重复执行（按业务唯一键跳过）。
pub async fn import_legacy_db(
    main_pool: &SqlitePool,
    org_id: i64,
    _actor: &str,
    src_path: &Path,
) -> AppResult<LegacyReport> {
    let src = open_readonly(src_path).await?;
    pragma_check(&src, true)
        .await
        .map_err(upload_not_readable)?;

    for t in ["project", "instrument", "sif", "diagram"] {
        if !table_exists(&src, t).await.map_err(upload_not_readable)? {
            return Err(AppError::BadRequest(format!(
                "旧库缺少必需表 `{t}`，无法导入"
            )));
        }
    }
    // 审计/旁路/工单/link 表按「有则导、无则跳过」处理（极早期桌面库可能没有）。

    let inst_cols = columns_of(&src, "instrument")
        .await
        .map_err(upload_not_readable)?;
    let inst_has_project = inst_cols.contains("project_id");
    let mut report = LegacyReport::default();

    let mut tx = main_pool.begin().await?;

    // ── 1. 项目：按 (org, code) 去重 ──────────────────────────────────────────
    let mut project_map: HashMap<i64, i64> = HashMap::new();
    let rows = sqlx::query(
        "SELECT id, code, name, client, location, phase, started_at, finished_at, notes,
                created_at, updated_at
           FROM project ORDER BY id",
    )
    .fetch_all(&src)
    .await?;
    for r in rows {
        let old_id: i64 = r.get("id");
        let code: String = r.get("code");
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT id FROM project WHERE org_id = ?1 AND code = ?2")
                .bind(org_id)
                .bind(&code)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(new_id) = exists {
            project_map.insert(old_id, new_id);
            report.projects.skipped += 1;
            continue;
        }
        let new_id = sqlx::query(
            "INSERT INTO project
                (code, name, client, location, phase, started_at, finished_at, notes,
                 created_at, updated_at, org_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )
        .bind(r.get::<String, _>("code"))
        .bind(r.get::<String, _>("name"))
        .bind(r.get::<String, _>("client"))
        .bind(r.get::<String, _>("location"))
        .bind(r.get::<String, _>("phase"))
        .bind(r.get::<String, _>("started_at"))
        .bind(r.get::<String, _>("finished_at"))
        .bind(r.get::<String, _>("notes"))
        .bind(r.get::<String, _>("created_at"))
        .bind(r.get::<String, _>("updated_at"))
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        project_map.insert(old_id, new_id);
        report.projects.inserted += 1;
    }

    // ── 2. 仪表：按 (新项目, tag) 去重；project_id 列缺时视为孤儿跳过 ─────────
    let mut instrument_map: HashMap<i64, i64> = HashMap::new();
    // 003 之前的桌面库没有 project_id 列：动态拼 SELECT，缺列按 NULL 处理。
    let inst_select = if inst_has_project {
        "SELECT id, tag, service, kind, role, psv_id, manufacturer, model, range_min,
                range_max, unit, setpoint, sil_target, proof_interval, installed_at, notes,
                created_at, updated_at, project_id
           FROM instrument ORDER BY id"
    } else {
        "SELECT id, tag, service, kind, role, psv_id, manufacturer, model, range_min,
                range_max, unit, setpoint, sil_target, proof_interval, installed_at, notes,
                created_at, updated_at, NULL AS project_id
           FROM instrument ORDER BY id"
    };
    let rows = sqlx::query(inst_select).fetch_all(&src).await?;
    for r in rows {
        let old_id: i64 = r.get("id");
        let old_pid: Option<i64> = r.get("project_id");
        let new_pid = match old_pid.and_then(|p| project_map.get(&p).copied()) {
            Some(p) => p,
            None => {
                report.orphan_rows += 1;
                report.instruments.skipped += 1;
                continue;
            }
        };
        let tag: String = r.get("tag");
        if sqlx::query_scalar::<_, i64>(
            "SELECT 1 FROM instrument WHERE project_id = ?1 AND tag = ?2",
        )
        .bind(new_pid)
        .bind(&tag)
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
        {
            report.instruments.skipped += 1;
            // 旧 link 仍可能要引用它：补建映射
            let new_id: i64 =
                sqlx::query_scalar("SELECT id FROM instrument WHERE project_id = ?1 AND tag = ?2")
                    .bind(new_pid)
                    .bind(&tag)
                    .fetch_one(&mut *tx)
                    .await?;
            instrument_map.insert(old_id, new_id);
            continue;
        }
        let new_id = sqlx::query(
            "INSERT INTO instrument
                (tag, service, kind, role, psv_id, manufacturer, model, range_min, range_max,
                 unit, setpoint, sil_target, proof_interval, installed_at, notes,
                 created_at, updated_at, project_id, org_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        )
        .bind(r.get::<String, _>("tag"))
        .bind(r.get::<String, _>("service"))
        .bind(r.get::<String, _>("kind"))
        .bind(r.get::<String, _>("role"))
        .bind(r.get::<String, _>("psv_id"))
        .bind(r.get::<String, _>("manufacturer"))
        .bind(r.get::<String, _>("model"))
        .bind(r.get::<Option<f64>, _>("range_min"))
        .bind(r.get::<Option<f64>, _>("range_max"))
        .bind(r.get::<String, _>("unit"))
        .bind(r.get::<Option<f64>, _>("setpoint"))
        .bind(r.get::<String, _>("sil_target"))
        .bind(r.get::<i64, _>("proof_interval"))
        .bind(r.get::<String, _>("installed_at"))
        .bind(r.get::<String, _>("notes"))
        .bind(r.get::<String, _>("created_at"))
        .bind(r.get::<String, _>("updated_at"))
        .bind(new_pid)
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        instrument_map.insert(old_id, new_id);
        report.instruments.inserted += 1;
    }

    // ── 3. SIF：按 (新项目, code) 去重 ────────────────────────────────────────
    let mut sif_map: HashMap<i64, i64> = HashMap::new();
    let rows = sqlx::query(
        "SELECT id, project_id, code, name, description, sil_design, sil_verified,
                demand_mode, pfdavg_target, proof_interval, created_at, updated_at
           FROM sif ORDER BY id",
    )
    .fetch_all(&src)
    .await?;
    for r in rows {
        let old_id: i64 = r.get("id");
        let new_pid = match project_map.get(&r.get::<i64, _>("project_id")).copied() {
            Some(p) => p,
            None => {
                report.orphan_rows += 1;
                report.sifs.skipped += 1;
                continue;
            }
        };
        let code: String = r.get("code");
        if let Some(new_id) =
            sqlx::query_scalar("SELECT id FROM sif WHERE project_id = ?1 AND code = ?2")
                .bind(new_pid)
                .bind(&code)
                .fetch_optional(&mut *tx)
                .await?
        {
            sif_map.insert(old_id, new_id);
            report.sifs.skipped += 1;
            continue;
        }
        let new_id = sqlx::query(
            "INSERT INTO sif
                (project_id, code, name, description, sil_design, sil_verified, demand_mode,
                 pfdavg_target, proof_interval, created_at, updated_at, org_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        )
        .bind(new_pid)
        .bind(r.get::<String, _>("code"))
        .bind(r.get::<String, _>("name"))
        .bind(r.get::<String, _>("description"))
        .bind(r.get::<String, _>("sil_design"))
        .bind(r.get::<String, _>("sil_verified"))
        .bind(r.get::<String, _>("demand_mode"))
        .bind(r.get::<Option<f64>, _>("pfdavg_target"))
        .bind(r.get::<i64, _>("proof_interval"))
        .bind(r.get::<String, _>("created_at"))
        .bind(r.get::<String, _>("updated_at"))
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        sif_map.insert(old_id, new_id);
        report.sifs.inserted += 1;
    }

    // ── 4. 图纸：按 (新项目, code) 去重；sif_id 重映射；version 从 0 起 ───────
    let mut diagram_map: HashMap<i64, i64> = HashMap::new();
    let rows = sqlx::query(
        "SELECT id, project_id, code, name, sif_id, sheet_size, revision, data,
                updated_at, created_at
           FROM diagram ORDER BY id",
    )
    .fetch_all(&src)
    .await?;
    for r in rows {
        let old_id: i64 = r.get("id");
        let new_pid = match project_map.get(&r.get::<i64, _>("project_id")).copied() {
            Some(p) => p,
            None => {
                report.orphan_rows += 1;
                report.diagrams.skipped += 1;
                continue;
            }
        };
        let code: String = r.get("code");
        if let Some(new_id) =
            sqlx::query_scalar("SELECT id FROM diagram WHERE project_id = ?1 AND code = ?2")
                .bind(new_pid)
                .bind(&code)
                .fetch_optional(&mut *tx)
                .await?
        {
            diagram_map.insert(old_id, new_id);
            report.diagrams.skipped += 1;
            continue;
        }
        let old_sif: Option<i64> = r.get("sif_id");
        let new_sif = old_sif.and_then(|s| sif_map.get(&s).copied());
        let new_id = sqlx::query(
            "INSERT INTO diagram
                (project_id, code, name, sif_id, sheet_size, revision, data,
                 updated_at, created_at, org_id, version)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10, 0)",
        )
        .bind(new_pid)
        .bind(r.get::<String, _>("code"))
        .bind(r.get::<String, _>("name"))
        .bind(new_sif)
        .bind(r.get::<String, _>("sheet_size"))
        .bind(r.get::<String, _>("revision"))
        .bind(r.get::<String, _>("data"))
        .bind(r.get::<String, _>("updated_at"))
        .bind(r.get::<String, _>("created_at"))
        .bind(org_id)
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        diagram_map.insert(old_id, new_id);
        report.diagrams.inserted += 1;
    }

    // ── 5. SIF↔仪表 link：按 (sif, instrument, role, port_index) 幂等 ─────────
    if table_exists(&src, "sif_instrument").await? {
        let rows = sqlx::query(
            "SELECT id, sif_id, instrument_id, role, port_index, diagram_id, note, created_at
               FROM sif_instrument ORDER BY id",
        )
        .fetch_all(&src)
        .await?;
        for r in rows {
            let new_sif = match sif_map.get(&r.get::<i64, _>("sif_id")).copied() {
                Some(v) => v,
                None => {
                    report.orphan_rows += 1;
                    report.links.skipped += 1;
                    continue;
                }
            };
            let new_inst = match instrument_map
                .get(&r.get::<i64, _>("instrument_id"))
                .copied()
            {
                Some(v) => v,
                None => {
                    report.orphan_rows += 1;
                    report.links.skipped += 1;
                    continue;
                }
            };
            let role: String = r.get("role");
            let port: i64 = r.get("port_index");
            if sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM sif_instrument
                  WHERE org_id = ?1 AND sif_id = ?2 AND instrument_id = ?3 AND role = ?4
                    AND port_index = ?5",
            )
            .bind(org_id)
            .bind(new_sif)
            .bind(new_inst)
            .bind(&role)
            .bind(port)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
            {
                report.links.skipped += 1;
                continue;
            }
            let new_diagram = r
                .get::<Option<i64>, _>("diagram_id")
                .and_then(|d| diagram_map.get(&d).copied());
            sqlx::query(
                "INSERT INTO sif_instrument
                    (sif_id, instrument_id, role, port_index, diagram_id, note, created_at, org_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .bind(new_sif)
            .bind(new_inst)
            .bind(&role)
            .bind(port)
            .bind(new_diagram)
            .bind(r.get::<String, _>("note"))
            .bind(r.get::<String, _>("created_at"))
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
            report.links.inserted += 1;
        }
    }

    // ── 6. 旁路记录：自然键 (新项目, 新SIF, 操作人, 时间, 理由) 幂等 ──────────
    if table_exists(&src, "bypass_record").await? {
        let rows = sqlx::query(
            "SELECT id, project_id, sif_id, bypassed_by, bypassed_at, reason,
                    planned_restore, restored_at, permit_no, approved_by
               FROM bypass_record ORDER BY id",
        )
        .fetch_all(&src)
        .await?;
        for r in rows {
            let (new_pid, new_sif) = match remap_refs(&r, &project_map, &sif_map) {
                Some(v) => v,
                None => {
                    report.orphan_rows += 1;
                    report.bypasses.skipped += 1;
                    continue;
                }
            };
            let by: String = r.get("bypassed_by");
            let at: String = r.get("bypassed_at");
            let reason: String = r.get("reason");
            if sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM bypass_record
                  WHERE org_id = ?1 AND project_id = ?2 AND sif_id = ?3
                    AND bypassed_by = ?4 AND bypassed_at = ?5 AND reason = ?6",
            )
            .bind(org_id)
            .bind(new_pid)
            .bind(new_sif)
            .bind(&by)
            .bind(&at)
            .bind(&reason)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
            {
                report.bypasses.skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO bypass_record
                    (project_id, sif_id, bypassed_by, bypassed_at, reason, planned_restore,
                     restored_at, permit_no, approved_by, org_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            )
            .bind(new_pid)
            .bind(new_sif)
            .bind(&by)
            .bind(&at)
            .bind(&reason)
            .bind(r.get::<String, _>("planned_restore"))
            .bind(r.get::<String, _>("restored_at"))
            .bind(r.get::<String, _>("permit_no"))
            .bind(r.get::<String, _>("approved_by"))
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
            report.bypasses.inserted += 1;
        }
    }

    // ── 7. 服务工单 ───────────────────────────────────────────────────────────
    if table_exists(&src, "service_ticket").await? {
        let rows = sqlx::query(
            "SELECT id, project_id, sif_id, instrument_id, kind, title, detail, opened_by,
                    opened_at, closed_at, severity
               FROM service_ticket ORDER BY id",
        )
        .fetch_all(&src)
        .await?;
        for r in rows {
            let new_pid = match project_map.get(&r.get::<i64, _>("project_id")).copied() {
                Some(p) => p,
                None => {
                    report.orphan_rows += 1;
                    report.tickets.skipped += 1;
                    continue;
                }
            };
            let opened_by: String = r.get("opened_by");
            let opened_at: String = r.get("opened_at");
            let title: String = r.get("title");
            if sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM service_ticket
                  WHERE org_id = ?1 AND project_id = ?2 AND opened_by = ?3
                    AND opened_at = ?4 AND title = ?5",
            )
            .bind(org_id)
            .bind(new_pid)
            .bind(&opened_by)
            .bind(&opened_at)
            .bind(&title)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
            {
                report.tickets.skipped += 1;
                continue;
            }
            let new_sif = r
                .get::<Option<i64>, _>("sif_id")
                .and_then(|s| sif_map.get(&s).copied());
            let new_inst = r
                .get::<Option<i64>, _>("instrument_id")
                .and_then(|i| instrument_map.get(&i).copied());
            sqlx::query(
                "INSERT INTO service_ticket
                    (project_id, sif_id, instrument_id, kind, title, detail, opened_by,
                     opened_at, closed_at, severity, org_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            )
            .bind(new_pid)
            .bind(new_sif)
            .bind(new_inst)
            .bind(r.get::<String, _>("kind"))
            .bind(&title)
            .bind(r.get::<String, _>("detail"))
            .bind(&opened_by)
            .bind(&opened_at)
            .bind(r.get::<String, _>("closed_at"))
            .bind(r.get::<String, _>("severity"))
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
            report.tickets.inserted += 1;
        }
    }

    // ── 8. 审计日志（有则导；按 5 元组幂等）──────────────────────────────────
    if table_exists(&src, "audit_log").await? {
        let audit_cols = columns_of(&src, "audit_log").await?;
        // 极早期库 payload/note 列名固定（002 即如此），直接查
        let rows = sqlx::query(
            "SELECT id, ts, actor, action, target_table, target_id, payload_json, note
               FROM audit_log ORDER BY id",
        )
        .fetch_all(&src)
        .await?;
        let _ = audit_cols;
        for r in rows {
            let target_table: String = r.get("target_table");
            let target_id: Option<i64> = r.get("target_id");
            let action: String = r.get("action");
            let actor: String = r.get("actor");
            let ts: String = r.get("ts");
            if sqlx::query_scalar::<_, i64>(
                "SELECT 1 FROM audit_log
                  WHERE org_id = ?1 AND target_table = ?2
                    AND IFNULL(target_id, -1) = IFNULL(?3, -1)
                    AND action = ?4 AND actor = ?5 AND ts = ?6",
            )
            .bind(org_id)
            .bind(&target_table)
            .bind(target_id)
            .bind(&action)
            .bind(&actor)
            .bind(&ts)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
            {
                report.audit_logs.skipped += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO audit_log
                    (ts, actor, action, target_table, target_id, payload_json, note, org_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            )
            .bind(&ts)
            .bind(&actor)
            .bind(&action)
            .bind(&target_table)
            .bind(target_id)
            .bind(r.get::<String, _>("payload_json"))
            .bind(r.get::<String, _>("note"))
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
            report.audit_logs.inserted += 1;
        }
    }

    tx.commit().await?;
    Ok(report)
}

/// 旁路行的 project/sif 双外键重映射。
fn remap_refs(
    r: &SqliteRow,
    project_map: &HashMap<i64, i64>,
    sif_map: &HashMap<i64, i64>,
) -> Option<(i64, i64)> {
    let pid = project_map.get(&r.get::<i64, _>("project_id")).copied()?;
    let sid = sif_map.get(&r.get::<i64, _>("sif_id")).copied()?;
    Some((pid, sid))
}
