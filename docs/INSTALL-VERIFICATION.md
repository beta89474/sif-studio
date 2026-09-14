# SIF Studio v0.1.0 实装验证清单

> **目标**：在 Windows 上双击 `SIF Studio_0.1.0_x64-setup.exe` → 应用窗口正常起 → 走完
> M2 系列（仪表台账 / 旁路授权 / 修改历史 / 审计包 CSV + PDF）→ 卸载干净。
>
> **适用范围**：v0.1.0 起台账与审计闭环。M3 OPC UA / 移动端不在此范围。
>
> **前置文档**：`INSTALL-Rust-on-Windows.md`（如要在另一台机器上重 build）。

---

## 0. 准备一台"干净"Windows（推荐但非必需）

| 检查项 | 命令 / 操作 | 通过标准 |
|---|---|---|
| 操作系统 | `winver` | Windows 10 1809+ 或 Windows 11（**WebView2 要求**） |
| 架构 | `echo %PROCESSOR_ARCHITECTURE%` | `AMD64`（不要 ARM） |
| 用户权限 | 普通管理员账户（**UAC 不要全程屏蔽**）| installer 要写 `Program Files` 与注册表 |
| 用户名 | 无中文 / 空格 / 特殊字符 | 保险起见；当前测试用户就是 `Beta` 一切正常 |
| 磁盘空间 | `Get-PSDrive C` | installer 安装包 ≈ 2.3 MB；运行后 ≤ 100 MB；建议预留 ≥ 500 MB |
| WebView2 | `Get-ItemProperty -Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"` 看 `pv` | `pv ≥ 100.0.0.0`；Win11 自带，Win10 多数自带，缺了从这里装 <https://developer.microsoft.com/microsoft-edge/webview2/> |
| VC++ 2015+ | 已装 Edge/Chrome 大概率有 | 不必单独验 |

**TL;DR**：Win10 1809+ 或 Win11，64 位，主用户账户，剩 500 MB 空间 = 够了。

---

## 1. 三种安装包，任挑一种

构建完成后，`src-tauri/target/release/bundle/` 下有：

| 路径 | 大小 | 场景 |
|---|---|---|
| `nsis/SIF Studio_0.1.0_x64-setup.exe` | 2.3 MB | **推荐** —— 单文件、装在用户级、可静默、`/S` |
| `msi/SIF Studio_0.1.0_x64_en-US.msi` | 3.2 MB | 适合域推送 / `msiexec /qn` / GPO |
| `sif-studio.exe` (在 `release/`) | 25-30 MB | **跳过 installer 直接跑** —— 不写注册表、不入"应用列表"，开发者尝鲜用 |

> ⚠️ **icon issue**：当前 `tauri.conf.json` 的 `productName` 是 `SIF Studio`，NSIS 安装后的
> 快捷方式名称是「SIF Studio」。如果公司里另有名词约定，改 `productName` 后重 build。

---

## 2. 走真实"筛选 → 导出 PDF"全链路（M2.6 + M2.7 重点）

### 2.1 启动 → 数据能看到

1. **首次启动**（最近一次安装后）
   - 标题栏显示 **「SIF Studio / 联锁工坊」** —— 标识符 `cn.sifstudio.app`
   - 默认打开「首页（Home）」仪表盘；空数据库时显示「暂无项目」+ 引导创建
   - 路径检查：`%APPDATA%\sif-studio\studio.db` 自动创建
2. **导一份种子数据**（如果你没现成数据）
   - 法 ①：菜单 / 启动向导里的"载入示例"按钮
   - 法 ②：菜单位置如有"导入"——选 Excel/CSV 走 M2 前的导入向导
3. **创建项目** —— 进「ProjectsView」新建一个 `P-001`
4. **创建 5-10 个仪表** —— 进「Instruments」 → 类型 / SIL / 用途 / 责任人
5. **创建 2-3 个 SIF** —— 进「SifDashboard」 → 选项目 → 填 SIL 设计 / 验证 / PFDavg 目标 / 检验周期

**期望**：列表行无红字错误、Dashboard 的「按角色聚合」列显示计数。

### 2.2 修改历史（M2.3 + M2.4 串讲）

6. 选一台仪表 → 点「查看历史」→ 右侧抽屉应：
   - 显示「创建 → 更新 → 更新……」倒序时间线
   - 每条审计项下有「变更字段」标签（红删 + 蓝高亮显示新增/改动）
   - 创建/删除行用灰色"全字段快照"块呈现，不走 diff 视图
7. 进 SIF 详情 → 改 SIL 等级（A→B）→ 重新打开历史抽屉 → 应有 1 条 `sil_verified:A → B`

**期望**：diff 表格 0 漏字段；时间不会显示「NaN」或「undefined」。

### 2.3 旁路授权（M2.2 关键路径）

8. 进 SIF → 旁路 Tab → 「新建旁路」
9. 走任意一条有效旁路流程：
   - **bypassed_by** + **approved_by** 都填，且两人不同
   - **reason** 不低于 5 字
   - **planned_restore** 至少为当前+1 小时（用未来时间，如 "2026-09-20 09:00"）
   - 提交成功 → 列表里 state 列应是「未到期」(active)
10. 同 SIF，再建一条 `planned_restore = 2026-09-01`（已过期）→ 上方横幅应是**「CRITICAL 旁路超时」** 红条
11. 恢复那条超时的 → 横幅消失；该行 state 转「已恢复」

