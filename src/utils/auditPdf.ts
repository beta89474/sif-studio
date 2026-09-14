/**
 * M2.6 — 审计包 PDF 报告生成（前端 pdfmake）
 *
 * 设计要点：
 *   - **零后端依赖**：所有数据由 store 喂入，pdfmake 在前端组装 PDF → 触发浏览器下载。
 *     真 Tauri 模式下走 plugin-dialog.save() 拿绝对路径；mock / puppeteer 走 mock-tauri.js。
 *   - **中文字体子集化**：simhei.ttf（系统自带，9.5MB）→ scripts/subset-simhei.cjs
 *     提取 ~1000 工程常用字 + ASCII → SimHei-subset.ttf（~200KB）。注入到 pdfmake VFS，
 *     输出的 PDF 自动嵌入字体子集，跨设备可读。
 *   - **懒加载**：pdfmake + 字体走动态 import，避免首屏 bundle 暴涨（pdfmake core ~2.4MB）。
 *   - **多页结构**：封面元数据 → 筛选条件 → 汇总统计 → 详细记录 → 附录，
 *     每页带 ISO 7200 风格页眉 + 「第 X 页 / 共 N 页」页脚。
 *
 * 数据流：
 *   store.auditPreview   (preview_audit_filtered, limit=100)
 *   store.auditSummary   (summarize_audit_filtered)
 *   filter               (6 维 AuditFilter)
 *   ↓
 *   generateAuditPdf(filter, preview, summary)
 *   ↓
 *   Blob URL + 自动 download()
 */
import type {
  AuditEntry,
  AuditFilter,
  AuditSummary,
  AuditCharts,
  DailyActivity,
  DurationBucket,
  SilChangeEvent,
} from "../stores/studio";

// pdfmake 模块的运行时类型（pdfmake 0.3.x 没有官方 TS 定义，自己写）
// 关键：0.3.x 用 addVirtualFileSystem() / addFonts()（旧版 0.2.x 直接赋 vfs/fonts 已废弃）
// 浏览器版 `OutputDocumentBrowser` 提供 getBlob(): Promise<Blob>（主入口的是 Node fs 流）
interface PdfMakeApi {
  virtualfs: Record<string, string>;
  fonts: Record<string, Record<string, string>>;
  addVirtualFileSystem: (files: Record<string, string>) => void;
  addFonts: (fonts: Record<string, Record<string, string>>) => void;
  createPdf: (def: unknown) => {
    getBlob: () => Promise<Blob>;
    download: (name?: string) => Promise<void>;
    open: () => Promise<void>;
  };
}

// Vite 把 pdfmake 包成 { default?: ... } 或 { p: { default?: ... } }
// <mod>.default 才是真正的 pdfmake 单例
type PdfMakeModule = { default?: PdfMakeApi; p?: { default?: PdfMakeApi } } & PdfMakeApi;

// ----------------------------------------------------------------------------
// 1. 懒加载 pdfmake + 字体子集
// ----------------------------------------------------------------------------
let _pdfMakePromise: Promise<PdfMakeApi> | null = null;

async function loadPdfMake(): Promise<PdfMakeApi> {
  if (_pdfMakePromise) return _pdfMakePromise;
  _pdfMakePromise = (async () => {
    // 动态 import → Vite 自动 split 到独立 chunk，首屏不加载
    const pdfMod = (await import("pdfmake/build/pdfmake.js")) as unknown as PdfMakeModule;
    // Vite/webpack 打包后结构：{ p: <webpack_module> } → <webpack_module>.default 是单例
    const pdfMake =
      pdfMod.p?.default || pdfMod.default || pdfMod;

    // Vite 解析 ?url → 拿到打包后的字体 URL
    const fontUrl = (await import("../assets/fonts/SimHei-subset.ttf?url"))
      .default as unknown as string;

    // fetch → arrayBuffer → base64（pdfmake VirtualFS 要 base64）
    const buf = await fetch(fontUrl).then((r) => r.arrayBuffer());
    const b64 = arrayBufferToBase64(buf);

    // 0.3.x API：addVirtualFileSystem + setFonts（旧版 0.2.x 直接赋 vfs/fonts 已废弃）
    pdfMake.addVirtualFileSystem({ "SimHei-subset.ttf": b64 });
    pdfMake.addFonts({
      SimHei: {
        normal: "SimHei-subset.ttf",
        bold: "SimHei-subset.ttf",
        italics: "SimHei-subset.ttf",
        bolditalics: "SimHei-subset.ttf",
      },
    });
    return pdfMake as PdfMakeApi;
  })();
  return _pdfMakePromise;
}

function arrayBufferToBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  // Chunk 拆分避免 call stack 溢出（200KB TTF → ~270K chars）
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode.apply(
      null,
      Array.from(bytes.subarray(i, i + CHUNK)),
    );
  }
  // 浏览器环境有 btoa；Tauri WebView 也走 btoa；Node 测试环境需 Buffer
  if (typeof btoa !== "undefined") return btoa(binary);
  return Buffer.from(buf).toString("base64");
}

// ----------------------------------------------------------------------------
// 2. 工具函数
// ----------------------------------------------------------------------------
function fmtTs(ts: string): string {
  // RFC 3339 → 'YYYY-MM-DD HH:MM:SS'（本地时区）
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts;
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  );
}

function fmtNowLocal(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  );
}

function describeFilter(f: AuditFilter): string[] {
  const lines: string[] = [];
  lines.push(`  · 实体类型：${f.targetTable || "全部"}`);
  lines.push(`  · 动作：${f.action || "全部"}`);
  lines.push(`  · 操作人：${f.actor || "全部"}`);
  lines.push(`  · 起止时间：${f.tsFrom || "不限"} 至 ${f.tsTo || "不限"}`);
  lines.push(
    `  · 实体 ID：${f.targetId != null ? `#${f.targetId}` : "全部"}`,
  );
  return lines;
}

// 把 AuditEntry.before/after 序列化成单行 JSON
function jsonOneLine(v: unknown): string {
  if (v == null) return "—";
  try {
    const s = JSON.stringify(v);
    // PDF 表格单格太长会断行 → 截断到 60 字
    return s.length > 60 ? s.slice(0, 57) + "..." : s;
  } catch {
    return String(v);
  }
}

function fieldsText(fc: string[]): string {
  if (fc.length === 0) return "—";
  if (fc.length === 1 && fc[0] === "*") return "全字段";
  return fc.join(" · ");
}

// ----------------------------------------------------------------------------
// 2b. ASCII 条形图 / 简单表格 —— 用 pdfmake canvas 块绘制矩形，规避字体子集缺字
// ----------------------------------------------------------------------------

/** 行高（pt）—— canvas rect 高度。字号 9pt → rect 高度 10~12pt 视觉舒适 */
const CHART_ROW_HEIGHT = 10;
/** 文字标签列宽（pt） */
const CHART_LABEL_WIDTH = 80;
/** 数值列宽（pt） */
const CHART_VALUE_WIDTH = 36;
/** 图表块总宽（pt，A4 landscape 减去边距 760） */
const CHART_TOTAL_WIDTH = 760 - CHART_LABEL_WIDTH - CHART_VALUE_WIDTH;

/** 渲染一行：标签 + 矩形条 + 数值 */
function makeBarRow(
  label: string,
  value: number,
  max: number,
): unknown {
  const ratio = max > 0 ? Math.min(value / max, 1) : 0;
  const filled = CHART_TOTAL_WIDTH * ratio;
  const empty = CHART_TOTAL_WIDTH - filled;
  return {
    columns: [
      {
        text: label,
        width: CHART_LABEL_WIDTH,
        style: "body",
        margin: [0, 1, 4, 0] as [number, number, number, number],
      },
      {
        // canvas 块绝对定位矩形：左侧填充实色、右侧淡色（显示"已分母=100%"的尺度感）
        canvas: [
          {
            type: "rect",
            x: 0,
            y: 0,
            w: filled,
            h: CHART_ROW_HEIGHT,
            color: "#1a1a1a",
            lineWidth: 0,
          },
          {
            type: "rect",
            x: filled,
            y: 0,
            w: empty,
            h: CHART_ROW_HEIGHT,
            color: "#f0f0f0",
            lineColor: "#aaa",
            lineWidth: 0.5,
          },
        ],
        width: CHART_TOTAL_WIDTH,
      },
      {
        text: String(value),
        width: CHART_VALUE_WIDTH,
        style: "body",
        alignment: "right",
        margin: [4, 1, 0, 0] as [number, number, number, number],
      },
    ],
    margin: [0, 0, 0, 2] as [number, number, number, number],
  };
}

