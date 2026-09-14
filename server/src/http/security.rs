//! 安全响应头中间件（阶段 C1）
//!
//! 对所有响应（含 /api 与静态资源）统一注入：
//! - `X-Content-Type-Options: nosniff`：禁止浏览器 MIME 嗅探
//! - `X-Frame-Options: SAMEORIGIN`：只允许同源框架（编辑器以同源 iframe 嵌入，
//!   不能用 DENY）；与 CSP `frame-ancestors 'self'` 双保险
//! - `Referrer-Policy: same-origin`：外跳不带完整路径
//! - `Permissions-Policy`：关闭摄像头/麦克风/地理位置/支付等用不到的能力
//! - `Content-Security-Policy`：
//!     · 主应用走外部 chunk，可用严格策略（无 unsafe-eval/inline-script）；
//!     · public/editor.html 是单文件工具页（内联 <script>），按路径单独放行，
//!       但仍限定 frame-ancestors 'self'（只能被本应用 iframe）。
//!
//! 注意：Vite dev server(:5173) 不经本中间件；这里只服务生产构建产物。

use axum::http::{HeaderName, HeaderValue};
use axum::{
    body::Body,
    http::{Request, Response},
    middleware::Next,
};

/// 主 SPA（index.html + /assets/* + /api/*）：严格策略。
///
/// - script-src 'self'：Vite 构建产物全部外置，无内联脚本（Vue 运行时无需 eval）
/// - style-src 放行 'unsafe-inline'：Vue 运行时写 style 属性/动态样式
/// - worker-src/blob: 与 frame-src blob:：pdfmake 用 blob Worker/新窗口预览 PDF
/// - img/font 放行 data:/blob:：图纸截图与字体子集
const CSP_APP: &str = concat!(
    "default-src 'self'; ",
    "base-uri 'self'; ",
    "object-src 'none'; ",
    "frame-ancestors 'self'; ",
    "form-action 'self'; ",
    "script-src 'self'; ",
    "style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data: blob:; ",
    "font-src 'self' data:; ",
    "connect-src 'self'; ",
    "frame-src 'self' blob:; ",
    "worker-src 'self' blob:;",
);

/// 单文件编辑器页（内联脚本 + 动态样式），脚本放行 unsafe-inline，
/// 但不允许被外站框架（frame-ancestors 保持 'self'）。
const CSP_EDITOR: &str = concat!(
    "default-src 'self' blob: data:; ",
    "base-uri 'self'; ",
    "object-src 'none'; ",
    "frame-ancestors 'self'; ",
    "form-action 'self'; ",
    "script-src 'self' 'unsafe-inline' blob:; ",
    "style-src 'self' 'unsafe-inline'; ",
    "img-src 'self' data: blob:; ",
    "font-src 'self' data:; ",
    "connect-src 'self'; ",
    "frame-src 'self' blob:; ",
    "worker-src 'self' blob:;",
);

const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=(), interest-cohort=()";

/// 判断路径是否为单文件编辑器页（含带查询串的 /editor.html）。
fn is_editor_page(path: &str) -> bool {
    path.split('?').next().is_some_and(|p| p == "/editor.html")
}

pub(crate) async fn security_headers(req: Request<Body>, next: Next) -> Response<Body> {
    let csp = if is_editor_page(req.uri().path()) {
        CSP_EDITOR
    } else {
        CSP_APP
    };

    let mut resp = next.run(req).await;
    let h = resp.headers_mut();

    fn insert(h: &mut axum::http::HeaderMap, name: HeaderName, value: &'static str) {
        h.insert(name, HeaderValue::from_static(value));
    }

    insert(h, axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    insert(h, axum::http::header::X_FRAME_OPTIONS, "SAMEORIGIN");
    insert(h, axum::http::header::REFERRER_POLICY, "same-origin");
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
    h.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_str(csp).expect("CSP 头值为合法 ASCII"),
    );

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_path_detection() {
        assert!(is_editor_page("/editor.html"));
        assert!(is_editor_page("/editor.html?diagram=1&v=123"));
        assert!(!is_editor_page("/"));
        assert!(!is_editor_page("/assets/index-x.js"));
    }
}
