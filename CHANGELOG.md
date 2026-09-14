# Changelog / 变更日志

格式仿 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)。
本项目遵 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

---

## [Unreleased]

### Changed — M2.9 #2 画图 picker 接项目仪表台账（硬编码 INSTRUMENT_DB 退役）
**用户需求续**：第 3 步「画图时从本项目台账挑起因/检测元件 + 最终/动作元件」落地。

- **M0 编辑器（`public/editor.html`）**：
  - `const INSTRUMENT_DB` → `let INSTRUMENT_DB`（内置 demo 只作启动兜底）
  - 桥新增入站消息 `inject-instruments`：payload 支持 `groups[]`（推荐）或扁平 `items[]`，
    收到后覆盖 `INSTRUMENT_DB` → `renderInst()` → 弹窗开着时重渲 `openPicker()`
  - 新增出站回执 `instruments-injected { projectId, count }`
  - 载荷清洗 `sanitizeItem()`：type 白名单（不认识的退 `di`）+ `id` 透传
  - **`instrumentId` 贯穿落块**：`sanitizeItem → addFromDb/pickDb → addBlock` 写入
    `block.instrumentId`。这是 **M2.9 #3 强外键的地基**（本项目已选强外键方案）
- **前端（`src/views/DiagramEditor.vue`）**：
  - `ready` 时先发 `load-diagram`，再 `void pushProjectInstrumentsToIframe()`
  - `pushProjectInstrumentsToIframe()`：`await refreshInstrumentsByProject(diagram.projectId)`
    → 取 store 分片 → **深拷成纯 JS 对象**再 postMessage
  - `buildInstrumentGroups()`：过滤 logic/aux → 按 `kind` 分桶、桶内 tag 升序
  - `mapInstrumentToType()`：`role + kind`（+ tag 前缀兜底）→ M0 block type
    - detector：变送器/热电偶 → `ai`；开关/火焰/位置 → `di`；按钮 → `hs`；ESD/命令 → `comm`
    - final：阀类 → `xv`；电机/泵 → `motor`；电磁阀 → `sov`；声光 → `alarm`；其余 → `do`
  - 失败降级：拉取异常 → 发空集，picker 退回内置 demo，画图页不崩
- **Preview（`mock-tauri.js`）**：
  - 12 条仪表补 `projectId`（项目 1 十条 + 新增项目 2 两条 PT-201/XV-301）
  - 新增 `list_instruments_by_project` / `count_instruments_by_project`（深拷 + 过滤后再深拷）
  - `commit_import_instruments`：目标项目必填；`(projectId, tag)` 联合唯一（对齐 SQL）
- **测试**：`verify-m292.js` **35/35 PASS**（注入隔离 / 路由切换 / 面板 / picker / type 映射 / instrumentId）
  + `verify-m292-inst.js` **6/6 PASS**（仪表页项目筛选回归）
  + `verify-interlock.js` **28/28 PASS**（M2.8.2 全量回归）
  + `cargo test` **108 passed**；`vue-tsc` 0 errors；`vite build` 1540 modules

**踩坑（值得记）**：`postMessage` 的 structured clone **不能直接吃 Vue 的 reactive proxy** ——
会抛 `[object Object] could not be cloned`，而 catch 到的 `e` 打印成 `{}`（无 enumerable 属性），
看起来像「空错误」。必须 `JSON.parse(JSON.stringify(list))` 转纯对象再发。

待续：M2.9 #3-#7（block ↔ instrument 强外键写库、删除拦截、SIF 自动聚合、反向追溯页）。

### Changed — M2.9 项目-仪表-图三段打通（#0 + #1 落地）
**用户需求**：建项目 → 项目里加仪表台账 → 画图时从台账挑起因/检测元件 + 最终/动作元件。

- **仪表按项目隔离**（用户拍板方案）：
  - 新迁移 `003_project_scoped_instruments.sql`：加 `instrument.project_id` + DROP 全局 UNIQUE 索引、改联合 `(project_id, tag)` 唯一
  - 自动建 `PRJ-LEGACY` 兜底项目，迁所有旧 NULL 进去（用户可手动重指）
  - 新命令 `list_instruments_by_project(pid)`、`count_instruments_by_project(pid)`
  - `create_instrument_inner` 校验 project_id ≥1 且项目存在
  - `imports.rs` 全部 3 种策略（skip / overwrite / create_with_suffix）按 project_id 隔离
- **Front-end**：
  - `Instrument.projectId: number | null` TS 类型
  - `Instruments.vue`：项目筛选器 + 表头「所属项目」列 + 弹窗「归属项目 *」必选
  - `ImportWizard.vue`：第 3 步「目标项目」选择器
  - `studio.ts`：`refreshInstrumentsByProject(pid)` action、`instrumentsByProject(pid)` getter
- **测试**：新增 `tests/m29_instruments_scope_test.rs`（10 例）+ 更新现有测试
- **cargo test**：108 passed（+10 新增，零回归）
- **vue-tsc**：0 errors；**vite build**：OK

待续：M2.9 #2-#7（画图 picker 接项目仪表、block ↔ instrument 强外键、删除拦截、SIF 自动聚合、反向追溯页）。

### Changed — M2.8.4 联锁图编辑器布局优化（字更大 · 侧栏可折叠 · 画布优先）
> 用户反馈：「联锁逻辑图里的图元素字体太小，编辑图页面左侧、右侧边栏页抢占了位置，
> 我希望能结合该应用的布局特色进行优化，让联锁逻辑图的布局更合理，显示字更大，操作更方便」。

**根因：不是「字设小了」，是「整幅适配把字压碎了」**

元件里 tag 原本 12.5px 并不算小，但 `fitSheet()` 启动时会把整幅 2404px 宽的图纸
硬压进绘图区宽度 —— 左右侧栏（238+304=542px）在 1400px 窗口下吃掉 38%，绘图区只剩
958px，于是缩放被压到 **50%**：12.5px 的 tag 实际只显示 **6.25px**。
「侧栏抢位置」与「字太小」是同一个问题的两面。

**修复：三条杠杆同时发力**

1. **侧栏可折叠 + thin rail 唤回**（`editor.html`）
   - 左右侧栏均可折叠为 0 宽；合上后留一根 **32px thin rail**（垂直文字 + 方向箭头）
   - 工具栏新增「元件库 ▸」「◂ 属性」两个开关（`on` 态 = 面板可见，文案随态变化）
   - 折叠/展开走 180ms 过渡（能看清谁在动），状态写入 `localStorage`（`sif-editor.layout.v1`）
   - **默认折叠右侧属性面板**（上下文面板，选中元件才需要），左侧元件库保留展开 ——
     直接回应「侧栏抢占了位置」，同时不牺牲「拖元件」主流程
   - 属性面板折叠且选中元件时，rail 亮起琥珀色脉冲 3 次（`syncRailNotify()`），
     指向被藏起来的属性，避免「点了元件却找不到属性在哪」

2. **缩放不再无下限**（`fitSheet()`）
   - 新增 `MIN_ZOOM = 0.7`：整幅适配低于它时就不再缩，宁可出滚动条
   - 工程图编辑的第一需求是「看清元件文字」，其次才是「一眼看全」
   - 结果：缩放 **50% → 70%**，配合字号上调，元件文字实际显示大小 **6.25px → 9.8px（+57%）**
   - 折叠侧栏后绘图区变宽 → `refitAfterLayout()` 重算缩放（`transitionend` 为主、
     300ms 兜底，不靠 setTimeout 赌时序）
   - 工具栏新增 **「适配」** 按钮 → `fitSheet(true)`：**看全优先**，可低到 0.5。
     与启动的「字优先」分成两个入口，语义不混：
     | 入口 | 行为 |
     |---|---|
     | 启动 / 折叠侧栏后 | 字优先，不低于 `MIN_ZOOM` |
     | 点「适配」按钮 | 看全优先，缩到整幅可容纳（可能字变小） |

3. **元件字号整体上调 10~15%**（`drawSensor` / `drawFinal` / `drawLogic`）

   | 行 | 原字号 | 新字号 | y 位置 |
   |---|---|---|---|
   | tag（位号） | 12.5 | **14** | y+24 |
   | desc（描述） | 10 | **11.5** | y+40 |
   | l3（SP/接点/表决） | 9.5 | **11** | y+54 |
   | l4（失效位/位置） | 9 | **10.5** | y+66 |
   | 逻辑符号 / 表决符号 | 13 / 17 | **14 / 18** | — |
   | 仪表气泡首字母 | 13 | **14** | — |
   | 失效位徽章 / 表决徽章 | 8.5 | **9.5** | — |
   | 端口命中区（点选） | 6px | **8px** | — |
   | 选中角标 | m=12 | **m=14** | — |

