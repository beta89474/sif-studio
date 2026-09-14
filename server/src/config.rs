//! 进程级配置（阶段 C / E）
//!
//! 全部从环境变量读取，启动时一次性固化进 AppState：
//! - `SIF_ADMIN_TOKEN`：服务器操作员令牌。设置后才开放
//!   `/api/admin/backup`、`/api/admin/restore`（这两个端点操作**全组织**数据，
//!   绝不能让普通 org owner 触碰）；不设置则端点返回 404。
//!   请求需带 `X-Admin-Token: <token>` 头。
//! - `SIF_COOKIE_SECURE`：见 http/auth.rs（HTTPS 部署时置 1）。
//! - `SIF_TRUST_PROXY`（E2）：置 1 表示服务位于受控反向代理之后，客户端 IP
//!   从 `X-Real-IP`（优先）或 `X-Forwarded-For` 末跳读取；不置时忽略这些头，
//!   以防直连客户端伪造头绕过限流。
//! - `SIF_INVITE_ONLY`（E4）：置 1 后关闭公开注册——注册请求必须携带有效
//!   邀请 token，防止私有部署被陌生人自助开组织。
//! - `SIF_SESSION_TTL_DAYS`（G1）：会话有效期（天），默认 30，允许 1~365。
//!   同时控制 session 表过期时间与登录 cookie 的 Max-Age。
//! - `SIF_RATE_LIMIT_LOGIN` / `SIF_RATE_LIMIT_REGISTER`（G2）：登录/注册
//!   限流阈值（窗口内最大尝试次数），默认各 10；窗口长度不变（登录 15 分钟、
//!   注册 1 小时）。
//! - `SIF_INVITE_TTL_DEFAULT` / `SIF_INVITE_TTL_MAX`（G3）：邀请链接默认
//!   有效期与上限（天），默认 7 / 30，上限允许 1~365。

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AppConfig {
    /// None = 管理端点关闭；Some(token) = 要求 X-Admin-Token 常量时间匹配。
    pub admin_token: Option<String>,
    /// 反向代理部署模式（信任代理注入的客户端 IP 头）。
    pub trust_proxy: bool,
    /// 仅接受邀请注册。
    pub invite_only: bool,
    /// 会话有效期（天）。
    pub session_ttl_days: i64,
    /// 登录限流阈值（窗口内尝试次数）。
    pub rate_limit_login: usize,
    /// 注册限流阈值（窗口内尝试次数）。
    pub rate_limit_register: usize,
    /// 邀请链接默认有效期（天）。
    pub invite_ttl_default: i64,
    /// 邀请链接最长有效期（天）。
    pub invite_ttl_max: i64,
}

impl AppConfig {
    /// 从环境变量构建；空串视为未设置。
    pub fn from_env() -> Self {
        let invite_ttl_max = env_num("SIF_INVITE_TTL_MAX", 30, 1, 365);
        Self {
            admin_token: std::env::var("SIF_ADMIN_TOKEN")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            trust_proxy: env_flag("SIF_TRUST_PROXY"),
            invite_only: env_flag("SIF_INVITE_ONLY"),
            session_ttl_days: env_num("SIF_SESSION_TTL_DAYS", 30, 1, 365),
            rate_limit_login: env_num("SIF_RATE_LIMIT_LOGIN", 10, 1, 100_000) as usize,
            rate_limit_register: env_num("SIF_RATE_LIMIT_REGISTER", 10, 1, 100_000) as usize,
            invite_ttl_default: env_num("SIF_INVITE_TTL_DEFAULT", 7, 1, invite_ttl_max),
            invite_ttl_max,
        }
    }

    /// 会话有效期（秒）：session 表过期与 cookie Max-Age 共用。
    pub fn session_ttl_seconds(&self) -> i64 {
        self.session_ttl_days * 24 * 60 * 60
    }

    /// 测试用：显式给/不给令牌（其余开关默认关闭）。
    pub fn with_admin_token(token: Option<&str>) -> Self {
        Self {
            admin_token: token.map(str::to_string),
            trust_proxy: false,
            invite_only: false,
            session_ttl_days: 30,
            rate_limit_login: 10,
            rate_limit_register: 10,
            invite_ttl_default: 7,
            invite_ttl_max: 30,
        }
    }

    /// 测试用：完整控制三个开关。
    pub fn for_test(token: Option<&str>, trust_proxy: bool, invite_only: bool) -> Self {
        Self {
            admin_token: token.map(str::to_string),
            trust_proxy,
            invite_only,
            session_ttl_days: 30,
            rate_limit_login: 10,
            rate_limit_register: 10,
            invite_ttl_default: 7,
            invite_ttl_max: 30,
        }
    }

    /// 常量时间比较，避免令牌校验成为侧信道。
    pub fn check_admin_token(&self, provided: &str) -> bool {
        match &self.admin_token {
            None => false,
            Some(expected) => constant_time_eq(expected.as_bytes(), provided.as_bytes()),
        }
    }
}

/// 解析 1/true/yes/on（大小写不敏感）为真，其余（含未设置）为假。
fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref().map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("on")
    )
}

/// 读取数值型环境变量：未设置/空 → default；非法 → 警告 + default；
/// 越界 → 警告 + 截断到 [min, max]（安全配置宁可保守，不能因 typo 静默失效）。
fn env_num(name: &str, default: i64, min: i64, max: i64) -> i64 {
    parse_num(std::env::var(name).ok().as_deref(), default, min, max)
}

/// 纯逻辑（便于单测，不碰进程环境）。
fn parse_num(raw: Option<&str>, default: i64, min: i64, max: i64) -> i64 {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => default,
        Some(s) => match s.parse::<i64>() {
            Ok(v) if (min..=max).contains(&v) => v,
            Ok(v) => {
                eprintln!(
                    "[sif-studio-server] 警告: 数值 {v} 超出允许范围 {min}~{max}，已截断到边界"
                );
                v.clamp(min, max)
            }
            Err(_) => {
                eprintln!("[sif-studio-server] 警告: 值 {s:?} 不是有效整数，使用默认值 {default}");
                default
            }
        },
    }
}

/// 等长常量时间比较（长度差异不掩盖：令牌长度本就不是秘密）。
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

pub type SharedConfig = Arc<AppConfig>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_num_defaults_on_missing_or_invalid() {
        assert_eq!(parse_num(None, 30, 1, 365), 30);
        assert_eq!(parse_num(Some(""), 30, 1, 365), 30);
        assert_eq!(parse_num(Some("   "), 30, 1, 365), 30);
        assert_eq!(parse_num(Some("abc"), 30, 1, 365), 30);
        assert_eq!(parse_num(Some("7.5"), 30, 1, 365), 30);
    }

    #[test]
    fn parse_num_passes_through_in_range() {
        assert_eq!(parse_num(Some("90"), 30, 1, 365), 90);
        assert_eq!(parse_num(Some("  7  "), 30, 1, 365), 7);
    }

    #[test]
    fn parse_num_clamps_out_of_range() {
        assert_eq!(parse_num(Some("9999"), 30, 1, 365), 365);
        assert_eq!(parse_num(Some("0"), 30, 1, 365), 1);
        assert_eq!(parse_num(Some("-5"), 30, 1, 365), 1);
    }

    #[test]
    fn session_ttl_seconds_matches_days() {
        let mut cfg = AppConfig::for_test(None, false, false);
        assert_eq!(cfg.session_ttl_seconds(), 30 * 24 * 60 * 60);
        cfg.session_ttl_days = 2;
        assert_eq!(cfg.session_ttl_seconds(), 2 * 24 * 60 * 60);
    }
}
