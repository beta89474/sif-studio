//! 启动元信息（DB 版本 / 部署形态 / 启动时间），给前端做版本显示与诊断。

use crate::AppResult;
use chrono::Utc;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppMeta {
    pub db_version: i64,
    /// 在线版：返回数据库连接串（隐去敏感参数的空间有限，SQLite 本地文件无密钥）
    pub db_path: String,
    pub app_version: String,
    pub started_at: String,
}

pub async fn db_version_inner(pool: &SqlitePool) -> AppResult<AppMeta> {
    // 读 migration 表的最大版本号——直接展示给用户
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(MAX(version), 0) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await?;

    Ok(AppMeta {
        db_version: row.0,
        db_path: crate::db::database_url(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        started_at: Utc::now().to_rfc3339(),
    })
}
