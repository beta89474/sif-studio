# SIF Studio / 联锁工坊

> 面向中小石化自动化工程师的桌面文档工具：联锁逻辑图绘制、真实仪表台账、SIF 跨图汇总、服务记录与审计资料导出。

---

## 📌 这是什么？

| 维度 | 内容 |
|---|---|
| **目标用户** | 中小型石化 / 化工 / 公用工程的自动化、仪表、运维工程师；中型设计院的电仪设计人员 |
| **典型场景** | 日常 P&ID 阅读、联锁逻辑图绘制与维护、改造项目仪表台账、跨多项目的 SIF 汇总、SIL 验算落地、年度审计资料整理 |
| **形态** | Tauri 桌面应用（Windows / macOS / Linux），本地 SQLite 存储，无需联网 |
| **标准依据** | IEC 61511-1:2016 / GB/T 21109.1-2007 / ISA-5.1 & 5.2 / GB/T 50770-2013 |
| **许可** | [MIT](./LICENSE) + 捐赠支持（[怎么赞助？](#-支持项目)） |

> 💡 **为什么叫「联锁工坊」** —— Studio 的中文翻译取「工作室 / 工坊」，把工程师"坐下来做仪表 + 画图 + 出资料"的工位感表达出来。

---

## 🎯 项目目标（Roadmap）

- **M0 ✅ 联锁逻辑图编辑器（单文件 HTML）**：完整工业级 6 列 SIF、30 种元件、走线优化、因果表、SIL 验算、附表、A 系列幅面；`联锁逻辑图编辑器.html`（独立维护）
- **M1 ✅ 数据库 + 多图 + 跨图 SIF 汇总**：本仓库当前主线
- **M2 ✅ 服务报告 / 审计包闭环**：旁路授权（M2.2）+ 仪表/SIF/项目修改历史字段级 diff（M2.3 + M2.4）+ 审计包 CSV 多维导出（M2.5）+ PDF 报告（M2.6：中文子集字体 + 五章 A4）+ 增补图表 + 公司抬头 + 签章位（M2.7）
- **M3 商业化**：自动升级、付费插件、品牌定制（仍保 MIT 自由发行版）
- **M4 OPC UA / 状态看板 / 移动端现场改图**

详见 [ROADMAP.md](./ROADMAP.md)。

---

## 🚀 快速上手（开发）

### 前置

- Node 18+
- Rust 1.77+（含 `cargo`，Tauri 2 工具链）— Windows 上详细步骤见 [docs/INSTALL-Rust-on-Windows.md](./docs/INSTALL-Rust-on-Windows.md)
- 在 Windows 上需要 WebView2 Runtime（Windows 10/11 已自带）

### 起步

```bash
git clone https://github.com/<your-org>/sif-studio.git
cd sif-studio
npm install
npm run tauri:dev        # 启动开发模式（Vite + Rust 一起跑）
```

第一次启动会自动建本地数据库：

```
%APPDATA%\sif-studio\studio.db     （Windows）
~/Library/Application Support/sif-studio/studio.db  （macOS）
~/.local/share/sif-studio/studio.db  （Linux）
```

### 单元 / 集成测试

```bash
cd src-tauri
cargo test                # 跑 sqlx 集成测试（不依赖 Tauri runtime）
npm run build             # 类型检查 + 产出 dist/
```

### 打包发布

```bash
npm run tauri:build       # 产出系统原生安装包（msi / dmg / AppImage）
```

---

## 🏗️ 架构（三层）

```
┌────────────────────────────────────────────────────────┐
│ Vue 3 + Pinia  UI 层                                    │
│ ┌── views ───────────────────────────────────────┐    │
│ │ Home.vue · Instruments.vue · SifDashboard.vue  │    │
│ │ ProjectsView.vue · DiagramEditor.vue (iframe)  │    │
│ └──────────────────────┬─────────────────────────┘    │
│                        │ invoke(...)                    │
│ ┌── stores ────────────┼─────────────────────────┐    │
│ │ studio.ts    IP 总线，Pinia 单一状态源         │    │
│ └──────────────────────┼─────────────────────────┘    │
└────────────────────────┼─────────────────────────────────┘
                         │ Tauri 2 IPC
┌────────────────────────┼─────────────────────────────────┐
│ Rust commands 层        │                                 │
│ ┌── src/commands ──────┼──────────────────────────┐    │
│ │ meta / instruments / projects / diagrams /      │    │
│ │ sifs / tags                                ·    │    │
│ └──────────────────────┬─────────────────────────┘    │
│ ┌── src/db.rs ──────────┼──────────────────────────┐    │
│ │ sqlx SqlitePool  +  嵌入式 migrate!             │    │
│ └──────────────────────┬─────────────────────────┘    │
└────────────────────────┼─────────────────────────────────┘
                         │
                  ┌──────▼────────┐
                  │ SQLite 单文件 │   ./migrations/v001_*.sql
                  └───────────────┘
```

**关键决策**：
- **数据本地优先**：每个用户一台机器一个 `studio.db`，跨设备迁移靠 `.db` 文件直接拷贝
- **M0 编辑器以 iframe + postMessage 嵌入**：保留 205KB 单文件 HTML 的所有能力，复用 5 色语义、走线、因果表
- **IPC 全走 camelCase**：`#[tauri::command]` 自动把 `list_sifs` 暴露成 `listSifs`
- **错误统一结构化**：`AppError` 序列化为 `{kind, message}`，前端表格/Toast 直接吃
- **级联靠外键**：schema 全靠 `ON DELETE CASCADE` 维护，删除项目自动清图、SIF、link

---

## 📊 数据模型（v001 schema，7 表）

```
┌──────────────┐         ┌──────────────┐
│   project    │1───────*│   diagram    │
└──────────────┘         └──────┬───────┘
       │1                  * 1  │
       │       ┌──────────────┐ │
       ├──────*│     sif      │*│   (sif_id 软链图)
       │       └──────┬───────┘ │
       │              *│        │
       │       ┌──────▼───────┐ │
       │       │sif_instrument│ │
       │       └──────┬───────┘ │
       │             *│─────────┘
       │       ┌──────▼───────┐
       │       │  instrument  │
       │       └──────────────┘
       │
       │1──────* bypass_record
       │1──────* service_ticket   (M2 起激活)
```

- `instrument` —— 真实仪表位号主数据（detector / final / logic / aux）
- `sif_instrument` —— 跨图关联表（同一仪表在多图/多角色）
- `bypass_record` —— 旁路登记（IEC 61511 §11.5.2 限时 + 报警要求）
- `service_ticket` —— 服务工单（M2 才激活）

详见 `src-tauri/migrations/v001_initial_schema.sql`。

---

## 📂 仓库结构

```
sif-studio/
├── package.json / vite.config.ts / tsconfig*.json
├── src/                              # Vue 前端
│   ├── App.vue · main.ts · router.ts
│   ├── stores/studio.ts             # Pinia 单一状态源
│   ├── views/                       # 5 个路由
│   ├── styles/global.css
│   └── assets/
├── src-tauri/                       # Rust 后端 + Tauri
│   ├── Cargo.toml · tauri.conf.json
│   ├── capabilities/default.json
│   ├── src/
│   │   ├── lib.rs · main.rs
│   │   ├── db.rs                   # sqlx Pool + 嵌入式迁移
│   │   ├── error.rs                # AppError + serde
│   │   └── commands/               # 11 模块 30+ commands（M2.7 加审计图表）
│   ├── migrations/             # v001 初始 + v002/v003 增量
│   └── tests/                  # integration_test + m22/m23/m24/m25/m27 分册
├── public/
│   └── editor.html                 # M0 单文件 HTML 编辑器（iframe 嵌入）
├── docs/
│   ├── INSTALL-Rust-on-Windows.md
│   ├── INSTALL-VERIFICATION.md     # 端用户实装自检清单
│   └── STYLE-GUIDE.md
├── scripts/
│   ├── subset-font.cjs            # 中文字体子集化（GB2312 → SimHei-subset.ttf）
│   ├── verify-font-coverage.cjs   # 子集覆盖率静态检查
│   ├── ensure-font.cjs            # prebuild 守卫（缺则重生）
│   └── verify-install.ps1         # Windows 安装前环境体检
├── README.md / ROADMAP.md / CHANGELOG.md / CONTRIBUTING.md
├── LICENSE (MIT) / SECURITY.md / CODE_OF_CONDUCT.md
└── .github/
    ├── ISSUE_TEMPLATE/
    └── PULL_REQUEST_TEMPLATE.md
```

---

## 🎨 设计语言

- **5 色词汇**（与 M0 编辑器共用）：检测 / 逻辑 / 最终(红) / 旁路(琥珀) / 复位(紫) / 选中(品红)
- **白图为主、蓝图可切换**：编辑器主题延续 M0
- **ISA-101 色彩语义唯一**：每色一义、克制使用（详见 `src/styles/global.css` 注释）

---

## 🤝 贡献

看 [CONTRIBUTING.md](./CONTRIBUTING.md)：
- 提 Issue 之前先扫一遍 [ROADMAP.md](./ROADMAP.md) 看是不是已经在做
- 提 PR 必须经过 `cargo test` + `npm run build` 全绿
- UI 改动贴 1 张截图 / 一段录屏

---

## 🔒 安全

发现安全问题请看 [SECURITY.md](./SECURITY.md)，**不要**直接发 Issue。

---

## 💸 支持项目

MIT 免费发行，没有任何功能限制。但如果你用了它节省了 100+ 小时的工时，欢迎赞助：

- GitHub Sponsors：`@Beta-coffee`（占位，待替换）
- 微信 / 支付宝打赏：见 [ROADMAP.md](./ROADMAP.md) 末尾

资金用途：开发用签名证书、付费图标库、招聘兼职维护者。

---

## 📜 许可

[MIT](./LICENSE) — 商用 / 二次分发 / 修改都允许，只要带上版权与许可声明。

如果你公司内用到这个工具并修复了 bug，欢迎 PR 回主仓。

---

## 🙏 致谢

- [Tauri 2](https://tauri.app/) — Rust + Webview 桌面运行时
- [Vue 3](https://vuejs.org/) + [Pinia](https://pinia.vuejs.org/) + [Lucide](https://lucide.dev/)
- [sqlx](https://github.com/launchbadge/sqlx) — 编译期校验 SQL
- IEC 61511 / ISA-5 / GB/T 21109 / GB/T 50770 工作组的所有公开文档
