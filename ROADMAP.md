# 路线图 / Roadmap

> 本文件记录 SIF Studio 各里程碑（M0 ~ M4）的目标、范围、当前状态、验收标准。
> 修改路线图请开 Issue，**不要直接提交**。

---

## M0 — 联锁逻辑图编辑器（单文件 HTML）✅

**状态**：已完成（在另一仓库/分支维护，仓库根目录有 `联锁逻辑图编辑器.html`）

**目标**：交付一份**单文件可双击打开**的工业级联锁逻辑图编辑器。

**关键能力**：
- 6 列 SIF 结构（detect / vote / logic / latch / aux / final），符合 IEC 61511 范式
- 30+ 种元件（PT / LT / FLSL / XV / SOV / TON / BYP / 继电器 / 表决 / 锁存 / 复位 / 试验 …）
- 列型可变量化 `sifCols`，支持增删列
- 走线优化：竖线只走列间空隙、长横线走行间空隙；交叉最小化（`minimizeCrossings`）
- 因果表（C&E）三态（X / - / ·）+ 组合门控 + 自动附注
- SIL 验算（PFDavg / HFT / SFF / 检验周期）对照
- A 系列幅面自动定尺（`sheetInfo`） + 图纸附表（SRS 摘录、SIF 信息带、标题栏）
- 5 色语义（白图 / 蓝图主题切换）+ ISA-101 色彩规则
- 导出 PDF / SVG / PNG / CSV / SRS
- 撤销 / 重做 / 自动贴合 / 列+/列− / 工具栏定制
- 205KB 单文件，零构建

**验收**：72+ 项断言全 PASS、0 JS 报错、3 个回归场景稳定。

---

## M1 — 数据库 + 多图 + 跨图 SIF 汇总 🟢

**状态**：**当前主线**。M1.0 ~ M1.3 完成；M1.4 子任务 10（编辑器 iframe 嵌入）✅；M1.5 测试 + 打包（部分 ⏳）。

**目标**：把 M0 工具"接进" Tauri 桌面应用，让用户的**单文件图**和**真实仪表台账**联通，生成可查询、可出报表、可审查的台账。

### M1.0：Tauri 工程骨架 ✅

- [x] 选型 Tauri 2 + Rust + sqlx 0.8 + SQLite + Vue 3 + Vite + TS + Pinia
- [x] 工程结构：src-tauri 后端 / src 前端 / public 资产 / docs
- [x] 单窗口 1440×900，全黑金属感侧栏
- [x] Tauri capabilities + 安全 CSP

### M1.1：数据库 schema + 迁移 ✅

- [x] 7 表：instrument / project / diagram / sif / sif_instrument / bypass_record / service_ticket
- [x] CHECK 约束：role / sil / demand_mode / sheet_size
- [x] 10+ 索引覆盖高频查询路径
- [x] v001 嵌入式迁移，启动自动跑
- [x] `db_path` 跨平台（%APPDATA% / ~/Library/ / ~/.local/share）

### M1.2：Rust commands + 跨图 SQL 聚合 ✅

- [x] 17 commands：db_version / list_instruments / create_instrument / get_instrument / update_instrument / delete_instrument / count_instruments / list_projects / get_project / create_project / ensure_default_project / list_diagrams / get_diagram / create_diagram / ensure_default_diagram / save_diagram_data / list_sifs / create_sif / delete_sif / get_sif / link_instrument_to_sif / unlink_instrument_from_sif / list_sif_links / import_tags_csv
- [x] `list_sifs` 单 SQL 聚合：`GROUP_CONCAT` + `COUNT(DISTINCT CASE WHEN role=...)` + `COUNT(DISTINCT diagram_id)`，返 SIF 跨图仪表/图号 CSV
- [x] `AppError` 结构化 → 前端 `{kind, message}` 友好提示
- [x] AppState：`Arc<SqlitePool>` 无 Mutex

### M1.3：前端 Vue + Pinia ✅