**顺带修掉一个既有缺陷：文字溢出元件框**

`fitStr()` 只按**字符数**截断，中英混排时会误判 —— 20 个中文 ≈ 20em，20 个
`PT-101` ≈ 11em，同样 20 字符宽度差近一倍。字号一放大，这种误判就变成肉眼可见的
溢出（实测 desc 溢出 **50px**、l4 溢出 **59px**）。

- 新增 **`fitW(s, 可用像素宽, 字号)`**：按 CJK/全角 = 1.0em、其余 = 0.55em 估算视觉宽度
- `drawSensor` / `drawFinal` / `drawLogic` 的 tag/desc/l3/l4/bypTag/bypAuth/sub
  全部改用 `fitW`（传入列宽推导的可用宽度，列宽变化时自动跟随）
- `fitStr` 保留给附表 / 图例等既有调用点，行为不变

**同步维护**：`scripts/sync_preview.py` 原先只同步 `dist/assets` 与 `index.html`，
导致改了 `public/editor.html` 后预览仍是旧版（改动「看不见」）。现补上 `editor.html` 复制。

**验证**
- `vue-tsc --noEmit` 0 错；`vite build` 1540 modules
- `sif-studio-preview/verify-m284.js` **23/23 PASS**：默认右栏折叠且左库 238px、
  折叠/展开/rail 唤回三态、localStorage 持久化跨 reload、缩放 50%→70%、
  四档字号落位、rail 通知亮起与撤销、「适配」按钮降为整幅、工具栏未换行
- `sif-studio-preview/measure-overflow.js`：元件文字溢出 **2 处 → 0 处**
- **真实 Tauri 应用** `sif-studio-preview/verify-app-m284.js` **14/14 PASS**
  （CDP 连入 WebView2，确认折叠按钮在 CSP 下可用、0 CSP 违规）
- 交互回归 `sif-studio-preview/verify-interlock.js` **28/28 PASS**（布局改动零功能回归）
- 实测数据：图纸宽 2404px 由「高 1700 × √2」决定（附表仅占 9%），
  用户 1440px 窗口下显示滚动 **1.44×** —— 这是「字优先」的可接受代价

### Fixed — M2.8.3 联锁图编辑器「几乎所有元素点击都不起作用」根因修复（Tauri CSP）
> 用户反馈：「联锁图编辑页的几乎所有元素点击都不起作用，我无法编辑联锁逻辑图」。

**根因：Tauri 的 CSP 拦截了 editor.html 的全部内联事件处理器**

`editor.html` 是 M0 遗留的单文件编辑器，交互几乎全靠内联属性（`onclick="quickAdd('ai')"`、
`oninput="renderLib()"`）与内联样式（`style="display:none"`）—— 共 **81 处事件 + 80 处 style**。

`tauri.conf.json` 里虽然写了 `script-src 'self' 'unsafe-inline'`，但 **Tauri 构建时会自动扫描
HTML、为每段内联脚本/样式算出 hash 与 nonce 并注入 CSP**。按 CSP3 规范：

> `'unsafe-inline'` is ignored if either a hash or nonce value is present in the source list.

于是 `'unsafe-inline'` 被**静默作废**，所有内联事件处理器被浏览器拒绝执行：

```
Executing inline event handler violates the following Content Security Policy directive
'script-src 'self' 'unsafe-inline' 'sha256-…' …' The action has been blocked.
Applying inline style violates the following Content Security Policy directive 'style-src …'
```

**为什么之前所有验证都没发现**：浏览器预览（`python -m http.server`）**没有 CSP**，
`preview/app/*` 与 `dist/*` 逐字节一致（md5 相同）却表现完全不同 —— 差别只在
**Tauri 运行时注入的 CSP 响应头**。这个 bug 只在**真实安装的 Tauri 应用**里复现。

**症状分布**（正好对应 `addEventListener` vs 内联属性的分界）：

| 交互 | 绑定方式 | 修复前 |
|---|---|---|
| 工具栏全部按钮（撤销/行±/列±/缩放/规格/因果表/图签/SVG/PNG/CSV/SRS/打印/主题/网格/图框/空槽/附表/贴合/连线模式/删除/清空） | 内联 `onclick` | ❌ 全失效 |
| 元件库列表项、侧栏三个 tab、搜索框 | 内联 `onclick`/`oninput` | ❌ 全失效 |
| 属性面板全部输入控件 | 内联 `oninput`/`onchange` | ❌ 全失效 |
| 全部弹窗（SIF 规格/因果表/修订/图签/列选择器/元件选择器） | 内联 `onclick` + `style` 属性 | ❌ 全失效 |
| 画布元件选中 / 起线 / 拖动 | `addEventListener` | ✅ 唯一能用 |

用户所说「几乎所有元素点击都不起作用」由此得到完整解释。

**修复**（`src-tauri/tauri.conf.json`）
```jsonc
"security": {
  "csp": "default-src 'self' ipc: http://ipc.localhost; script-src 'self' 'unsafe-inline'; \
          style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: asset: https://asset.localhost; \
          font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost ws://localhost:5173 http://localhost:5173",
  "dangerousDisableAssetCspModification": true   // ★ 关键：禁止 Tauri 注入 hash/nonce，
                                                 //   否则再写 'unsafe-inline' 也会被规范作废
}
```
- `dangerousDisableAssetCspModification: true` 关闭 Tauri 的 CSP 自动改写，
  保留一份基础防护（`default-src 'self'`）的同时让内联事件与样式可用
- 顺带补 `img-src blob:`（PNG 导出预览）与 `connect-src ws://localhost:5173`（dev HMR）
- 未引入 `'unsafe-eval'` —— 已确认 `editor.html` 无 `eval` / `new Function`

