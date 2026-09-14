# 贡献指南

欢迎！本项目秉承开放、可读、低门槛的原则。请先读 [ROADMAP.md](./ROADMAP.md)，看你的方向是不是已经在做，避免重复劳动。

---

## 行为准则

请阅读 [CODE_OF_CONDUCT.md](./CODE_OF_CONDUCT.md)。简单说：建设性讨论、对事不对人、对新手耐心、对边缘案例包容。

---

## 怎么提 Issue

- 🐛 **Bug**：用 Bug 模板 —— 提供最小复现、操作步骤、截图（UI 问题）、期望/实际、运行环境
- 💡 **功能建议**：用 Feature 模板 —— 痛点场景、你期望的行为、参考标准（IEC 61511 / ISA-5.x / GB/T 21109 / GB/T 50770）
- ❓ **使用问题**：先扫 [README](./README.md) + [docs/](./docs)，没答案再开 discussion
- 🔒 **安全问题**：直接看 [SECURITY.md](./SECURITY.md)，**不发公开 Issue**

---

## 怎么提 PR

1. **Fork + 新建分支**：`git checkout -b feat/<short-name>` 或 `fix/<short-name>`
2. **先确认能跑通**：
   ```bash
   npm install
   cd src-tauri && cargo test && cd ..
   npm run build
   ```
3. **小步提交**：一个 commit 一个语义；commit message 用 `feat: …` / `fix: …` / `docs: …` / `style: …` / `refactor: …` / `test: …` / `chore: …` 之类前缀
4. **追加测试**：业务逻辑改动**必须**有测试 / 静态自查说明
5. **文档同步**：改了用户面 = 同步 README / CHANGELOG 的 Unreleased 节
6. **填 PR 模板**：标题简洁、背景 1 段、改了什么 1 张图 / 1 张表、怎么验 3~5 步

---

## 代码风格

### 前端（Vue 3 + TS）

- Composition API + `<script setup lang="ts">`，避免 Options API
- 组件命名 PascalCase；stores camelCase；CSS BEM 或 scoped
- `<template>` 缩进 2 格；`<script>` 双引号
- 别用全局样式变量覆盖 5 色词汇，新增语义色前开 Issue 讨论

### 后端（Rust）

- 命名：snake_case 函数 / 字段；PascalCase 类型；SCREAMING_SNAKE_CASE 常量
- 错误用 `AppError` 包装，**不要**直接 `unwrap()` 在会触达命令边界的地方
- 异步：所有 DB 操作走 `sqlx` 异步；不要 `tokio::task::spawn_blocking` 直接进 pool
- 测试：`#[tokio::test]`；helper 函数用 `setup_scenario()` 返回 ID 元组，**解构而非重复插入**

### 数据库

- 新增表必须：建索引（覆盖 `id` / 高频查询列） + CHECK 约束（枚举 / 范围）
- 关联表：必须 `ON DELETE CASCADE` 或 `SET NULL`，**避免悬空指针**
- 迁移文件：`migrations/v002_xxx.sql`，只能追加，**禁止修改历史迁移**

---

## 提交信息

```
feat(sif-dashboard): 显示跨图仪表数量 + 详情展开

- list_sifs 已用 GROUP_CONCAT 返回 detectors_csv / finals_csv
- 详情展开改为 4 块结构（元数据 / 检测 / 最终 / 出现位置）
- 新增关联按钮 + 弹窗，可选 role / port_index

Refs: #123
```

---

## 提 PR 之后

CI 会跑 `cargo test` + `npm run build`。失败的话：
- 看日志贴评论
- 修好后 `git push` 会重跑，**不要 close 再开**

---

## 进阶

- **修改几何 / 走线**：先读 [docs/design/M1-architecture.md](./docs/design/M1-architecture.md)，改完跑集成测试 + 在 jsdom 里跑 M0 渲染验证
- **新增 IPC command**：先在 `commands/mod.rs` 注册 + `lib.rs` `generate_handler!` 加名；前端 `studio.ts` 加方法；本仓库无 Tauri 测试工具，只能编译验证
- **修改 schema**：先在 `migrations/v0XX_xxx.sql` 写新版；写示例数据迁移脚本（IF NOT EXISTS）

---

## 联机协作

- Discussion：长期问题、设计讨论
- Issue：bug + feature + 短期任务
- Project：里程碑看板（在 `Projects` Tab 看）

---

感谢你的贡献 🙏
