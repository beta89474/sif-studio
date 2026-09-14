//! 库入口（在线版 HTTP 服务）
//!
//! 设计要点：
//! - AppState 仅持有 Arc<SqlitePool>，sqlx 池内部已做线程安全
//! - command 模块按业务域分组（meta / instruments / projects / diagrams / sifs / tags 等），
//!   全部保留为纯业务函数 `*_inner(pool, ...)`，HTTP 层（http/rpc.rs）负责分发
//! - snake_case 业务命名不变；前端走 camelCase（serde 输入结构 + RPC 适配层）

pub mod backup;
pub mod commands;
pub mod config;
pub mod db;
mod error;
pub mod http;
pub mod import;

use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::SqlitePool;

pub use config::AppConfig;
pub use error::{AppError, AppResult};
use http::ratelimit::RateLimiter;

/// 进程级状态：注入到每个 axum handler
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<SqlitePool>,
    pub config: Arc<AppConfig>,
    pub rate: Arc<RateLimiter>,
}

/// 启动 HTTP 服务：开池 + 跑迁移 + 挂载 /api 路由 + 静态资源托管。
pub async fn serve() -> AppResult<()> {
    let pool = db::open().await?;
    let config = Arc::new(AppConfig::from_env());
    let state = AppState {
        db: Arc::new(pool),
        rate: Arc::new(RateLimiter::with_limits(
            config.rate_limit_login,
            config.rate_limit_register,
        )),
        config,
    };

    let app = http::router(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(AppError::Io)?;
    eprintln!("[sif-studio-server] listening on http://{addr}");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