/** "无数据" 占位 */
function emptyChartHint(text: string): unknown {
  return {
    text,
    style: "body",
    color: "#888",
    italics: true,
    margin: [0, 4, 0, 4] as [number, number, number, number],
  };
}

/** 每日活动柱图（取最近 14 天；缺失日期前端补 0） */
function buildDailyActivityChart(daily: DailyActivity[]): unknown[] {
  if (daily.length === 0) {
    return [emptyChartHint("（暂无审计活动数据）")];
  }
  // 取最近 14 天；不足全部显示
  const recent = daily.slice(-14);
  const max = Math.max(...recent.map((d) => d.count), 1);
  return [
    {
      text: `近 ${recent.length} 天 · 总 ${daily.reduce((s, d) => s + d.count, 0)} 次审计活动`,
      style: "chartCaption",
      margin: [0, 0, 0, 4] as [number, number, number, number],
    },
    ...recent.map((d) => makeBarRow(d.date, d.count, max)),
  ];
}

/** 旁路时长分布柱图（固定 5 桶） */
function buildBypassDurationChart(buckets: DurationBucket[]): unknown[] {
  if (buckets.every((b) => b.count === 0)) {
    return [emptyChartHint("（暂无已恢复的旁路记录）")];
  }
  const max = Math.max(...buckets.map((b) => b.count), 1);
  const total = buckets.reduce((s, b) => s + b.count, 0);
  return [
    {
      text: `共 ${total} 条已恢复旁路 · 按持续时长分桶`,
      style: "chartCaption",
      margin: [0, 0, 0, 4] as [number, number, number, number],
    },
    ...buckets.map((b) => makeBarRow(b.label, b.count, max)),
  ];
}

/** SIL 等级变更时间线（用表格而非柱图 —— 离散事件） */
function buildSilChangeTable(events: SilChangeEvent[]): unknown {
  if (events.length === 0) {
    return emptyChartHint("（暂无 SIF 创建或 SIL 等级变更记录）");
  }
  const rows = events.map((e) => {
    const fromText = e.fromSil ? e.fromSil : "（新）";
    return [
      { text: fmtTs(e.ts), style: "cellMono" },
      { text: e.sifCode, style: "cellMono" },
      { text: e.action === "sif_create" ? "新建" : "更新", style: "cellMono" },
      { text: fromText, style: "cellMono" },
      { text: "→", style: "cellMono", alignment: "center" },
      { text: e.toSil, style: "cellMono", bold: true },
    ];
  });
  return {
    table: {
      headerRows: 1,
      dontBreakRows: true,
      widths: [110, 80, 50, 50, 20, 50],
      body: [
        [
          { text: "时间 / TS", style: "th" },
          { text: "SIF", style: "th" },
          { text: "动作", style: "th" },
          { text: "原 SIL", style: "th" },
          { text: "", style: "th" },
          { text: "新 SIL", style: "th" },
        ],
        ...rows,
      ],
    },
    layout: "lightHorizontalLines",
    fontSize: 8,
  };
}

