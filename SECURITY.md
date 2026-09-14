# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | ✅                 |
| < 0.1   | ❌                 |

## Reporting a Vulnerability

**请不要**在公开 Issue 里披露安全漏洞。

发邮件到 `security@sif-studio.example`（占位），内容建议包括：
1. 漏洞描述（影响哪些组件 / 数据）
2. 复现步骤（PoC 截图 / 代码片段）
3. 影响评估（信息泄露 / 拒绝服务 / 提权 / ……）
4. 是否已知被利用
5. 你的联系方式

我们会在 **48 小时内**确认收悉 + **7 天内**给出修复计划。

## Scope（我们关心的）

- 本地 SQLite DB 文件被未授权读取（理论上仅 owner user 可读）
- Tauri IPC 命令的越权调用（capability 缺失导致前端 XSS 触发 `invoke`）
- 旁路 SIF 后未报警的合规漏洞
- 用户自定义 SVG / HTML 注入（XSS in Editor）

## Out of Scope

- 操作系统自身漏洞
- 第三方依赖（sqlx / Tauri / Vue）已知 CVE（请直接 upstream 上报）
- 用户自己 fork 后改坏的责任

## 已知安全设计

- `studio.db` 默认存 `%APPDATA%/sif-studio/`（Windows ACL 保护）
- SQLite 启用 WAL + foreign_keys（`PRAGMA foreign_keys = ON`）
- Tauri 2 capabilities 明示最小权限（见 `src-tauri/capabilities/default.json`）
- M0 编辑器嵌入到 iframe：用 `srcdoc` 或 `src` 走本地资源，**不引入第三方 CDN**
- 不收集任何遥测 / 不联网（Tauri 2 默认 `core:webview` 拒绝所有外发请求）

## 致谢

公开感谢安全研究者（如果愿意署名）—— 见 README 致谢节。