- [x] 5 路由：/ · /instruments · /sifs · /projects · /diagram/:id
- [x] Pinia store 单一状态源：`stores/studio.ts`
- [x] Home：4 统计卡 + SIL 验算进度 + 最近 SIF + 标准列表
- [x] Instruments：CRUD + 角色/SIL 筛选 + 弹窗表单
- [x] SifDashboard：4 统计卡 + 搜索/筛选 + 主表 + 详情展开 4 块 + 创建/关联弹窗
- [x] ProjectsView：项目卡 + 图管理 + 跳编辑器
- [x] DiagramEditor：iframe 嵌入 M0 + postMessage 通信 + 脏标 + 保存到数据库
- [x] 5 色语义 CSS variables（与 M0 一致）

### M1.4：M0 编辑器嵌入（iframe + postMessage）✅

- [x] 把 205KB `联锁逻辑图编辑器.html` 复制为 `public/editor.html`
- [x] 在 `</script>` 前注入 SIF Studio 桥：`window.exportJson` / `window.importJson` / `postMessage('ready'|'dirty'|'loaded')`
- [x] DiagramEditor：监听 `dirty` → 顶栏 "待保存" 灯亮；点 "保存到数据库" → postMessage `request-save` → 编辑器返 `save-response` → invoke `save_diagram_data`

### M1.5：测试 + 打包 ⏳

- [x] Rust 集成测试 7 例：db_version / tag unique / role check / 跨图汇总 / 级联删除 / 项目级联 / link unlink
- [x] 项目级 README / ROADMAP / CHANGELOG / CODE_OF_CONDUCT / SECURITY
- [x] .github ISSUE_TEMPLATE + PULL_REQUEST_TEMPLATE
- [⏳] 在真实 Windows + macOS 上跑 `cargo test` + `npm run tauri:build` 出安装包（**当前环境无 Rust 工具链 + 无网络，沙盒不能跑编译**）

**验收 M1**：
- 单图 CRUD · 项目多图 · 跨图仪表台账自动汇总 ✅
- SIF 跨图 = 单 SQL 聚合（实测 7 仪表 / 3 图 ⇒ 各列正确）✅
- 删 instrument → 关联 link 自动清 ✅
- 删 project → diagram + sif + link 全部级联 ✅
- iframe ↔ Tauri IPC 双向 ✅
- 集成测试 PASS（环境就绪时） ✅（静态审查）

---

## M2 — 服务报告 / 审计包 🟡 进行中（M2.1 ~ M2.7 已完成）

**目标**：让用户**一键出三份资料**：
1. **检验计划矩阵**：每 SIF 一张表，列出 PFDavg / 检验周期 / 故障模式 / 复测动作
2. **旁路登记册**：含工作票号 / 申请人 / 时长 / 复役状态
3. **SIL 验算报告 PDF**：PFDavg 全图、HFT、SFF、CCF、检验覆盖率、IEC 61511 §11 项 checklist

### 已完成
- ✅ **M2.1 仪表批量导入向导** —— CSV / TSV / XLSX 解析 + 14 组关键词字段映射 + 3 种去重策略（skip / overwrite / create_with_suffix）
- ✅ **M2.2 旁路授权台账** —— IEC 61511-1 §11.5.2 合规校验（双签 + 理由 ≥5 字 + 计划恢复 ≥1h）；`active/overdue/restored` 运行时 SQL `CASE` 计算不落库；全站逾期横幅三级严重度 + 60s 轮询
- ✅ **M2.3 仪表修改历史** —— 字段级 diff（`{before, after, fieldsChanged}` 三段式 payload），红删蓝高亮 + 时间线抽屉
- ✅ **M2.4 SIF / Project 修改历史** —— 协议扩四段式（+`description`）；link/unlink 拓扑变更单独展示；`EntityHistoryDrawer` 通用化接 3 类实体
- ✅ **M2.5 审计包导出（CSV）** —— `audit_log` 全量 + **六维筛选**（表 / 动作 / 操作人 / 起止时间 / 实体 ID）；UTF-8 BOM 让 Excel 双击不乱码；10 列含完整 `before_json` / `after_json` 快照；汇总看板 + 实时预览（300ms 防抖）
- ✅ **M2.6 审计包 PDF 报告** —— 前端 `pdfmake` 0.3.11 + **中文字体子集化**（GB2312 全字集 8764 字 → 2.07MB TTF，懒加载）；A4 横版五章结构（报告说明 / 筛选条件 / 汇总统计 / 详细记录 / 附录）+ ISO 7200 页眉页脚；导出栏双按钮（CSV + PDF）；`scripts/subset-font.cjs` + `verify-font-coverage.cjs` 保证零缺字
- ✅ **M2.7 审计包 PDF 增补图表 + 公司抬头 + 签章位** —— 3 个聚合命令（每日活动 / 旁路时长 / SIL 变更）+ pdfmake `canvas` 矩形条形图（零字体子集依赖）+ ISO 7200 抬头加公司名/项目/版本 + 附录三栏签章位（编制/审核/批准）；12 新测试，**85 passed 总计**，5 页 0 缺字