// ----------------------------------------------------------------------------
// 3. docDefinition —— PDF 内容组装
// ----------------------------------------------------------------------------
function buildDocDefinition(
  filter: AuditFilter,
  preview: AuditEntry[],
  summary: AuditSummary | null,
  totalRows: number,
  charts: AuditCharts | null,
  meta: {
    companyName: string;
    projectName: string;
    reportVersion: string;
    preparedBy: string;
    reviewedBy: string;
    approvedBy: string;
  },
): unknown {
  const nowStr = fmtNowLocal();
  const filterLines = describeFilter(filter);

  // 汇总表 — 按动作
  const summaryByAction = summary?.byAction ?? {};
  const summaryByTable = summary?.byTable ?? {};
  const actionEntries = Object.entries(summaryByAction).sort((a, b) => b[1] - a[1]);
  const tableEntries = Object.entries(summaryByTable).sort((a, b) => b[1] - a[1]);

  // 详细记录表（限于预览传入的数量；超出会显示「前 N 条 / 共 M 条」）
  const detailRows = preview.map((e) => [
    { text: String(e.id ?? ""), style: "cellMono" },
    { text: fmtTs(e.ts), style: "cellMono" },
    { text: e.actor || "—", style: "cellText" },
    { text: e.action, style: "cellMono" },
    { text: e.targetTable, style: "cellMono" },
    {
      text: e.targetId != null ? String(e.targetId) : "—",
      style: "cellMono",
    },
    { text: fieldsText(e.fieldsChanged ?? []), style: "cellText" },
    { text: jsonOneLine(e.before), style: "cellJson" },
    { text: jsonOneLine(e.after), style: "cellJson" },
    { text: e.note || "—", style: "cellText" },
  ]);

  return {
    pageSize: "A4",
    pageOrientation: "landscape", // 10 列横向才放得下
    pageMargins: [40, 90, 40, 60],

    // 全局字体 / 默认样式
    defaultStyle: { font: "SimHei", fontSize: 9, color: "#000" },

    // 页眉 —— ISO 7200 扩展：公司抬头 / 项目名 / 报告版本 / 日期 / 页码
    header: (currentPage: number, pageCount: number) => ({
      margin: [40, 24, 40, 16] as [number, number, number, number],
      columns: [
        {
          stack: [
            { text: meta.companyName, style: "headerCompany" },
            { text: "SIF Studio / 联锁工坊 · 审计包报告", style: "headerTitle" },
            {
              text: `项目：${meta.projectName} · 报告版本 ${meta.reportVersion}`,
              style: "headerSub",
            },
          ],
        },
        {
          stack: [
            { text: "DOC-AUDIT-001", style: "headerMeta", alignment: "right" },
            {
              text: `${nowStr}`,
              style: "headerMeta",
              alignment: "right",
            },
            {
              text: `第 ${currentPage} 页 / 共 ${pageCount} 页`,
              style: "headerMeta",
              alignment: "right",
            },
          ],
        },
      ],
    }),

    // 页脚 —— 单行说明
    footer: () => ({
      margin: [40, 16, 40, 16] as [number, number, number, number],
      text: "本报告由 SIF Studio 自动生成 · 仅供审计与项目移交使用",
      style: "footerText",
      alignment: "center",
    }),

    content: [
      // ===== 报告说明 =====
      {
        text: "一、报告说明",
        style: "h1",
        margin: [0, 0, 0, 8] as [number, number, number, number],
      },
      {
        text:
          "本报告汇总系统审计日志（audit_log）按当前筛选命中的全部记录，"
          + "包含元数据、筛选条件、汇总统计与详细记录。"
          + "源数据存储于本地 SQLite（%APPDATA%/sif-studio/studio.db），"
          + "导出格式为 PDF 1.3（pdfkit 默认；已内嵌中文字体子集，跨设备开箱可读）。",
        style: "body",
        margin: [0, 0, 0, 12] as [number, number, number, number],
      },

      // ===== 筛选条件 =====
      {
        text: "二、筛选条件",
        style: "h1",
        margin: [0, 8, 0, 8] as [number, number, number, number],
      },
      ...filterLines.map((line) => ({
        text: line,
        style: "body",
      })),
      {
        text: `命中记录：${totalRows} 条 / 预览显示：${preview.length} 条`,
        style: "body",
        bold: true,
        margin: [0, 8, 0, 16] as [number, number, number, number],
      },

      // ===== 汇总统计 =====
      {
        text: "三、汇总统计",
        style: "h1",
        margin: [0, 8, 0, 8] as [number, number, number, number],
      },
      {
        text: "按动作统计（By Action）",
        style: "h2",
        margin: [0, 0, 0, 4] as [number, number, number, number],
      },
        {
        table: {
          headerRows: 1,
          widths: ["*", "auto"],
          body: [
            [
              { text: "动作 / Action", style: "th" },
              { text: "计数 / Count", style: "th", alignment: "right" },
            ],
            ...actionEntries.map(([k, v]) => [
              { text: k, style: "td" },
              { text: String(v), style: "td", alignment: "right" },
            ]),
          ],
        },
        layout: "lightHorizontalLines",
        margin: [0, 0, 0, 16] as [number, number, number, number],
      },
      {
        text: "按实体类型统计（By Table）",
        style: "h2",
        margin: [0, 0, 0, 4] as [number, number, number, number],
      },
      {
        table: {
          headerRows: 1,
          widths: ["*", "auto"],
          body: [
            [
              { text: "实体类型 / Table", style: "th" },
              { text: "计数 / Count", style: "th", alignment: "right" },
            ],
            ...tableEntries.map(([k, v]) => [
              { text: k, style: "td" },
              { text: String(v), style: "td", alignment: "right" },
            ]),
          ],
        },
        layout: "lightHorizontalLines",
        margin: [0, 0, 0, 16] as [number, number, number, number],
      },

      // ===== 分析图表（M2.7） =====
      {
        text: "四、分析图表",
        style: "h1",
        margin: [0, 16, 0, 8] as [number, number, number, number],
      },
      {
        text:
          "本节从三个维度量化呈现审计活动：每日审计量反映操作节奏、"
          + "旁路时长分布反映 MOC 执行效率、SIL 等级变更反映安全指标演化。"
          + "（注：SIL 变更采用 sif.sil_verified 字段，与 PFDavg 量级强相关：A→10⁻⁵, B→10⁻⁴, C→10⁻³, D→10⁻²。）",
        style: "body",
        margin: [0, 0, 0, 12] as [number, number, number, number],
      },
      // ---- 4.1 每日审计活动 ----
      {
        text: "4.1 每日审计活动（Daily Activity）",
        style: "h2",
        margin: [0, 4, 0, 4] as [number, number, number, number],
      },
      ...buildDailyActivityChart(charts?.dailyActivity ?? []),

      // ---- 4.2 旁路时长分布 ----
      {
        text: "4.2 旁路时长分布（Bypass Duration）",
        style: "h2",
        margin: [0, 12, 0, 4] as [number, number, number, number],
      },
      ...buildBypassDurationChart(charts?.bypassDurationBuckets ?? []),

      // ---- 4.3 SIL 等级变更 ----
      {
        text: "4.3 SIL 验算等级变更（SIL Verification Change）",
        style: "h2",
        margin: [0, 12, 0, 4] as [number, number, number, number],
      },
      buildSilChangeTable(charts?.silChangeTimeline ?? []),

      // ===== 详细记录 =====
      {
        text: "五、详细记录",
        style: "h1",
        margin: [0, 16, 0, 8] as [number, number, number, number],
        pageBreak: "before", // 详细表另起一页，避免穿插混乱
      },
      {
        text:
          preview.length < totalRows
            ? `以下显示前 ${preview.length} 条（共 ${totalRows} 条命中）。` +
              `如需完整记录请改用 CSV 导出。`
            : `以下显示全部 ${totalRows} 条记录。`,
        style: "body",
        margin: [0, 0, 0, 8] as [number, number, number, number],
      },
      {
        table: {
          headerRows: 1,
          dontBreakRows: true,
          keepWithHeaderRows: 1,
          widths: [
            "auto", // 编号
            60, // 时间
            "auto", // 操作人
            "auto", // 动作
            "auto", // 表
            "auto", // target_id
            "*", // 字段变更
            80, // before
            80, // after
            "auto", // 备注
          ],
          body: [
            [
              { text: "#", style: "th" },
              { text: "时间 / TS", style: "th" },
              { text: "操作人 / ACTOR", style: "th" },
              { text: "动作 / ACTION", style: "th" },
              { text: "实体 / TABLE", style: "th" },
              { text: "ID", style: "th" },
              { text: "变更字段 / FIELDS", style: "th" },
              { text: "变更前 / BEFORE", style: "th" },
              { text: "变更后 / AFTER", style: "th" },
              { text: "备注 / NOTE", style: "th" },
            ],
            ...detailRows,
          ],
        },
        layout: "lightHorizontalLines",
        fontSize: 8,
      },

      // ===== 附录 =====
      {
        text: "六、附录",
        style: "h1",
        margin: [0, 16, 0, 8] as [number, number, number, number],
        pageBreak: "before",
      },
      {
        text: "签章位（Signature Block）",
        style: "h2",
        margin: [0, 0, 0, 4] as [number, number, number, number],
      },
      {
        text:
          "本报告经下列人员审核确认生效。签章可采用电子签名、扫描件或手写签字。",
        style: "body",
        margin: [0, 0, 0, 8] as [number, number, number, number],
      },
      // 3 个签章格 —— 用 table 实现简单三列边框（pdfmake vfs 已有 lightHorizontalLines）
      {
        table: {
          widths: ["*", "*", "*"],
          body: [
            [
              {
                stack: [
                  { text: "编制 / Prepared by", style: "sigRole" },
                  { text: "", margin: [0, 28, 0, 0] as [number, number, number, number] },
                  {
                    canvas: [
                      { type: "line", x1: 0, y1: 0, x2: 180, y2: 0, lineWidth: 0.5, lineColor: "#000" },
                    ],
                  },
                  { text: meta.preparedBy || "_________________", style: "sigName" },
                  { text: "姓名 / 签字", style: "sigLabel" },
                  { text: "日期 / Date：____________", style: "sigLabel" },
                ],
                margin: [4, 6, 4, 6] as [number, number, number, number],
              },
              {
                stack: [
                  { text: "审核 / Reviewed by", style: "sigRole" },
                  { text: "", margin: [0, 28, 0, 0] as [number, number, number, number] },
                  {
                    canvas: [
                      { type: "line", x1: 0, y1: 0, x2: 180, y2: 0, lineWidth: 0.5, lineColor: "#000" },
                    ],
                  },
                  { text: meta.reviewedBy || "_________________", style: "sigName" },
                  { text: "姓名 / 签字", style: "sigLabel" },
                  { text: "日期 / Date：____________", style: "sigLabel" },
                ],
                margin: [4, 6, 4, 6] as [number, number, number, number],
              },
              {
                stack: [
                  { text: "批准 / Approved by", style: "sigRole" },
                  { text: "", margin: [0, 28, 0, 0] as [number, number, number, number] },
                  {
                    canvas: [
                      { type: "line", x1: 0, y1: 0, x2: 180, y2: 0, lineWidth: 0.5, lineColor: "#000" },
                    ],
                  },
                  { text: meta.approvedBy || "_________________", style: "sigName" },
                  { text: "姓名 / 签字", style: "sigLabel" },
                  { text: "日期 / Date：____________", style: "sigLabel" },
                ],
                margin: [4, 6, 4, 6] as [number, number, number, number],
              },
            ],
          ],
        },
        layout: {
          hLineColor: () => "#000",
          vLineColor: () => "#000",
          hLineWidth: () => 0.5,
          vLineWidth: () => 0.5,
        },
        margin: [0, 4, 0, 16] as [number, number, number, number],
      },
      {
        text: "数据来源",
        style: "h2",
        margin: [0, 12, 0, 4] as [number, number, number, number],
      },
      {
        ul: [
          "audit_log 表（M2.1 起的所有变更都会写一行审计）",
          "instrument_create / update / delete · sif_create / update / delete / link / unlink · project_create / update / delete · bypass_create / update / restore / delete",
        ],
        style: "body",
      },
      {
        text: "字段说明",
        style: "h2",
        margin: [0, 8, 0, 4] as [number, number, number, number],
      },
      {
        ul: [
          "BEFORE / AFTER 列只展示值；完整 JSON 见 CSV 导出的 before_json / after_json 列",
          "变更字段列展示发生变化的字段名列表；「全字段」表示 create / delete",
          "「—」表示空值（NULL 或删除时）",
        ],
        style: "body",
      },
      {
        text: "审计依据",
        style: "h2",
        margin: [0, 8, 0, 4] as [number, number, number, number],
      },
      {
        ul: [
          "IEC 61511-1 §5.2.6.1.5 — 审计追踪（audit trail）",
          "ISA-84.00.01 §5.6 — 变更管理记录",
          "GB/T 21109.1-2007 §5.2.6 — 安全仪表系统生命周期",
        ],
        style: "body",
      },
    ],

    styles: {
      h1: { fontSize: 14, bold: true, margin: [0, 6, 0, 4] as [number, number, number, number] },
      h2: { fontSize: 11, bold: true, margin: [0, 4, 0, 2] as [number, number, number, number] },
      body: { fontSize: 9, margin: [0, 0, 0, 4] as [number, number, number, number] },
      th: { fontSize: 9, bold: true, fillColor: "#f0f0f0", color: "#000" },
      td: { fontSize: 9 },
      cellMono: { fontSize: 7.5 },
      cellText: { fontSize: 7.5 },
      cellJson: { fontSize: 7, color: "#333" },
      headerCompany: { fontSize: 12, bold: true, color: "#000" },
      headerTitle: { fontSize: 9, color: "#000" },
      headerSub: { fontSize: 8, color: "#555" },
      headerMeta: { fontSize: 7.5, color: "#555" },
      footerText: { fontSize: 7, color: "#888", italics: true },
      chartCaption: { fontSize: 8, color: "#666", italics: true },
      sigRole: { fontSize: 9, bold: true, alignment: "center" },
      sigName: { fontSize: 10, alignment: "center", margin: [0, 6, 0, 2] as [number, number, number, number] },
      sigLabel: { fontSize: 7.5, color: "#888", alignment: "center" },
    },
  };
}

