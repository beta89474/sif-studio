//! DB 连接池 + 迁移 runner（在线版）
//!
//! 数据库路径策略：
//!   - 环境变量 `DATABASE_URL`，默认 `sqlite://data/studio.db?mode=rwc`
//!     （容器部署时把 /data 挂为持久卷）
//!
//! 迁移：把 `migrations/` 整个目录嵌入二进制（部署时无需再带外部 SQL 文件）。

use crate::AppResult;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub type Pool = SqlitePool;

/// 嵌入式迁移账本（编译期打进二进制）。open() 与跨版本备份恢复共用同一套，
/// 保证"恢复时把旧备份升级到当前版本"用的 SQL 与正常启动完全一致。
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// 数据库连接串：DATABASE_URL 或默认文件库。
pub fn database_url() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://data/studio.db?mode=rwc".to_string())
}

fn is_memory_url(url: &str) -> bool {
    url.contains(":memory:")
}

/// 主入口：开池 + 跑迁移。
pub async fn open() -> AppResult<SqlitePool> {
    let url = database_url();

    // 文件库：确保父目录存在（内存库跳过）。
    if !is_memory_url(&url) {
        if let Some(path_part) = url
            .strip_prefix("sqlite://")
            .and_then(|s| s.split('?').next())
        {
            if !path_part.is_empty() {
                let p = std::path::Path::new(path_part);
                if let Some(parent) = p.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)?;
                    }
                }
            }
        }
    }

    let opts = SqliteConnectOptions::from_str(&url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true) // 必须开 —— schema 全靠外键级联
        .busy_timeout(std::time::Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;

    MIGRATOR.run(&pool).await?;

    Ok(pool)
}

/// 测试 helper：内存 SQLite（不开文件）。集成测试用。
pub async fn open_in_memory() -> AppResult<SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// 测试 helper：文件库（WAL + FK + 迁移已跑）。备份/恢复测试专用——
/// `VACUUM INTO` 对纯 `:memory:` 源库不会产出文件，这是 SQLite 的行为。
pub async fn open_file(path: &std::path::Path) -> AppResult<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}