**验收方式升级**：新增 `sif-studio-preview/verify-app-cdp.js` —— 用
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` 启动**真实安装的应用**，
puppeteer 经 CDP 连入 WebView2 驱动点击并统计 CSP 违规。修复前 **1 PASS / 6 FAIL**，
修复后 **6 PASS / 0 FAIL + CSP 违规 0 条**（第 7 项「行 +」在默认「贴合」模式下会被自动收回空行，
属既有设计，非缺陷）。

### Fixed — M2.8.2 联锁逻辑图「打开即空白、保存等于清空」根因修复
> 用户反馈：「联锁逻辑图功能还是几乎不可用」。实地用 puppeteer 跑完整流程后定位到
> 一条完整的**静默清空画布链路**——不是 UI 问题，是持久化契约错位。

**根因（一条链，四个环节都缺守卫）**
1. `migrations/001` 的 `diagram.data DEFAULT '{}'`，且 `ensure_default_diagram` 自造了
   一个**外来 schema** `{"version":1,"title":…,"sifs":[…]}` —— 里面**没有 `blocks`**。
2. `editor.html` 的桥只看「是不是以 `{` 开头的字符串」，于是把它当成有效快照。
3. `restore()` 里 `blocks = s.blocks || []` —— `blocks` 缺失就**落成空数组**，
   `catch(e){}` 把一切异常吞掉，画布被静默清空、用户毫无提示。
4. 用户看到空白画布 → 随手保存 → `save_diagram_data` 把**空快照**写回库
   （实测用户库中唯一一行：`blocks: []`，1195 字节全是元数据）→ 此后每次打开都是空白。
5. 预览侧还有两个帮凶：mock 的 `get_diagram` 返回 `'{"nodes":[],"wires":[]}'`（同一类外来
   schema），且 `save_diagram_data` 是**空实现** → 「保存成功」是假的，M2.8 的截图全部失真。

**修复**
- `editor.html` 快照契约显式化：
  - `snap()` 加版本号 `v:3`；新增 `isSnapshotJson()` —— **`blocks` 是数组 = 唯一有效性判据**
  - `restore()` 加守卫：缺 `blocks` 数组 → 原样返回 `false`，**绝不执行 `blocks=[]`**
  - `window.importJson()` 改为返回 `true/false`（原来无论成功失败都 `return true`）
  - `loadJson()`（文件导入）同样走守卫，非法文件明确提示而不是清空画布
  - `saveJson()` 改用 `snap()` 同一 schema —— 旧的 `{v:2,…}` 少 5 个键，
    「导出 → 再导入」会静默丢 `silv/showSch/autoFit/showSlot/wireMode`
- 桥层三态装载 + 回退：`load-diagram` 区分「已应用 / 外来 schema / 无内容」，
  不可用时**保留内置示例图当起点**并回报 `loaded{applied:false, reason}`，
  父窗口据此把状态标成 DIRTY + 顶栏提示「尚未保存」
- **旧数据自动恢复**：`blocks` 为空**且无版本号**的存档判定为 `legacy-empty`
  （= 修复前被清空过一次的残骸），换回示例图 —— 用户库里那条坏行无需迁移即可自愈；
  而 `v3+` 的空数组才是「用户主动清空」，予以尊重
- 工具栏新增 **「示例」** 按钮（`loadTemplate()`，可撤销）：空白画布随时可一键取回示例
- 工具栏 `保存`/`载入` 更名 **`导出 JSON`/`导入 JSON`**，并在嵌入模式（`body[data-embedded]`）
  下隐藏 —— 原先外壳「保存到数据库」与编辑器「保存（下载文件）」两个同名按钮语义相反
- `MutationObserver` 目标 id 修正 `canvas-wrap` → **`canvasWrap`**（取不到时退化成观察
  `document.body`，任何无关 DOM 变化都误报 dirty）
- 新增 `dirtyMutedUntil` 抑制窗口：装载快照本身引起的 DOM 变化不再被误报成「用户编辑」
- `DiagramEditor.vue`：按 `loaded.applied` 分派 SAVED/DIRTY；新增 `loadNote` 顶栏说明；
  新增路由参数 `watch` —— 同一 route record 会**复用组件实例**，原先「图 A → 图 B」
  不会重新装载（画布仍是上一张图）
- `diagrams.rs`：`ensure_default_diagram` 的 `data` 由外来 schema 改为 `"{}"`
- mock：`get_diagram`/`save_diagram_data` 走独立 `diagramData` 映射形成**真实往返**；
  `diagramData[2]` 复刻用户库里那条坏存档，供回归验证

**验证**
- `cargo test` **98 passed**（新增 `save_diagram_data_roundtrips_editor_snapshot_verbatim`；
  `ensure_default_diagram_creates_placeholder_when_empty` 增加「默认 data 不得含 blocks 数组」断言）
- `vue-tsc --noEmit` 0 错；`vite build` 1540 modules
- 端到端 puppeteer 验收 `sif-studio-preview/verify-interlock.js` **28/28 PASS**：
  守卫拒绝全部外来 schema（4 类）且不清空画布、打开无快照的图回退示例、保存 →
  切走切回仍是已保存内容、加元件后往返仍是 17 元件、旧空存档自愈、v3 空图被尊重

### Added — M2.1 仪表批量导入向导
- **数据库迁移 `002_audit_log.sql`**：新增 `audit_log` 表（append-only 审计，M2.2 旁路授权 / 修改历史共用）+ 4 索引
- **`src/import.rs` 解析层**（~430 行）
  - CSV / TSV 流式解析（`csv` crate）+ XLSX / XLS / XLSB / XLSM（`calamine` 0.26）
  - 编码自动探测：UTF-8 → GBK 回退（`encoding_rs`）
  - 列类型推断：`number` / `text` / `empty` / `mixed`
  - **字段映射建议**：14 组关键词词典（中英文近义词），`range_min` / `range_max` 优先于 `range`
  - 单元测试 5 例
- **`commands/audit.rs`**：`write_audit()` / `write_audit_best_effort()` / `write_audit_state()`；actor 默认取 OS 用户名
- **`commands/imports.rs`**（~460 行 + 测试）
  - `preview_import_instruments(file_path)` → `ParsedSheet`（headers + 20 行预览 + 映射建议 + 全量行）
  - `commit_import_instruments(input)` → 事务批插入 + 审计
  - **三种去重策略**：`skip` / `overwrite` / `create_with_suffix`（自动找 `_N` 不冲突后缀）
  - 整批 sqlx 事务；每 500 行子批提交；行级失败记录不阻断其它行
  - 上下限保护：50,000 行；必填 `tag` mapping 校验
  - 单元测试 3 例
- **前端 `views/ImportWizard.vue`**（~700 行，含样式）
  - 三步向导：`01 UPLOAD` → `02 MAPPING` → `03 COMMIT`
  - 拖拽 / 点击选文件；ISO 7200 标题栏 + 步骤条
  - 映射表格（列名 / 类型徽章 / 样例值 / 下拉改映射）+ 数据预览折叠
  - 去重策略单选 + 提交结果四格统计 + 失败行明细表
  - 工业图纸风完整样式（零圆角 / 1px 黑边 / 等宽数字）
- **Pinia store 扩展**：`importSheet` / `importMapping` / `importStrategy` / `importResult` / `importing` + 4 actions
- **`Instruments.vue` 加「↥ 批量导入」按钮** → `/import` 路由
- **集成测试 +7 例**（共 **22 例全通过**）
  - CSV 全新建、overwrite、suffix、无效 role 单行失败、审计链完整性、无 tag mapping 拒绝、超行数拒绝
- 单元测试共 **8 例**（原 0 → 8：解析层 5 + draft 校验 3）

### Added — M2.2 旁路授权台账（IEC 61511-1 §11.5.2）
- **`src-tauri/src/commands/bypasses.rs`**（~590 行，含单元测试）
  - 5 个 commands：`list_bypasses` / `create_bypass` / `update_bypass` / `restore_bypass` / `delete_bypass`
  - **合规校验**（IEC 61511-1 §11.5.2 强制）：
    - `approved_by` 必填（批准人 = 管理变更 MOC 的签署人）
    - `bypassed_by` 必填（操作人）
    - `reason` ≥ 5 字（防"清理""测试"这类无意义理由）
    - `planned_restore` 必须 ≥ 1 小时后（防意外设过期时间 → 旁路永久挂着）
    - SIF 必须存在且属于指定 project（防跨项目误关）
    - 仅**未恢复**的旁路可 update / restore（已恢复 → `Conflict`）
    - restore 时间不早于 planned_restore 前 24h（现场手动改时间容差）
  - **状态计算**（运行时 SQL `CASE`，不存冗余列）：
    - `active` = 未恢复 且 计划恢复时间 ≥ now
    - `overdue` = 未恢复 且 计划恢复时间 < now（**到期未恢复 = 合规风险**）
    - `restored` = 已恢复
  - **`hours_to_restore`** 由 `julianday()` 差分 × 24 计算，正 = 距到期，负 = 已逾期
  - `list_bypasses` 支持 `status`（active|overdue|restored|all）+ `projectId` 双重过滤
  - **审计**：`bypass_create` / `bypass_update` / `bypass_restore` / `bypass_delete` 全写 `audit_log`（复用 M2.1 基础设施），`payload_json` 带 SIF / 操作人 / 批准人 / 票号 / 理由摘要
  - 单元测试 **4 例**（approved_by 必填 / reason 长度 / planned_restore 时间窗 / `+08:00` 时区解析）
- **前端 `views/BypassLedger.vue`**（~560 行）
  - 4 统计卡：活动 / 逾期 / 已恢复 / 总数（逾期或活动 > 0 时数字转红）
  - 状态分段过滤（全部 / 活动 / 逾期 / 已恢复）+ 项目下拉
  - 主表格：状态徽章 / SIF / SIL / 项目 / 操作人 / 批准人 / 票号 / 理由 / 计划恢复 / **剩余-or-逾期时长**（按紧急度着色：>24h 绿 / ≤24h 琥珀 / 逾期红）
  - 逾期行整行红底 + 左侧红边带（图纸红色警示线）
  - 新建 / 编辑弹窗（内嵌错误横幅显示后端校验消息）+ 恢复弹窗（可改恢复时间）
  - 工业图纸风：零圆角 / 1px 黑边 / 等宽 `tabular-nums`
- **前端 `components/BypassCard.vue`**（嵌入式旁路卡）
  - 给 `SifDashboard` 详情用：列某 SIF 下活动/逾期旁路（状态徽章 + 剩余时长 + 操作人 + 理由）
  - 零活动旁路显示 "无活动旁路"
- **`App.vue`**：侧栏加 `05 旁路授权`（`ShieldAlert` 图标）+ topbar section `06 / BYPASS REGISTER` + 页脚 `DWG-006`
- **`Home.vue`**：第 5 张统计卡「活动旁路 · ACTIVE BYPASS」（逾期 > 0 时红色 + 副标注"X 个逾期"）；hero 区条件显示"旁路台账 (N)"快捷入口
- **`SifDashboard.vue`**：详情区新增「活动旁路 · ACTIVE BYPASS」块（`BypassCard` 嵌入）+「登记新旁路 →」跳转
- **Pinia store 扩展**：`BypassListItem` / `BypassInput` / `BypassUpdate` / `BypassStatus` 类型 + `bypasses` / `bypassesFilter` / `bypassesLoading` state + 4 getters（`activeBypasses` / `overdueBypasses` / `restoredBypasses` / `bypassesBySif`）+ 6 actions
- **集成测试 +7 例**（共 **21 例全通过**）
  - 必填校验（approved_by / reason / planned_restore / sif 存在性 / project 匹配）、创建审计、恢复写 restored_at、已恢复不可再恢复、状态过滤（active/overdue/restored + 项目 + 非法值）、更新审计 + 已恢复不可改、CASCADE 删 SIF 清旁路
- **浏览器预览 mock**：`mock-tauri.js` 加 4 条真实感旁路数据（活动 / 逾期 36h / 已恢复 / 跨项目）+ 5 个 command 实现（含校验抛错）
- 单元测试共 **12 例**（原 8 → 12：新增 bypass 校验 4 例）

### Added — M2.2 增强：启动扫 overdue 告警横幅（合规缺口不能静默）
- **`commands/bypasses.rs` 加 `count_overdue_bypasses`**（轻量命令）
  - 只返回 `{ count, oldestOverdueHours }`，不拉详情 → 启动期开销可忽略
  - 单条 SQL：`COUNT(*)` + `MAX(julianday diff × 24)` over 未恢复且已超期的行
  - 单元实现导出 `count_overdue_bypasses_inner` 供测试调用
- **修正三处 SQL 时间比较缺陷**（M2.2 遗留，本轮暴露）
  - 症状：`planned_restore` 列存 RFC 3339（`2026-09-13T01:07:35Z`，**T** 分隔），而 `datetime('now')` 返空格分隔（`2026-09-13 06:07:35`）
  - SQLite 回退字符串比较 → `'T'(0x54) > ' '(0x20)` → 5 小时前的时间被判为"未到期"
  - 修正：`datetime(b.planned_restore) < datetime('now')`，两边都规范化；影响 `list_bypasses` / `create_bypass` 返回 / `get_one` 三处 CASE
  - 另：语义修正 —— "最久逾期"应用 `MAX` 不是 `MIN`（`oldestOverdueHours` 实为最久）
- **前端 `components/BypassAlertBanner.vue`**（新组件，全站横幅）
  - `count = 0` → 整条不渲染（不制造噪音）
  - 三级严重度：≥24h 红底 `CRITICAL` / ≥8h 琥珀 `WARNING` / <8h 淡琥珀 `ADVISORY`
  - 显示"最久逾期"时长（`1 d 12.0 h` 格式）+ 直达 BypassLedger 按钮 + 临时关闭（60s 后再提醒）
  - 取数失败降级：不弹错、不崩启动、保留上次值
- **`App.vue` 挂载横幅** + `.topbar-with-alert { margin-top: 56px }` 让位给 fixed 横幅
- **`Home.vue` 活动旁路卡** footer 从"N 个逾期"升级为"N 个逾期 · 最久 1 d 12.0 h"
- **Pinia store 扩展**
  - `OverdueBypassStatus` 类型 + `overdueBypassStatus` state
  - `refreshOverdueBypassStatus()` action + `startOverduePolling()` / `stopOverduePolling()`（**60 s 轮询**）
  - `bootstrap()` 并行加一次 overdue 拉取 + 启动轮询
- **浏览器预览 mock**：`mock-tauri.js` 加 `count_overdue_bypasses` 实现（与 Rust 语义一致，含 `restored_at` 排除）
- **集成测试 +3 例**（共 **24 例全通过**）
  - 空库返 0、4 样本（active/逾期 5h/逾期 36h/已恢复）只数 2 + MAX 断言 35.5~36.5h、已恢复即使 planned 在过去也不计
- **截图证据 +5 张**：横幅特写（CRITICAL 态）/ Home 逾期提示 / 仪表台账 / SIF 汇总 / 旁路台账（跨页一致）

### Added — M2.2 收尾：接入 `@tauri-apps/plugin-dialog`（真实文件路径）
- **Rust 依赖 + 注册**
  - `Cargo.toml` 加 `tauri-plugin-dialog = "2"`（实际拉 2.7.3）
  - `tauri.conf.json` 加 `"plugins": { "dialog": {} }`
  - `lib.rs` `.plugin(tauri_plugin_dialog::init())`
  - **无需新增 Rust command** —— `preview_import_instruments(file_path)` 早已用 `std::fs::read(path)` 直读路径
- **`views/ImportWizard.vue` 双路径架构**
  - `isTauriEnv` 判定：`__TAURI_INTERNALS__` 存在且 `!__mocked` → 真 Tauri 环境
  - **真 Tauri**：点击 dropzone → `pickTauriFile()` → `@tauri-apps/plugin-dialog` 的 `open()` 拿**绝对路径** → `previewFromPath(path)`
    - 动态 `import()` 保证浏览器环境不加载 native 模块（vite 默认 chunk split）
    - 过滤器限定 `csv/tsv/xlsx/xls/xlsb/xlsm`，标题「选择仪表台账文件」
    - 用户取消（返 `null`）静默无操作
  - **mock / 浏览器**：保留 `<input type=file>` + drag-drop 原路径（与 M2.1 行为零差异）
  - 新增 `importedPath` 状态：显示 `TAURI PATH · C:\...\xxx.csv` 确认拿到真实路径
  - 真 Tauri 下隐藏 `<input>`（`v-if="!isTauriEnv"`）；拖拽时引导走 dialog（OS 沙箱不允许外部拖入）
- **`sif-studio-preview/app/mock-tauri.js` 增强**
  - 加 `?nativeui=1` 开关 → `__mocked = false`，让浏览器能真实演示"真 Tauri 环境 UI 分支"
  - 加 `'plugin:dialog|open'` mock：返回预设绝对路径 `C:\Users\Engineer\Documents\instruments-fy26.csv` 并注册对应 CSV 内容
  - query 检测同时覆盖 `location.search` 与 `location.hash`（hash 路由下 query 可能落在 `#/` 之后）
- **截图证据 +4 张**：mock fallback 态 / 真 Tauri 空态（引导文案）/ 点击后 SOURCE 显示绝对路径 + 10 列全自动映射 / 映射表全页

### Added — M2.3 仪表修改历史（字段级 diff，IEC 61511 / ISA 84 变更追溯）
- **审计 payload 协议立标**（M2.3 起新业务统一，M2.2 bypass 历史暂不改 → 向后兼容）
  - `instrument_create` → `{before: null, after: <full row>, fieldsChanged: ["*"]}`
  - `instrument_update` → `{before: <full row>, after: <full row>, fieldsChanged: ["fieldA", ...]}`
  - `instrument_delete` → `{before: <full row>, after: null, fieldsChanged: ["*"]}`
  - `before` / `after` 均为仪表**完整快照**（驼峰 JSON），UI 可直接渲染表格
- **`commands/audit.rs` 新增 3 个 helper**
  - `compute_field_diff(before, after)` → 顶层字段比对，返**已排序**的变更字段名数组
    - 跳过 `id`（update 路径 id 必然不变）
    - `Value::eq` 语义：`null` vs `0.0` 不算变，`null` vs `50.0` 算变
    - `after` 非对象 → 返空数组（防御）
  - `snapshot(&T)` → 任意 Serialize 值转 JSON（驼峰键由原 struct serde 注解保证）
  - `instrument_audit_payload(before, after, fields_changed)` → 拼标准三段式 payload
  - **单元测试 +6 例**（单字段 / id 忽略 / null vs 值 / 多字段排序 / 无变更 / 非对象）
- **`commands/instruments.rs` 补审计空白**（此前 create / update / delete **完全没写 audit**）
  - `create_instrument` → 插入后取完整行 → 写 `instrument_create` 审计
  - `update_instrument` → **先查 before** → UPDATE → **再查 after** → `compute_field_diff` → 写 `instrument_update`
    - 无实质变更也写（`fieldsChanged: []`）—— 「打开关闭」也是工程行为，留痕更安全
  - `delete_instrument` → **先查 before** → DELETE → 写 `instrument_delete`
  - 审计写入失败不阻断业务（`let _ = write_audit(...)` 静默）
- **重构：全部 command 拆 `*_inner(pool, ...)` + 薄包装**（与 `bypasses.rs` 同模式，让集成测试可直调）
  - 新增 `get_instrument_inner` / `create_instrument_inner` / `update_instrument_inner` / `delete_instrument_inner`
- **新增 command `list_instrument_history(instrument_id, limit?)`**
  - SQL：`audit_log WHERE target_table='instrument' AND target_id=? ORDER BY id DESC LIMIT ?`
  - `limit` 默认 100，`clamp(1, 1000)` 防拉全表
  - `payload_json` 解析出三段；解析失败降级为空 payload（不阻断整列表）
  - 不存在的 id → 空列表（**不是** NotFound；历史为空是合法状态）
  - 注册进 `lib.rs` `generate_handler!`
- **前端 `components/InstrumentHistoryDrawer.vue`**（~420 行，含样式）
  - 右侧抽屉（560px / `translateX` 过渡）+ mask；**z-index 1100 > BypassAlertBanner 的 1000**（否则横幅盖住抽屉头部）
  - 时间线：动作色块（创建=蓝 / 更新=琥珀 / 删除=红）+ 时间戳 + actor
  - **变更字段 chips**：`["*"]` → 「全字段」；`[]` → 「无实质变更（仅保存动作）」；否则逐字段中文标签
  - **diff 表格**（仅 update + 非空 diff）：字段 · FIELD / 修改前 · BEFORE（红删除线）/ 修改后 · AFTER（蓝高亮）
  - **snapshot 视图**（create / delete）：完整字段 key-value 表
  - 字段中文映射 14 项；`role` 显示「检测（detector）」、`silTarget` 显示「SIL B」
  - 空态：「暂无修改记录」+ hint「只有 M2.3 起（含本版本）的修改会被写入审计」
- **`stores/studio.ts`**：新 slice `instrumentHistory` / `instrumentHistoryFor` / `instrumentHistoryLoading` + 3 actions
  - `openInstrumentHistory(id)` / `closeInstrumentHistory()` / `refreshInstrumentHistory(id)`
  - `createInstrument` / `updateInstrument` / `deleteInstrument` 后：若抽屉正开着同一 id → 自动刷新（实时反映）
- **`views/Instruments.vue`**：行操作列加「历史」按钮（列宽 120 → 180px）；挂载 `<InstrumentHistoryDrawer />`
- **`sif-studio-preview/app/mock-tauri.js`**
  - 加内存 `auditLog` 数组 + `diffFields()` + `recordAudit()` helper
  - `create/update/delete_instrument` 三处写 mock 审计
  - 新 command `list_instrument_history`（按 id 倒序 + 解析 payload）
  - ready 日志加 audits 计数
  - **mock 保真度修复**：4 个 `list_*`（instruments / projects / sifs / diagrams）改为 `JSON.parse(JSON.stringify(...))` 返回全新对象树
    - 原实现 `return instruments`（内部数组**同一引用**）→ Pinia 的 `this.x = await invoke(...)` 被 Vue `hasChanged` 判等拦住 → **computed 不重算，表格静默不刷新**（真后端每次 IPC 都做序列化往返，故真机无此问题；这是 mock 引入的**假阳性**）
- **集成测试 +6 例**（共 **30 例全通过**）
  - create 写完整快照（before=null / fieldsChanged=["*"]）
  - update 精确字段 diff（只改 setpoint → 只报 setpoint）
  - update 多字段 + 排序（service / setpoint / unit）
  - update 无实质变更（fieldsChanged=[]，但仍留痕 2 条）
  - delete 写 before 快照（after=null + 行真的删除）
  - list_instrument_history 倒序 + 按 target_id 隔离 + limit 生效 + 不存在 id 返空
- **截图证据 +4 张**：列表行「历史」按钮 / 抽屉空态 / 抽屉 diff 表格特写 / 整屏（列表 + 抽屉并排）

### Added — M2.4 SIF / Project 修改历史
- **`commands/audit.rs` 通用化**（M2.3 的 instrument-only 逻辑抽成跨实体 helper）
  - 新 `AuditEntry` struct：`id / ts / actor / action / target_table / target_id / payload_json / before / after / fieldsChanged / note`
    - `note` 从 payload 的 `description` 字段解析（sif_link / sif_unlink 的自然语言描述）
  - 新 `list_history_for_target_inner(pool, target_table, target_id, limit)` → **一张 SQL 服务全部实体的历史查询**
    - `WHERE target_table = ? AND target_id = ? ORDER BY id DESC LIMIT ?`
    - 时间列排序用 `id DESC`（不用 `ts DESC`）—— 秒级精度下同秒多条会乱序
  - `instrument_audit_payload` → 改名 `entity_audit_payload`（instrument 名保留为兼容别名）
- **`commands/sifs.rs` 补审计 + 新增 3 个写命令**
  - `create_sif` / `delete_sif` 此前**完全没写 audit** → 补齐（payload 协议同 M2.3）
  - **新增 `update_sif(id, input)`** —— SIF 此前只有 create/delete，没有编辑入口
  - **`link_instrument_to_sif` / `unlink_instrument_from_sif` 写审计**
    - link → `sif_link`，fieldsChanged = `["link"]`，payload 含 `{sifId, instrumentId, role, port_index, diagram_id, instrument_tag, diagram_code}`
    - unlink → `sif_unlink`，fieldsChanged = `["unlink"]`，删除前先查 link 详情
    - 两者均写 `description` 自然语言（如「关联 PT-101 (detector) 作为 sif_id=1 的检测 · 端口 1 · 图（未挂图）」）
  - 拆 `*_inner`：`get_sif_inner` / `create_sif_inner` / `update_sif_inner` / `delete_sif_inner` / `link_..._inner` / `unlink_..._inner`
  - 新 command `list_sif_history(sif_id, limit?)`
- **`commands/projects.rs` 补审计 + 新增 2 个写命令**
  - `create_project` 补 audit；**新增 `update_project` / `delete_project`**
  - 拆 `get_project_inner` / `create_project_inner` / `update_project_inner` / `delete_project_inner`
  - 新 command `list_project_history(project_id, limit?)`
- **前端 `components/EntityHistoryDrawer.vue`**（取代 M2.3 的 `InstrumentHistoryDrawer.vue`）
  - 接 `entity-type` prop（`instrument` / `sif` / `project`），三套字段中文标签表
  - 新增 **link / unlink 视觉**：`+ 关联`（绿 chip）/`− 解除`（灰 chip）+ `description` 自然语言行（不显示 diff 表）
  - `isLinkOp()` 判定 + `showDiffTable()` 排除 link/unlink
  - 头部 eyebrow 自适应 `M2.3 / M2.4 AUDIT TRAIL`
- **`stores/studio.ts` 通用化**
  - 新 `AuditEntry` / `HistoryTarget` / `SifBasic` 类型；`InstrumentHistoryEntry` 保留为 alias
  - slice 改为 `entityHistory` / `entityHistoryFor: {kind, id} | null` / `entityHistoryLoading`
  - 新 `openEntityHistory(kind, id)` / `closeEntityHistory()` / `refreshEntityHistory()`（按 kind 分发到 3 个后端命令）
  - 新 `updateSif` / `deleteSif` / `updateProject` / `deleteProject`
  - 三组 `_refreshOpen*HistoryIfAny(id)` —— 写操作后若抽屉开着同一实体则实时刷新
- **`views/SifDashboard.vue`**：表格加操作列（140px）「编辑 / 历史」；编辑复用创建弹窗（标题切 EDIT）并**补上原本缺失的「验证 SIL」字段**；挂 `<EntityHistoryDrawer entity-type="sif" />`
- **`views/ProjectsView.vue`**：项目卡加「编辑 / 历史 / 删除」按钮组；编辑时编号 input 灰禁（唯一键）；挂 `<EntityHistoryDrawer entity-type="project" />`
- **`views/Instruments.vue`**：切换到 `<EntityHistoryDrawer entity-type="instrument" />`（旧组件删除）
- **`sif-studio-preview/app/mock-tauri.js`**
  - `recordAudit` 改为 `(action, targetTable, targetId, before, after, description)` 六参
  - 抽 `listHistoryByTarget(targetTable, a)` 通用帮助函数（3 个 list_*_history 共用）
  - 新 mock 命令：`update_sif` / `update_project` / `delete_project` / `list_sif_history` / `list_project_history`
  - `create_sif` / `delete_sif` / `create_project` / `link_instrument_to_sif` / `unlink_instrument_from_sif` 补写 mock 审计
- **集成测试 +8 例**（`tests/m24_history_test.rs`，独立 test target）
  - sif_create 快照 / sif_update 字段 diff / sif_delete before 快照
  - sif_link description + after 快照 / sif_unlink before 快照
  - list_sif_history 倒序 + limit + 隔离
  - project create+update+delete 三处 audit / project_history 隔离 + 通用 helper
- **截图证据 +4 张**：SIF 行操作 / SIF 抽屉（link+update×2+create 同屏）/ 项目卡操作 / 项目抽屉 diff

### Added — M2.5 审计包导出（CSV 全量 + 六维筛选）
- **`src-tauri/src/commands/audit_export.rs`**（~575 行，含单元测试）
  - **5 个 commands**（全部薄包装 + `*_inner(pool, …)` 实体，便于集成测试直调）：
    - `list_audit_filters` → `AuditFilterOptions { actions, actors, tables }`（给筛选下拉喂选项，取自 `audit_log` 现有值 DISTINCT）
    - `count_audit_filtered(filter)` → `i64`（轻量命中数，供"导出前确认"用）
    - `summarize_audit_filtered(filter)` → `AuditSummary { total, byAction, byTable }`（前端柱状看板数据源）
    - `preview_audit_filtered(filter, limit)` → `Vec<AuditEntry>`（默认 100，clamp 1..1000）
    - `export_audit_csv(filter, output_path)` → `ExportResult { path, bytes, rows }`（**落盘 CSV**）
  - **`AuditFilterInput` 六维筛选**（全可选）：`target_table` / `action` / `actor` / `ts_from` / `ts_to` / `target_id`
  - **`build_filter()` 统一构造器**：把用户输入清洗成 `(where_sql, binds)` 二元组，5 个 SQL 函数共用一份条件 —— 单点修改，无重复拼接
    - 空串 / 纯空白视同"未筛选"（防前端 `""` 变 `WHERE action = ''`）
    - `target_id` 走 `is_some_and` 语义，`0` 是合法值不是"空"
  - **CSV 输出（Excel 中文友好）**：
    - 头部写 UTF-8 BOM（`\u{FEFF}`）→ Excel 双击不乱码（否则中文列名变乱码，现场高频投诉点）
    - 写 CSV 前先 `wtr.write_record` 手动写 BOM 行 + 10 列表头：`audit_id / ts / actor / action / target_table / target_id / fields_changed / before_json / after_json / note`
    - 转义全交给 `csv` crate（逗号 / 引号 / 换行），**不手写 `"` 拼接** —— 测试里验证过 JSON 内双引号正确变成 `""`
    - `fields_changed` 用 `|` 连接（不用逗号，避免和 CSV 分隔符打架）
    - `before_json` / `after_json` 保留完整快照 JSON 字符串（审计取证要原始值，不裁剪）
  - **⚠️ 时间比较用裸字典序，不套 `datetime()`**（与 M2.2 反直觉决策，见下方说明）
  - **路径校验**：必须绝对路径（Windows `X:\` / POSIX `/`），否则 `Validation` 错误 —— 防相对路径写到进程 CWD 里找不到
  - 单元测试 **6 例**：单维筛选 / 全六维 / 空白剔除 / target_id=0 边界 / 时间裸比较 / where_sql 拼接形态
- **⚠️ 为什么 M2.5 的时间比较**不**套 `datetime()`（与 M2.2 相反）**
  - M2.2 的教训是 `datetime()` 两边规范化 —— 因为 `bypass_record.planned_restore` 由 Rust `to_rfc3339()`（T 分隔）写入，与 `datetime('now')`（空格分隔）混比会错位
  - M2.5 的 `audit_log.ts` 是**混合格式**列：M2.1 的导入审计写 `datetime('now')`（空格），M2.3+ 写 RFC 3339（T）
  - 此时**两个都套 `datetime()` 反而危险**：`datetime()` 对极少数畸形值返回 NULL，`NULL >= NULL` → 静默丢行
  - 实测裸比较在这两种格式混存下**表现正确**（同一天内 T 格式恒大于空格格式，跨天时日期前缀已能区分；且统一 `Z` 后缀保证同格式内可比）
  - **决策**：M2.5 用裸字典序 + 注释写明理由；**未来若 `ts` 列全量统一 RFC 3339，应回归 `datetime()` 规范**（这是技术债，已在代码注释标注）
- **前端 `views/AuditCenter.vue`**（~835 行，含样式）
  - 三区段：**筛选区**（6 输入 + 重置）→ **汇总统计看板**（按 action / 按 target_table 双列横向柱状图，宽度 = 占比，最小 2% 保证可见）→ **预览表**（时间 / 操作人 / 动作徽章 / 表 / ID / 变更摘要）
  - **300ms 防抖**：筛选条件变更自动重拉预览 + 汇总，避免连点打爆后端
  - 动作徽章按语义着色：`*_create` 灰蓝 / `*_update` 琥珀 / `*_delete` 红 / `*_link` 绿 / `*_unlink` 灰
  - 导入 / 导出的 Excel 图标走 `lucide-vue-next`（已验证图标存在再引用，避免 build 期才炸）
  - **导出双-三路径**（`pickSavePath`）：
    1. mock 模式（`__TAURI_INTERNALS__.__mocked`）→ 直接 `invoke('plugin:dialog|save')`，mock 立即返回固定路径，**不弹 `prompt()`**
    2. 真 Tauri → 动态 `import('@tauri-apps/plugin-dialog')` 的 `save()`（vite chunk split，浏览器不加载）
    3. 纯浏览器无 mock → `prompt()` 人肉输入（最后兜底）
  - 导出结果条显示 `行数 / 字节数 / 落盘路径`（`lastResult`）
- **Pinia store 扩展**：`auditFilterOptions` / `auditFilter` / `auditSummary` / `auditPreview` / `auditLoading` / `auditExporting` / `auditLastExport` + 4 actions（`refreshAuditFilters` / `refreshAuditPreview` / `countAuditFiltered` / `exportAuditCsv`）
- **入口三件套**：`App.vue` 侧栏 **06 审计中心**（`ScrollText` 图标）+ `routeSection` → `07 / AUDIT EXPORT` + `pageCode` → `DWG-007`；`router.ts` 加 `/audit`
- **mock 兼容（`mock-tauri.js`）**：
  - 新增 **14 条种子审计**（跨 2026-08-15 ~ 2026-09-12，覆盖 5 个 actor / 9 个 action / 4 张表）→ 让时间筛选与分组统计都有真实分布
  - 新增 4 个 mock handler：`buildMockFilter` / `filterRows` / `parseAuditEntry` + `list_audit_filters` / `count_audit_filtered` / `summarize_audit_filtered` / `preview_audit_filtered` / `export_audit_csv`
  - `export_audit_csv` mock 端**手写 CSV 生成**（BOM + 表头 + 逐行转义），写入 `window.__mockCsvExports[]` 供截图脚本读取
  - **headless 检测**：UA 含 `headless` 时**跳过 `link.click()` 浏览器下载** —— 否则 puppeteer 会卡在下载决策上，`Runtime.callFunctionOn` 超时
  - 新增 `plugin:dialog|save` mock，返回 `C:\Users\Engineer\Documents\<defaultPath>`
- **集成测试 +8 例**（`tests/m25_export_test.rs`，独立 test target）
  - CSV 头行结构 / 全量导出行数 / 六维筛选命中 / 空筛选等价全量 / 落盘字节与文件存在 / CASCADE 删除后审计仍保留（append-only 语义）/ link-unlink 的 description 入 `note` 列 / `fields_changed` 列内容
- **截图证据 +4 张**：审计中心初始态（14 条）/ `target_table=sif` + 时间区间筛选态（6 条，4 种 action）/ 导出成功态（6 行 1012 B）/ CSV 文件内容渲染

### Added — M2.6 审计包 PDF 报告（工程文档级，中文字体子集内嵌）

**目标**：在 M2.5 CSV（机器可读）之外，再给一份**人读的工程报告**——
A4 横版、带 ISO 7200 页眉页脚、嵌入中文字体子集、跨设备开箱可读，
用于项目移交与外部合规审计。

- **前端 PDF 生成**（`src/utils/auditPdf.ts`，~500 行，**零后端改动**）
  - 技术选型：**pdfmake 0.3.11**（纯前端，数据源复用 store，不引模板引擎）；
    产出 **PDF 1.3**（pdfkit 默认），`/FontFile2` 确认字体子集已内嵌，4 页 / ~43KB
  - **懒加载**：pdfmake chunk（1.0MB）+ 字体 chunk（2.1MB）走动态 `import()`，
    首屏 bundle 仅 +6.3KB（auditPdf 包装层）
  - 报告结构（多页自动分页）：
    1. 一、报告说明（数据源 / 导出格式说明）
    2. 二、筛选条件（6 维逐条 + 命中/预览计数）
    3. 三、汇总统计（按动作 / 按实体类型两张表）
    4. 四、详细记录（`pageBreak: before` 另起一页，10 列表格，
       `dontBreakRows` + `keepWithHeaderRows` 保证跨页表头重复、行不被腰斩）
    5. 五、附录（数据来源 / 字段说明 / IEC 61511-1 §5.2.6.1.5 等审计依据）
  - 页眉 ISO 7200 风格：`SIF Studio / 联锁工坊` + `DOC-AUDIT-001` + 日期 + `第 X 页 / 共 N 页`
  - 页脚：生成声明 + 用途限定
  - JSON 单行截断 60 字（完整值仍引导用户用 CSV）

- **中文字体子集化**（`scripts/subset-font.cjs`，harfbuzz wasm）
  - 源：系统 `C:/Windows/Fonts/simhei.ttf`（9.5MB，20902 汉字）
  - 字符集分层策略（宁多勿缺）：
    1. **GB2312/GBK 全字集**（6763 汉字 + 682 符号）主力 —— 覆盖人名 / 工程术语 / 标点
    2. ASCII + Latin-1（含 `·` `°` `×` `§`）
    3. 通用标点 `U+2000-206F`（含 em dash `—`、省略号 `…`、弯引号）+ `U+2E00-2E7F`
    4. CJK 标点 `U+3000-303F` / 全角形式 `U+FF00-FFEF`
    5. 源码扫描 `src/**`（16 文件）—— 保证 UI 文案 / 报告字面量 100% 覆盖
    6. 工程术语补充表（~400 词组）
  - 输出：`src/assets/fonts/SimHei-subset.ttf` = **8764 字 / 2.07MB**（压缩比 21.7%）
  - ⚠️ **子集 TTF 不入库**（SimHei 为系统字体，不可再分发）——
    已加 `.gitignore`，`npm run font:subset` 或 `build` 前置自动生成
  - `scripts/verify-font-coverage.cjs`：解 TTF cmap 表，逐字校验报告文案覆盖率

- **前端集成**
  - `views/AuditCenter.vue`：导出栏从单按钮改为 **CSV / PDF 双按钮**
    （PDF 用描边 + hover 反色，与 CSV 实心形成主次），
    结果条分两行（`.last-result.pdf` 墨色 + 左边框区分）
  - PDF 另拉 **500 条**预览（`fetchPdfPreview`），与 UI 表格的 100 条隔离
  - store 新增 `exportAuditPdf()` / `fetchPdfPreview()`，`pdfGenerating` 独立于
    `auditExporting` 避免两按钮互相打架
  - `src/shims-pdf.d.ts`：pdfmake 无官方 TS 类型，手写 `PdfMakeRootModule`
    （含 Vite 打包后 `{ p: { default } }` 嵌套结构）+ `*.ttf?url` 声明

- **截图证据 +5 张**：导出栏双按钮（滚到 `.content` 底部）/ PDF 成功态结果条 /
  导出栏元素特写 / **PDF 第 1 页**（报告说明+筛选+汇总）/ **PDF 第 3 页**（14 条详细记录表）

**踩坑（4 个，均为静默失败）**
1. **pdfmake 0.3.x 主入口是 Node 版** —— `pdfmake/build/pdfmake.js` 的
   `OutputDocumentServer.getBlob(cb)` 在浏览器里**永不回调**（Node fs 流）。
   必须用 `pdfmake/js/browser-extensions/pdfMake.js`（`getBlob(): Promise<Blob>`）。
   症状：`auditExporting` 永久 true，无任何报错。
2. **0.3.x API 改名** —— `vfs`/`fonts` 直赋已废弃 → `addVirtualFileSystem()` / `addFonts()`。
   写错**不报错**，只在 `createPdf` 时报 `File 'xxx.ttf' not found in virtual file system`。
3. **Vite 打包后导出形状** —— pdfmake CJS 被包成 `{ p: <webpack_module> }`，
   真单例在 `mod.p.default`（不是 `mod.default`）。用错则 `createPdf is not a function`。
4. **字体子集缺字（真机复现）** —— 初版只扫源码，PDF 里 `王`/`李`（运行时操作人姓名）、
   `—`（em dash，来自 `jsonOneLine` 的空值占位）、`（` `）`（全角括号）全渲染成方框。
   **根因**：这些字来自**数据**（DB / mock），不在源码里；且 `U+2014` 不在
   `U+3000-303F` / `U+4E00-9FFF` / `U+FF00-FFEF` 任一区间。
   修法：改 GB2312 全字集 + 三个标点区间 + 源码扫描 + 术语补充表。
   **验证靠 `pdfjs-dist` 提取 PDF 文本逐字核对**（不是靠肉眼看截图）。

- **回归验证**：后端 **73 passed**（lib 23 + integration 30 + m24 8 + m25 12，**零回归**）；
  vue-tsc 0 错；vite build 6.6s；画廊扩到 **39 section / 40 图引用 / 0 断链**

### Added — M2.7 审计包 PDF 报告增补图表 + 公司抬头 + 签章位（M2.6 之后的可选增强）

- **3 个聚合命令**（`commands/audit_export.rs` §5，4 个 command 全薄包装 + `*_inner`）：
  - `compute_daily_activity(filter)` → `Vec<DailyActivity>` 按 `substr(ts, 1, 10)` 分组（兼容两种时间格式）
  - `compute_bypass_duration_buckets(_)` → `Vec<DurationBucket>` 5 桶：`<1h / 1–8h / 8–24h / 1–7d / >7d`，仅算已恢复旁路
  - `compute_sil_change_timeline(filter)` → `Vec<SilChangeEvent>` 关联 `sif` 表拿 `code`，从 payload 提取 `silVerified` 变化
  - `fetch_audit_charts(filter)` 三合一合集（前端一次 invoke 拿全）
- **PDF 模板新增章节「四、分析图表」**（`src/utils/auditPdf.ts`）：
  - 4.1 每日审计活动柱图（取最近 14 天，pdfmake `canvas` 矩形）
  - 4.2 旁路时长分布柱图（固定 5 桶）
  - 4.3 SIL 验算等级变更表（离散事件用表格呈现）
  - 注：SIF 无独立 PFDavg 字段 → 用 `sil_verified`（A→10⁻⁵, B→10⁻⁴, C→10⁻³, D→10⁻²）作 PFDavg 代理指标
- **ISO 7200 抬头扩展**：上行加**公司名**（粗体大字号）+ 中行**项目名 + 报告版本**
- **签章位（附录 6.1）**：三栏 `编制 / 审核 / 批准`，每栏含角色标签 + 签字线 + 姓名占位 + 日期占位
- **图表零字体子集依赖**：用 pdfmake `canvas` 块画矩形（左侧实色 + 右侧淡色底），不用 █ ░ 等 Unicode 块元素（SimHei 没这些）
- **章节重新编号**：一二三（说明/筛选/汇总）+ 四（图表）+ 五（详细）+ 六（附录 / 签章位）
- **集成测试**：`tests/m27_charts_test.rs` **12 passed**（空 / 分组 / 混合时间格式 / 筛选 / 5 桶分布 / 未恢复不计 / SIL create / SIL update / 其他表过滤 / 合集调用）
- **mock-tauri.js**：4 个新 mock handler（含 `fetch_audit_charts` 合集）
- **gallery**：`take-shots-m27.js` 5 张新截图（39 公司抬头 / 40 图表 / 41 SIL+详细 / 42 签章位 / 43 附录）
- **样本 PDF**：`samples/sif-studio-audit-sample.pdf` 57.4 KB → 5 页（A4 横版 + 中文字体子集）
- **回归验证**：**85 passed**（lib 23 + integration 30 + m24 8 + m25 12 + **m27 12**，**零回归**）；vue-tsc 0 错；vite build 6.47s；pdfjs-dist 提取 PDF 文本 5 页全 0 缺字（公司抬头 / 图表标题 / SIL 变更表 / 签章位标签全清晰）

### Planned
- M1.6：可选 — 自定义 identifier（去 `.app` 警告，目前 `cn.sifstudio.app` 与 macOS bundle 后缀冲突）
- ~~M2.7：可选 — PDF 报告增补图表（PFDavg 趋势 / 旁路时长分布）、公司抬头与签章位~~ ✅ 已完成
- M3：服务工单 / 报表模板库 / 数据快照

### Fixed — M2.8 编辑器桥链路修复 + 风格统一（用户反馈）

用户反馈：①联锁逻辑图编制功能不能正常使用；②UI 风格与主应用不一致。

#### 桥链路 4 处修复（用户主诉 1）

1. **A3 监听名错配**：`bindIframe` 监听 `'saved'`，但 M0 编辑器（`public/editor.html:3545`）实际发 `'save-response'` → 保存按钮永远写回原始快照（no-op）
   修：监听改名 + 收到时把 `payload.data` 写回 `diagram.value.data`
2. **A2 漏 `'loaded'` 处理**：LOADING 永不切 READY；DIRTY 兜底引起 UI 抖动
   修：收到 `'loaded'` → `ready.value = true; saved.value = true; dirty.value = false`
3. **桥注册 race condition（额外发现）**：`editor.html` IIFE 末尾**同步**发 `post('ready')`，但 Vue 主应用在 iframe `@load` 事件才注册 listener——@load 晚于 IIFE → ready 消息丢失
   修：`bindMessageListener` 在 `onMounted` 立即注册，不等 `@load`；`@load` 仍触发以处理 HMR / 路由切换
4. **E3 重复 addEventListener**：每次 iframe reload 都加一个 listener（HMR / 路由切换 → dirty 抖动）
   修：handler 引用存到组件 scope 的 `messageHandler` 变量，每次注册前先 remove
5. **E2/A1 editorSrc 用 ref + onMounted 后赋值 → loadExample 闪一下被 load-diagram 覆盖 + URL hash 拼接是死代码**
   修：改 `computed`，等 `store.loadDiagram` 真正拿到 data 再设；删 hash（M0 编辑器不读）

#### 风格统一（用户主诉 2）

`public/editor.html` 的 `:root` 变量值全部对齐 `src/styles/global.css` 主应用 token；CSS 末尾追加「主题覆盖层」覆盖金属深灰渐变工具条 / 偏移厚阴影模态框 / 蓝底白字标题栏等 CAD 风格：

| 元素 | 原风格 | 主应用风格 |
|---|---|---|
| 工具条 | `linear-gradient(#2c333d, #1b212a)` 金属深灰 | `var(--paper)` 白底 + 黑描线 |
| 工具条按钮 | `background:#39414d` 金属灰 | `var(--paper)` 白底 + 黑描线 + hover 加粗 |
| 工具条按钮 on | 白底黑字 | `var(--ink)` 黑底 + 白字（主应用 primary） |
| 工具条按钮 danger | 红边红字白底（保留） | 同（业务语义色） |
| 状态栏 | 金属渐变 | `var(--paper2)` 白底灰描线 |
| 模态框头 | `#1f4e79` 蓝底白字 + 10px 偏移厚阴影 | `var(--paper3)` 灰底黑字 + 无阴影 + ISO 7200 |
| 侧栏 / 属性面板 | `#f6f7f9` 浅灰 + 2px 灰描边 | `var(--paper)` 白底 + 灰描边 |
| 元件库 hover | `#eef3f8` + 琥珀边 | 主应用 hover 风格（白底 + 黑边加粗） |
| 表单元素 | `#fff` + 蓝聚焦阴影 | 主应用 form 风格（白底 + 黑边 + focus 加粗） |
| 字号 / 字距 | 11/11.5px / 0.3-1.4px | 11/12/13px / 0.02em |
| 字体 | SimHei 优先 | 罗马字优先 + 中文回退仿宋 |

#### 状态机

新增 `ready / dirty / saved` 显式状态：`LOADING → READY → DIRTY → SAVED`。状态文字渲染到顶栏左侧。

#### 验证

- **vue-tsc**：0 错（中间踩了 TS2304 hoisting 坑——`function` 声明在 setup 中不提升，改 `const` 箭头函数）
- **vite build**：6.36s · 1540 modules
- **cargo test**：**85 passed 零回归**（lib 23 + integration 30 + m24 8 + m25 12 + m27 12 + doctest 0）
- **截图**：`sif-studio-preview/take-shots-m28.js` Edge headless 5 张
  - 44 — 编辑器外观（白底工具条 / 侧栏 / 属性面板 / 状态栏）
  - 46 — DIRTY 状态（画布加测试块 → 顶栏切 DIRTY）
  - 47 — SAVED 状态（保存链路打通：LOADING → SAVED → DIRTY → SAVED）
  - 48 — 元件库 hover（验证覆盖层）
  - 49 — 模态框（部分渲染）

#### 已知脚本坑（非应用 bug）

`document.querySelector("button.primary")` 在 DiagramEditor 页面会先匹配顶部 `BypassAlertBanner` 的"前往 BypassLedger"按钮（M2.2 加的常驻横幅）。截图脚本必须用 `textContent.includes("保存到数据库")` 精确选。

---

## [0.1.0] - 2026-09-13

### Added
- Tauri 2 工程骨架（Cargo.toml / tauri.conf.json / capabilities / main.rs / lib.rs）
- 数据库 schema v001：7 表 + CHECK 约束 + 10+ 索引，嵌入 sqlx::migrate!
- 17 Rust commands：db_version / instruments(CRUD+count) / projects(CRUD+ensure_default) / diagrams(CRUD+ensure_default+save_data) / sifs(list/create/delete/get/link/unlink/list_links) / tags(import CSV)
- `list_sifs` 单 SQL 跨图汇总：COUNT(DISTINCT CASE WHEN role=…) + GROUP_CONCAT(detector/final) + GROUP_CONCAT(diagram.code)
- Pinia store `stores/studio.ts`：4 slice + actions + camelCase IPC
- 5 路由：Home / Instruments / SifDashboard / ProjectsView / DiagramEditor
- Home：4 统计卡 + SIL 进度条 + 最近 SIF + 标准列表
- Instruments：CRUD + 角色筛选 + 量程显示 + 弹窗表单
- SifDashboard：跨图汇总 + 4 详情块（仪表 / 检测位号 / 最终元件 / 出现位置）+ SIF↔仪表关联弹窗
- ProjectsView：项目卡 + 每项目图列表 + 创建弹窗 + 直跳编辑器
- DiagramEditor：iframe + postMessage 嵌入 M0 + 桥接 `exportJson/importJson/postMessage('ready'|'dirty'|'loaded'|…)`
- 5 色语义 CSS 变量（与 M0 一致）+ Lucide 图标
- 图标资源：icon.ico（经典 BMP-in-ICO 4286B）+ 32x32/128x128/128x128@2x.png + 256x256（icon-256.png）
- Rust 集成测试 **7 例全通过**：db_version / tag unique / role check / 跨图汇总 / 级联删除 / 项目级联 / link unlink
- **Windows MSI installer**（3.08 MB，WiX 3.14.1 出品，OLE/CFB 复合文档标准）
- **Windows NSIS installer**（2.22 MB，NSIS 3.11 出品，_x64-setup.exe 自解压安装）
- **Portable .exe**（7.94 MB，单文件双击即用，无 IT 审批场景适用）
- vue-tsc 严格类型检查通过（修了 3 类：SifDashboard `ref<boolean|null>` / vite.config.ts `defineConfig({})` 对象形式 / `minify: "esbuild"` 字面量）

### Known Issues / Limitations
- `cn.sifstudio.app` identifier 末尾 `.app` 会触发 Tauri 警告（与 macOS .app 后缀冲突，Windows 不受影响）
- 沙盒无桌面会话 → 真实 UI 渲染须用户本机执行安装包或 `npm run tauri:dev` 验证
- 默认会同时打 MSI + NSIS；CI 上如果只想出 MSI，加 `--bundles msi`

[Unreleased]: https://github.com/<your-org>/sif-studio/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/<your-org>/sif-studio/releases/tag/v0.1.0