// ----------------------------------------------------------------------------
// 4. 公开 API
// ----------------------------------------------------------------------------

export interface AuditPdfResult {
  /** pdf bytes（用于打印 / 调试） */
  bytes: number;
  /** 详细表记录数（与 preview.length 一致） */
  rows: number;
  /** pdfmake 创建的内部 doc 实例（调用方可能想 download） */
  doc: { getBlob: (cb: (b: Blob) => void) => void; download: (n?: string) => void };
}

/**
 * 生成审计包 PDF —— 返回可下载的 Blob URL
 *
 * 用法：
 *   const r = await generateAuditPdf({filter, preview, summary, totalRows, charts, defaultFilename, ...meta});
 *   r.doc.download('audit.pdf');     // 浏览器自动下载
 *   // 或：
 *   r.doc.getBlob(b => window.open(URL.createObjectURL(b)));
 */
export async function generateAuditPdf(args: {
  filter: AuditFilter;
  preview: AuditEntry[];
  summary: AuditSummary | null;
  totalRows: number;
  charts: AuditCharts | null;
  defaultFilename: string;
  companyName: string;
  projectName: string;
  reportVersion: string;
  preparedBy: string;
  reviewedBy: string;
  approvedBy: string;
}): Promise<AuditPdfResult> {
  const pdfMake = await loadPdfMake();
  const docDef = buildDocDefinition(
    args.filter,
    args.preview,
    args.summary,
    args.totalRows,
    args.charts,
    {
      companyName: args.companyName,
      projectName: args.projectName,
      reportVersion: args.reportVersion,
      preparedBy: args.preparedBy,
      reviewedBy: args.reviewedBy,
      approvedBy: args.approvedBy,
    },
  );
  const doc = pdfMake.createPdf(docDef);
  const blob = await doc.getBlob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = args.defaultFilename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  // puppeteer / headless 环境跳过 revoke（否则 blob 引用失效，截图脚本读不到）
  const skipRevoke =
    typeof window !== "undefined" &&
    (window as unknown as { __TAURI_INTERNALS__?: { __puppeteerEnv?: boolean } })
      .__TAURI_INTERNALS__?.__puppeteerEnv;
  setTimeout(() => URL.revokeObjectURL(url), skipRevoke ? 60000 : 1000);
  // M2.6 测试钩子：把最后一次的 blob 暴露到 window，供 puppeteer 截图脚本渲染 PDF 内容
  if (typeof window !== "undefined") {
    (window as unknown as { __lastPdfBlob?: Blob }).__lastPdfBlob = blob;
    (window as unknown as { __lastPdfFilename?: string }).__lastPdfFilename =
      args.defaultFilename;
  }
  return {
    bytes: blob.size,
    rows: args.preview.length,
    doc,
  };
}