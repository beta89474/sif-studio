# 草稿：GitHub Release `v0.1.0` 公告

> 这是一份可直接复制粘贴到 GitHub Release 页面的公告。先在内部过一遍措辞再发。

---

## 标题

**SIF Studio v0.1.0 — 仪表台账 + 联锁逻辑图 + 审计包闭环（首次公开发行）**

---

## 正文

SIF Studio / 联锁工坊 的第一个正式版本 —— 把"画联锁图 + 管仪表台账 + 出审计资料"这三件事
做成一个**完全本地**的桌面工具，对外零网络依赖。

### 🎯 这个版本解决了什么

- **仪表台账的真实性**：之前用 Excel / 表格管仪表，改一处忘了改另一处；SIF Studio 让每台仪表有独立行 + 修改历史（M2.3 / M2.4 字段级 diff）+ 跨项目汇总
- **联锁逻辑图绘制**：M0 编辑器嵌入，独立画图能力 + 导出 SVG / PNG
- **SIF 跨图汇总**：同一台仪表可能在 3 张图里出现 5 次，工具自动汇总它在哪个 SIF 哪个角色
- **年度审计的负担**：审计员来要"过去 12 个月我们改过什么" —— 一键导出 CSV / PDF（带公司抬头 + 签字栏），省 6 小时

### 📦 下载

| 文件 | 大小 | 适用 |
|---|---|---|
| `SIF Studio_0.1.0_x64-setup.exe` | ≈ 2.3 MB | Windows 10 1809+ / 11 单文件安装 |
| `SIF Studio_0.1.0_x64_en-US.msi` | ≈ 3.2 MB | 域推送 / GPO / 静默安装 |
| `Source code (zip)` | — | 仅需审计源码 / 二次开发 |
| `SHA256SUMS.txt` | — | 校验 |

### 🚀 三步上手

1. 下载 `SIF Studio_0.1.0_x64-setup.exe`（≈ 2.3 MB）
2. 双击安装 —— 默认用户级目录，**无需管理员**
3. 启动，进首页 → "新建项目" → 录入几台仪表 → 点"导出审计包 PDF" 试一下

详细步骤：仓库内 [docs/INSTALL-VERIFICATION.md](./docs/INSTALL-VERIFICATION.md)。

### ✨ v0.1.0 主要功能（M0 → M2 全套）

- ✅ **联锁逻辑图编辑器**（M0）：6 列 SIF、30 种元件、走线优化、因果表、A 系列幅面、SVG/PNG 导出
- ✅ **多项目多图管理**（M1）：SQLite 持久化、跨图 SIF 汇总、A0/A1/A2/A3/A4 任意幅面
- ✅ **仪表修改历史**（M2.3）：字段级 diff，创建/更新/删除全留痕
- ✅ **SIF / 项目修改历史**（M2.4）：同一抽屉复用，3 种实体类型
- ✅ **旁路授权台账**（M2.2）：双签 + ≥5 字理由 + ≥1 小时到期；超时按 24h/8h/<8h 三档横幅告警；IEC 61511-1 §11.5.2
- ✅ **审计包 CSV 导出**（M2.5）：六维筛选（表/动作/操作人/时间/实体 ID）+ UTF-8 BOM + Excel 友好
- ✅ **审计包 PDF 报告**（M2.6 + M2.7）：**中文字体子集化**（GB2312 全字集 8764 字 → 2.07 MB TTF）+
  A4 横版五章（封面 / 筛选条件 / 汇总统计 / 详细记录 / 附录）+ **公司抬头** + **3 段图表**
  （每日活动 / 旁路时长 / SIL 变更）+ **签章位**（编制 / 审核 / 批准）

### 🧪 质量指标

- **后端测试**：85 passed（M2.7 含 12 个新测试）—— lib 23 + integration 30 + m22 7 + m23 3 + m24 8 + m25 12 + m27 12
- **前端类型检查**：vue-tsc 0 错
- **构建**：1 次 `npm run tauri:build` 出 MSI + NSIS，全流水线 6.5 分钟
- **PDF 文本提取**：5 页样本 0 缺字（pdfjs-dist 逐字核对）
- **字体子集**：GB2312 6763 汉字 + ASCII + 全角标点 + 工程符号 = 8764 字（2.07 MB，从系统 SimHei 派生，不入仓）

### ⚠️ 已知限制

- **OPC UA 通信**：M3 范围，本版本不内置
- **云同步 / 团队协作**：M3 范围
- **PDF 字体不含 emoji / 罕用繁体字 / 日韩汉字**：GB2312 字集不覆盖。若需要罕用字，改 `scripts/subset-font.cjs` 的"选字"层
- **审计表 ≫ 10 万行未做流式**：若数据规模大，CSV/PDF 在预览阶段可能卡；想做流式改后端 `sqlx::query` streaming
- **installer 不内置 WebView2 引导**：Win10 早期版本需手动装 <https://developer.microsoft.com/microsoft-edge/webview2/>
- **图标**：当前 `productName = "SIF Studio"`，NSIS 安装后的快捷方式是「SIF Studio」。如需公司术语命名，改 `tauri.conf.json` 的 `productName` 后重 build

### 🙏 致谢

- [Tauri 2](https://tauri.app/) · [Vue 3](https://vuejs.org/) · [Pinia](https://pinia.vuejs.org/) · [sqlx](https://github.com/launchbadge/sqlx) · [pdfmake](https://pdfmake.org/)
- IEC 61511-1:2016 · ISA-5.1 / 5.2 · GB/T 21109.1-2007 · GB/T 50770-2013
- 报告字体：SimHei（Windows 自带 / 商用许可 / 未再分发）

### 💬 反馈

- **Bug Report**：[GitHub Issues] → 标签 `bug`
- **功能建议**：标签 `enhancement`，先扫 [ROADMAP.md](./ROADMAP.md) 看是不是已经在做
- **安全问题**：见 [SECURITY.md](./SECURITY.md)，**不要**直接开 Issue
- **交流群**：微信 / QQ 群（占位，待群主决定）

---

## SHA256SUMS

```
9ff7c1a942715d9d9400c3ef3e3dafff920ae109b582141a615e5e28a42cd6da *SIF Studio_0.1.0_x64-setup.exe
e0ba8cc3c69d4a5437b77a3d5c5cc872582dd977107dbc4fa95275d13f4c7bd1 *SIF Studio_0.1.0_x64_en-US.msi
```

> 文件大小：NSIS 4.15 MB / MSI 5.20 MB（含 M2.7 代码 + capabilities 修复）

---

## Tag 签名

构建产物用 git tag 锁版本：

```bash
git tag -s v0.1.0 -m "SIF Studio v0.1.0 — first public release"
git push origin v0.1.0
```

密钥：见运维文档（本机 `~/.ssh/sif-studio-release.asc`）。