**期望**：横幅按 24h/8h/<8h 三档变色（红 / 琥珀 / 灰），不显示「在 5 小时前」这种错位（曾因 SQLite TEXT 时间比较的 'T' vs ' ' 字典序踩过坑，详见 MEMORY.md）。

### 2.4 审计包（M2.5 + M2.6 + M2.7）

12. 进「审计中心（AuditCenter）」（菜单位置，看顶部 nav）
13. 设置筛选：
    - 时间窗口：最近 30 天
    - 动作：勾上 `instrument_create` / `instrument_update` / `sif_create` / `sif_update`
    - 操作人：你登记过的人
    - 实体：`instrument` + `sif`
14. 点 **「预览」**，行数 ≤ 后端 default limit（500 / 1000）—— 若全库行数 > 1 万，日期务必设
15. **导出 CSV**：
    - 真的弹文件保存框（**M2.6 修复了 dialog plugin capabilities**，之前 NSIS 装出来点了无反应）
    - 默认文件名 `audit-P-001-2026-09-13.csv`（含项目代码 + 日期）
    - 列：编号 / 时间 / 操作人 / 动作 / 表 / ID / 变更字段 / 变更前 / 变更后 / 备注
16. **导出 PDF**：
    - 真的弹文件保存框（**同上 capabilities 修复**）
    - 浏览器无下载，**pdfmake 在 Tauri WebView 内直接产 Blob 喂 `<a download>`**
    - 默认文件名 `audit-P-001-2026-09-13.pdf`
    - 默认 5 页：A4 横向，**封面抬头有公司名 + 项目名 + 报告版本 + ISO 7200 标头**
    - 图表章节有 3 段：每日活动 / 旁路时长 / SIL 变更
    - 末尾 3 栏签章位（编制 / 审核 / 批准）
    - **用任意 PDF 阅读器打开**：所有中文正常显示，不应该有方框 □（详见 M2.6 字体子集化）
17. （可选）把 PDF 邮件发出去 / 打印预览 / 在 Foxit / Adobe Reader 里打开确认

**期望**：PDF 在 Explorer 双击直接被默认 PDF 阅读器打开，看到全部中文字符正常。

---

## 3. 关键风险点 / 已修复坑

| 现象 | 原因 | 已修复？ |
|---|---|---|
| 点"导出"按钮没反应 | `dialog:default` 权限缺，Tauri 2 IPC 静默拒绝 | **是**（本次实装前补 `capabilities/default.json`） |
| PDF 中文显示为□ | 字体未子集化 / 子集不全 | 是（GB2312 全字集 8764 字） |
| 旁路「超时」状态错位 | SQLite TEXT 时间 RFC 3339 'T' vs ' ' 字典序错 | 是（`<` 比较两侧都套 `datetime()`） |
| 编辑器画面空白 | M0 iframe 桥未注入 export/import 钩子 | 是（注入锚点 `layoutCols...fitSheet();` 末尾） |
| installer 大小异常大 | 没把 `pdfmake` + TTF 拆 chunk | 是（vite `manualChunks` 拆出 `pdfmake` chunk） |

---

## 4. 卸载 / 数据清理

- **应用卸载**：「设置 → 应用 → SIF Studio → 卸载」—— 完全干净，**`%APPDATA%\sif-studio\` 不自动清**
- **数据保留 / 清除**：
  - 想留数据备查 → 仅卸载应用，DB 文件留在 `%APPDATA%\sif-studio\studio.db`
  - 想彻底清 → 卸载后再手动删 `%APPDATA%\sif-studio\` 整个目录
- **下次安装再启动** → 同一个 `%APPDATA%` → 之前的数据**还在**，能直接接着用

**警告**：DB 包含全部仪表台账 + 审计包记录。生产环境数据删了不可恢复。

---

## 5. 实装验证通过 = 发版门槛

| 必须全过 | 推荐过 |
|---|---|
| ✅ 启动正常 + 数据持久化 | ✅ PDF 邮件分享可读 |
| ✅ M2.3 / M2.4 历史抽屉可见 diff | ✅ 卸载干净无残留 |
| ✅ M2.2 旁路三档严重度横幅 | ✅ 多 SIF / 多项目切换不串数据 |
| ✅ M2.5 CSV 真存到选择的路径 | |
| ✅ M2.6/M2.7 PDF 真保存到选择的路径 + 中文 0 缺字 | |

**任一"必须"失败 → 不发版 → 提 issue 标注 `m2.x-regression`。**

---

## 6. 已知限制（v0.1.0 不解决，但要在 release notes 写）

- **不内置 OPC UA 通信** —— M3 范围
- **不内置云同步 / 团队协作** —— M3 范围
- **PDF 字体子集不含 emoji / 罕用繁体字 / 日本汉字** —— GB2312 字集（6763 汉字 + 686 符号 = 8764 字）
  - 想覆盖罕用字 → 改 `scripts/subset-font.cjs` 的"选字"层，纳入常用 7000 罕用字扩展
- **审计导出目前无后端流式** —— 若 audit 表 ≫ 10 万行，CSV/PDF 都走 `fetchPreview` 取内存 → 可能卡
  - 想流式 → 后端改 `sqlx::query` streaming + chunked output
- **installer 不内置 WebView2 引导下载** —— Win10 早期版本需手动装