### 待做
- ⬜ 检验计划矩阵 / SIL 验算报告专属模板（M2.7 之后的可选收尾）
- ⬜ `service_ticket` 表激活（按 kind = maintenance / inspection / failure / audit 区分）
- ⬜ 多 SIF 多图签：M0 编辑器扩展支持一张图画 2~4 个 SIF
- ⬜ 跨项目的 SIF 汇总查询（`project_id IS NULL`）

---

## M3 — 商业化 ⬜

**目标**：在不破坏 MIT 自由发行版的前提下，提供「商用版」：
- 自动升级通道（无侵入：单独 updater.exe，不改主程序签名）
- 付费插件：P&ID 自动识别 / DCS 数据导入 / 多人协作（中央库 + 客户端）
- 品牌定制：企业 logo / 颜色 / 客户专属 banner
- 工单优先支持

**许可策略**：
- **MIT 主仓**：永远免费；编辑器主功能 / 数据库 / 跨图汇总 / 80% 仪表模板
- **闭源插件**：放到 `plugins/` 子目录；单独授权；与 MIT 仓 release 解耦

---

## M4 — OPC UA / 状态看板 / 移动端 ⬜

| 子项目 | 目标 |
|---|---|
| OPC UA 客户端 | 直读 DCS / ESD / FGS 实时状态，仪表台账自动从 P&ID 拍照识别 |
| 状态看板 | 一屏展示所有 SIF 当前状态（绿 = 正常 / 黄 = 旁路 / 红 = 跳闸），运维一瞥即知 |
| 现场改图 | 平板 / 手机触控适配，登录后能直接改联锁图回传；现场巡检 + 即时记录 |
| PLC 交叉引用 | 读 S7 / Studio 5000 项目，把 SIF tag ↔ PLC tag 双向链 |

---

## 💸 资金用途 / How sponsorship money is spent

赞助资金透明公布（每季度 README 追加一节）：
- 代码签名证书（Tauri 打包需要，每年 $200~400）
- 付费图标库（Lucide Pro 等，扩大图标）
- 兼职审稿人 / 文档贡献（按 PR/PR review 一份 $30~100）

不接受：
- 任何限制开源协议的对价
- 强制闭源的"独家合作"

---

## 🤔 决策原则（怎么判断"该不该做"）

| 做 | 不做 |
|---|---|
| 用户故事里 5 个以上独立痛点 | 一个人一句话反馈 |
| 标准 / 法规硬性要求 | "最好有"的锦上添花 |
| 现有方案成本 ≥ 3 人月 / 年 | 已有现成开源方案 |
| 与"运维工程师一站式桌面工具"主线一致 | 偏离主线、专做边缘功能 |
| 维护成本 / 复杂度可控 | 需要引入 5 个新依赖、3 个新数据源 |
| 在中国 / IEC 61511 / ISA-5 场景真实可用 | 仅符合 NIST / 北美 ANSI 标准 |
