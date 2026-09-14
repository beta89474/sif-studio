//! 进程内滑动窗口限流（阶段 D2）
//!
//! 只守护公开的认证端点（登录/注册），防止在线爆破与批量开号：
//! - 登录：同一 IP 对同一邮箱 15 分钟内最多 10 次尝试；
//!   成功登录后该桶清零（正常用户输错几次不会被锁）。
//! - 注册：同一 IP 每小时最多 10 次（防批量注册）。
//!
//! 设计取舍：
//! - 内存态（`Mutex<HashMap>`），不引 redis 等外部依赖；单实例部署够用，
//!   重启即解禁——认证爆破防护本就不需要持久化惩罚。
//! - 多实例水平扩展时各实例独立计数，阈值乘以实例数仍远高于人工速度；
//!   真需要全局一致时再把实现换成共享存储（接口不变）。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 登录尝试桶：IP + 邮箱（默认 15 分钟 10 次）。
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
const LOGIN_MAX_DEFAULT: usize = 10;

/// 注册桶：仅按 IP（默认 1 小时 10 次）。
const REGISTER_WINDOW: Duration = Duration::from_secs(60 * 60);
const REGISTER_MAX_DEFAULT: usize = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    Login,
    Register,
}

pub struct RateLimiter {
    login_max: usize,
    register_max: usize,
    buckets: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::with_limits(LOGIN_MAX_DEFAULT, REGISTER_MAX_DEFAULT)
    }

    /// G2 —— 阈值可由 AppConfig 按 SIF_RATE_LIMIT_LOGIN / _REGISTER 配置；
    /// 窗口长度保持常量（时长极少需要按部署调整）。
    pub fn with_limits(login_max: usize, register_max: usize) -> Self {
        Self {
            login_max,
            register_max,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// 记录一次尝试；超过窗口阈值返回 false（调用方应答 429）。
    pub fn check(&self, kind: LimitKind, ip: &str, email: &str) -> bool {
        let (window, max, key) = match kind {
            LimitKind::Login => (
                LOGIN_WINDOW,
                self.login_max,
                format!("login:{}:{}", ip, email.trim().to_lowercase()),
            ),
            LimitKind::Register => (REGISTER_WINDOW, self.register_max, format!("register:{ip}")),
        };

        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        let hits = buckets.entry(key).or_default();
        hits.retain(|t| now.duration_since(*t) < window);
        if hits.len() >= max {
            return false;
        }
        hits.push(now);
        // 顺手清理空桶，避免无界增长
        buckets.retain(|_, v| !v.is_empty());
        true
    }

    /// 登录成功后清掉该 IP+邮箱的失败/尝试记录（不误伤正常用户）。
    pub fn reset_login(&self, ip: &str, email: &str) {
        let key = format!("login:{}:{}", ip, email.trim().to_lowercase());
        let mut buckets = self.buckets.lock().expect("rate limiter mutex poisoned");
        buckets.remove(&key);
    }
}
