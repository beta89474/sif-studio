/**
 * Pinia store — 整个前端唯一状态来源
 *
 * 设计原则：
 * - 4 slice：meta / instruments / projects / diagrams / sifs（外加 references 子模块）
 * - 一切调用都走 Tauri invoke()，错误统一捕获
 * - "ensure default" 系列方法首次启动自动补默认数据，避免空白屏
 */

import { defineStore } from "pinia";
import { invoke } from "../api/rpc";

// ============================================================================
// 类型（与 Rust 端 #[serde(rename_all="camelCase")] 一一对应）
// ============================================================================

export interface AppMeta {
  dbVersion: number;
  dbPath: string;
  appVersion: string;
  startedAt: string;
}

export interface Instrument {
  id: number;
  tag: string;
  service: string;
  kind: string;
  role: "detector" | "final" | "logic" | "aux";
  psvId: string;
  manufacturer: string;
  model: string;
  rangeMin: number | null;
  rangeMax: number | null;
  unit: string;
  setpoint: number | null;
  silTarget: "NA" | "A" | "B" | "C" | "D";
  proofInterval: number;
  installedAt: string;
  notes: string;
  /** M2.9 — 项目归属（仪表台账按项目隔离） */
  projectId: number | null;
  /** IEC 61511-2 — 危险未检测失效率（/h） */
  lambdaDu: number;
  /** 危险已检测失效率（/h） */
  lambdaDd: number;
  /** 安全未检测失效率（/h） */
  lambdaSu: number;
  /** 安全已检测失效率（/h） */
  lambdaSd: number;
  /** 安全失效分数 SFF（0~1） */
  sff: number;
  /** 检验测试覆盖率 PTC（0~1） */
  ptCoverage: number;
  /** 硬件故障容忍度 HFT（0/1/2） */
  hft: number;
  /** IEC 61508-2 — 设备类型 type_a（简单元件）/ type_b（复杂元件） */
  equipmentType: string;
}

export interface Project {
  id: number;
  code: string;
  name: string;
  client: string;
  location: string;
  phase: string;
  startedAt: string;
  finishedAt: string;
  notes: string;
}

/** 报警台账（ISA-18.2）—— 与联锁图/SIF 无关，按项目隔离，可选关联仪表 */
export interface Alarm {
  id: number;
  projectId: number;
  tag: string;
  instrumentId: number | null;
  description: string;
  /** HH | H | LL | L | DEV | RATE | DISC | OTHER */
  alarmType: string;
  /** critical | high | medium | low */
  priority: string;
  /** process | equipment | safety */
  category: string;
  setpoint: number | null;
  unit: string;
  deadband: number | null;
  delaySeconds: number;
  /** normal | active | bypassed | shelved */
  status: string;
  responseAction: string;
  notes: string;
}

export interface DiagramSummary {
  id: number;
  projectId: number;
  code: string;
  name: string;
  sifId: number | null;
  sheetSize: string;
  revision: string;
  updatedAt: string;
  sifCode: string | null;
  sifSilDesign: string | null;
}

export interface Diagram {
  id: number;
  projectId: number;
  code: string;
  name: string;
  sifId: number | null;
  sheetSize: string;
  revision: string;
  data: string;
  /** B5 乐观锁版本号：保存时随请求带上，响应里返回新版本 */
  version: number;
  updatedAt: string;
  createdAt: string;
}

export interface SifSummary {
  id: number;
  projectId: number;
  projectCode: string;
  code: string;
  name: string;
  description: string;
  silDesign: string;
  silVerified: string;
  demandMode: string;
  pfdavgTarget: number | null;
  proofInterval: number;
  /** IEC 61511-2 — 子系统表决架构 */
  sensorArch: string;
  logicArch: string;
  finalArch: string;
  /** 平均修复时间 MTTR（小时） */
  mttrHours: number;
  /** 共因失效因子 β（0~1） */
  betaFactor: number;
  /** SRS — 项目/装置 */
  plant: string;
  /** SRS — 工艺单元 */
  unit: string;
  /** SRS — 关联设备 */
  equip: string;
  /** SRS — 响应时间要求 */
  responseTime: string;
  /** SRS — 安全状态定义 */
  safeState: string;
  /** SRS — 复位要求 */
  resetReq: string;
  /** SRS — 旁路管理 */
  bypassReq: string;
  /** SRS — 设计依据标准 */
  designStandard: string;
  /** SIF 生命周期阶段（design/construction/commissioning/operation/closed） */
  lifecyclePhase: string;
  detectorCount: number;
  finalCount: number;
  logicCount: number;
  auxCount: number;
  diagramCount: number;
  detectorsCsv: string;
  finalsCsv: string;
  diagramsCsv: string;
  /** 运行时计算：PFDavg（null = 数据不足） */
  pfdavgCalculated: number | null;
  /** 运行时计算：PFH（高需求/连续，1/h，null = 数据不足） */
  pfhCalculated: number | null;
  /** 运行时计算：达到的 SIL（NA/A/B/C/D） */
  silAchieved: string;
  /** 运行时计算：子系统 PFD/PFH 组件 JSON */
  pfdComponents: string;
  /** silVerified 与 silAchieved 一致性：pending/unverified/verified/overclaimed/downgraded */
  silVerifyStatus: string;
  /** Route 1H — 架构与实际通道数匹配状态：matched/degraded/empty */
  sensorMatch: string;
  logicMatch: string;
  finalMatch: string;
}

/** SIF 基础字段（create/update/get 返回）；不含跨图汇总（参考 SifSummary） */
export interface SifBasic {
  id: number;
  projectId: number;
  code: string;
  name: string;
  description: string;
  silDesign: string;
  silVerified: string;
  demandMode: string;
  pfdavgTarget: number | null;
  proofInterval: number;
  sensorArch: string;
  logicArch: string;
  finalArch: string;
  mttrHours: number;
  betaFactor: number;
  plant: string;
  unit: string;
  equip: string;
  responseTime: string;
  safeState: string;
  resetReq: string;
  bypassReq: string;
  designStandard: string;
  lifecyclePhase: string;
}

export interface SifLink {
  id: number;
  sifId: number;
  instrumentId: number;
  role: string;
  portIndex: number;
  diagramId: number | null;
  note: string;
  tag: string;
  kind: string;
  service: string;
  silTarget: string;
}

// ============================================================================
// M2.1 — 仪表批量导入
// ============================================================================

export type ColKind = "number" | "text" | "empty" | "mixed";

export type ImportStrategy = "skip" | "overwrite" | "create_with_suffix";

export const IMPORT_TARGETS = [
  "tag",
  "service",
  "kind",
  "role",
  "manufacturer",
  "model",
  "psv_id",
  "range",
  "range_min",
  "range_max",
  "unit",
  "setpoint",
  "sil_target",
  "proof_interval",
  "installed_at",
  "notes",
] as const;
export type ImportTarget = (typeof IMPORT_TARGETS)[number];

export interface ParsedHeader {
  index: number;
  name: string;
  kind: ColKind;
  samples: string[];
}

export interface SuggestedMapping {
  column: number;
  target: string | null;
  confidence: number;
}

export interface ParsedSheet {
  sourceName: string;
  format: string;
  totalRows: number;
  headers: ParsedHeader[];
  previewRows: string[][];
  suggestedMapping: SuggestedMapping[];
  rows: string[][];
}

export interface MappingEntry {
  column: number;
  target: ImportTarget | "";
}

export interface RowError {
  row: number;
  reason: string;
}

export interface CommitImportResult {
  batchId: number;
  inserted: number;
  updated: number;
  skipped: number;
  failed: number;
  errors: RowError[];
}

// ============================================================================
// M2.2 — 旁路授权台账（IEC 61511-1 §11.5.2）
// ============================================================================

export type BypassStatus = "active" | "overdue" | "restored";

export interface BypassListItem {
  id: number;
  projectId: number;
  projectCode: string;
  sifId: number;
  sifCode: string;
  /** SIF.sil_verified（验算后的 SIL） */
  sifSil: string;
  bypassedBy: string;
  bypassedAt: string;
  reason: string;
  plannedRestore: string;
  restoredAt: string;
  permitNo: string;
  approvedBy: string;
  /** SQL CASE 计算：active | overdue | restored */
  status: BypassStatus;
  /** hours_to_restore = (planned - now) × 24；正=距到期，负=已逾期 */
  hoursToRestore: number;
}

/** M2.2 增强：全站 overdue 轻量状态（横幅用） */
export interface OverdueBypassStatus {
  count: number;
  /** 最久一条已逾期小时数；null 当 count=0 */
  oldestOverdueHours: number | null;
}

export interface BypassInput {
  projectId: number;
  sifId: number;
  bypassedBy: string;
  reason: string;
  /** ISO 8601 / RFC 3339 */
  plannedRestore: string;
  permitNo?: string;
  approvedBy: string;
}

export interface BypassUpdate {
  bypassedBy?: string;
  reason?: string;
  plannedRestore?: string;
  permitNo?: string;
  approvedBy?: string;
}

// ============================================================================
// M2.3 — 仪表修改历史（字段级 diff）
//
// payload 协议：{before, after, fieldsChanged}
//   - instrument_create → before=null, after=<full row>, fieldsChanged=["*"]
//   - instrument_update → before=<full row>, after=<full row>, fieldsChanged=[...]
//   - instrument_delete → before=<full row>, after=null, fieldsChanged=["*"]
// ============================================================================

export interface InstrumentHistoryAction {
  raw: string;
  kind: "create" | "update" | "delete" | "link" | "unlink";
}

/** M2.4 通用 AuditEntry —— 后端 audit_log 的一条（含 payload 解析后的字段） */
export interface AuditEntry {
  id: number;
  ts: string;
  actor: string;
  /** 后端 action 字段，如 'instrument_create' / 'sif_link' / 'project_update' */
  action: string;
  targetTable: string;
  targetId: number | null;
  payloadJson: string;
  before: Record<string, unknown> | null;
  after: Record<string, unknown> | null;
  fieldsChanged: string[];
  /** sif_link / sif_unlink 等非字段级操作的自然语言描述 */
  note: string;
}

// ============================================================================
// 检验测试台账（IEC 61511-1 §16.3）
// ============================================================================

/** 检验测试记录（nextDueAt = testedAt + SIF.proofInterval 月） */
export interface ProofTest {
  id: number;
  sifId: number;
  sifCode: string;
  projectCode: string;
  testedAt: string;
  /** pass | fail | conditional */
  result: string;
  testedBy: string;
  nextDueAt: string;
  findings: string;
  notes: string;
  createdAt: string;
  /** 关联的检验规程 SOP（§16.2.2） */
  sopId: number | null;
  sopCode: string | null;
  sopTitle: string | null;
  sopVersion: string | null;
  /** SQL CASE：overdue（最新一条且 nextDueAt < today）| current */
  status: string;
  /** 是否按成文规程执行 */
  sopLinked: boolean;
}

/** 逾期检验计数（供横幅） */
export interface OverdueProofTestStatus {
  count: number;
  oldestOverdueDays: number | null;
}

/** 检验测试新建/更新输入 */
export interface ProofTestInput {
  sifId: number;
  testedAt: string;
  result?: string;
  testedBy?: string;
  findings?: string;
  notes?: string;
  /** 关联的检验规程 id */
  sopId?: number | null;
}

// ============================================================================
// 检验测试规程 SOP（IEC 61511-1 §16.2.2）
// ============================================================================

/** 检验规程（含被检验记录引用计数） */
export interface ProofTestSop {
  id: number;
  orgId: number;
  code: string;
  title: string;
  version: string;
  docRef: string;
  scope: string;
  testMethod: string;
  passCriteria: string;
  notes: string;
  createdAt: string;
  updatedAt: string;
  usageCount: number;
}

/** 检验规程新建/更新输入 */
export interface ProofTestSopInput {
  code: string;
  title: string;
  version?: string;
  docRef?: string;
  scope?: string;
  testMethod?: string;
  passCriteria?: string;
  notes?: string;
}

// ============================================================================
// LOPA（保护层分析）—— IEC 61511-1 Annex E
// ============================================================================

/** LOPA 场景（含保护层 COUNT 聚合） */
export interface LopaScenario {
  id: number;
  orgId: number;
  projectId: number;
  sifId: number | null;
  projectCode: string;
  sifCode: string | null;
  code: string;
  title: string;
  hazard: string;
  cause: string;
  consequence: string;
  /** minor | medium | major | catastrophic */
  severity: string;
  initFreq: number | null;
  riskTol: number | null;
  /** NA | A | B | C | D */
  silClaim: string;
  notes: string;
  createdAt: string;
  updatedAt: string;
  layerCount: number;
  // ---- 运行时 gap analysis（不落库） ----
  /** 关联 SIF 的设计目标 / 验证值 */
  sifSilDesign: string | null;
  sifSilVerified: string | null;
  /** RRF = initFreq / riskTol */
  rrfRequired: number | null;
  /** RRF 区间反推的 SIL（NA/A/B/C/D） */
  silFromRrf: string | null;
  /** 关联 SIF 计算出的 silAchieved */
  sifSilAchieved: string | null;
  /** unlinked | na | pending_data | covered | gap | drift */
  gapStatus: string;
  /** 人类可读对照说明 */
  gapMessage: string;
}

/** 新建/更新场景输入 */
export interface LopaScenarioInput {
  projectId: number;
  sifId?: number | null;
  code: string;
  title: string;
  hazard?: string;
  cause?: string;
  consequence?: string;
  /** minor | medium | major | catastrophic */
  severity?: string;
  initFreq?: number | null;
  riskTol?: number | null;
  /** NA | A | B | C | D */
  silClaim?: string;
  notes?: string;
}

/** 独立保护层 */
export interface LopaLayer {
  id: number;
  orgId: number;
  scenarioId: number;
  seq: number;
  /** ipl | bypass | alarm | procedural */
  layerType: string;
  description: string;
  pfd: number | null;
  credit: number;
  createdAt: string;
}

/** 新建/更新保护层输入 */
export interface LopaLayerInput {
  scenarioId: number;
  seq?: number;
  layerType: string;
  description?: string;
  pfd?: number | null;
  credit?: number;
}

/** 抽屉打开的实体目标 */
export interface HistoryTarget {
  /** 'instrument' | 'sif' | 'project' | 'alarm' */
  kind: "instrument" | "sif" | "project" | "alarm";
  id: number;
}

// 旧 M2.3 类型别名（向后兼容；新代码统一用 AuditEntry）
export type InstrumentHistoryEntry = AuditEntry;

// ============================================================================
// M2.5 — 审计包导出（CSV + 多维筛选）
// ============================================================================

/** 审计筛选条件（与 Rust AuditFilterInput 一一对应，6 维全可选） */
export interface AuditFilter {
  targetTable?: string;
  action?: string;
  actor?: string;
  /** RFC 3339 / ISO 8601（如 '2026-09-01T00:00:00Z'） */
  tsFrom?: string;
  tsTo?: string;
  targetId?: number;
}

/** 下拉框的可筛选项（去重后的全集） */
export interface AuditFilterOptions {
  actions: string[];
  actors: string[];
  tables: string[];
}

/** 命中汇总（按 action / table 维度） */
export interface AuditSummary {
  total: number;
  byAction: Record<string, number>;
  byTable: Record<string, number>;
}

/** M2.7 图表：每日审计活动量（按 substr(ts, 1, 10) 分组） */
export interface DailyActivity {
  date: string; // YYYY-MM-DD
  count: number;
}

/** M2.7 图表：旁路时长分桶（固定 5 桶） */
export interface DurationBucket {
  label: string;
  count: number;
  /** 桶下界（小时），用于前端排序稳定 */
  lowerHours: number;
}

/** M2.7 图表：SIL 验算等级变更事件 */
export interface SilChangeEvent {
  ts: string;
  sifId: number;
  sifCode: string;
  /** sif_create 时为空 */
  fromSil: string;
  /** 验算后等级（A/B/C/D/NA） */
  toSil: string;
  action: string;
}

/** M2.7 三合一图表数据（一次拿全给 PDF 模板） */
export interface AuditCharts {
  dailyActivity: DailyActivity[];
  bypassDurationBuckets: DurationBucket[];
  silChangeTimeline: SilChangeEvent[];
}

/** 导出结果 */
export interface ExportResult {
  path: string;
  bytes: number;
  rows: number;
  /** 命中服务端 10 万行上限被截断（仅 CSV 导出可能出现） */
  truncated?: boolean;
}

// ============================================================================
// 通用错误类
// ============================================================================
export interface InvokeError {
  kind: string;
  message: string;
}

function asError(e: unknown): InvokeError {
  if (e && typeof e === "object" && "kind" in (e as object)) {
    return e as InvokeError;
  }
  if (e instanceof Error) {
    return { kind: "unknown", message: e.message };
  }
  return { kind: "unknown", message: String(e) };
}

// ============================================================================
// Slice State
// ============================================================================
interface StudioState {
  meta: AppMeta | null;
  instruments: Instrument[];
  /** 报警台账（跨项目全量；项目视图在前端按 projectId 过滤） */
  alarms: Alarm[];
  projects: Project[];
  diagramsByProject: Record<number, DiagramSummary[]>;
  sifSummary: SifSummary[];
  lastError: InvokeError | null;
  ready: boolean;
  // ---- M2.1 仪表批量导入 ----
  importSheet: ParsedSheet | null;
  importMapping: MappingEntry[];
  importStrategy: ImportStrategy;
  importResult: CommitImportResult | null;
  importing: boolean;
  /** M2.9 — 导入目标项目（仪表台账按项目隔离） */
  importTargetProjectId: number | null;
  // ---- M2.2 旁路授权台账 ----
  bypasses: BypassListItem[];
  bypassesFilter: "all" | BypassStatus;
  bypassesLoading: boolean;
  /** M2.2 增强：全站启动时扫 overdue 的轻量状态（横幅用） */
  overdueBypassStatus: OverdueBypassStatus;
  /** 60s 轮询 setInterval id；actions 私有 */
  _overdueTimer: number;
  // ---- 检验测试（IEC 61511-1 §16.3）----
  /** 按 sifId 过滤后的检验记录（展开 SIF 时拉取） */
  proofTests: ProofTest[];
  /** 当前检验列表锁定的 SIF（null = 全量） */
  proofTestsFilterSifId: number | null;
  /** 全站逾期检验计数（横幅用，60s 轮询刷新） */
  overdueProofTestStatus: OverdueProofTestStatus;
  // ---- 检验规程 SOP（IEC 61511-1 §16.2.2）----
  proofTestSops: ProofTestSop[];
  // ---- M2.3/M2.4 通用修改历史（M2.3 仅 instrument；M2.4 扩到 sif + project）----
  entityHistory: AuditEntry[];
  /** 当前打开抽屉的目标；null = 抽屉关闭 */
  entityHistoryFor: HistoryTarget | null;
  entityHistoryLoading: boolean;
  // ---- M2.5 审计包导出 ----
  auditFilters: AuditFilterOptions;
  auditPreview: AuditEntry[];
  auditSummary: AuditSummary | null;
  auditLoading: boolean;
  auditExporting: boolean;
  // ---- M2.7 审计包图表（PDF 报告增补图表用）----
  auditCharts: AuditCharts | null;
  auditChartsLoading: boolean;
  // ---- LOPA（IEC 61511-1 Annex E）----
  lopaScenarios: LopaScenario[];
  lopaFilterProjectId: number | null;
  lopaLayers: LopaLayer[];
  lopaLayersForScenarioId: number | null;
}

export const useStudioStore = defineStore("studio", {
  state: (): StudioState => ({
    meta: null,
    instruments: [],
    alarms: [],
    projects: [],
    diagramsByProject: {},
    sifSummary: [],
    lastError: null,
    ready: false,
    // M2.1 导入初始状态
    importSheet: null,
    importMapping: [],
    importStrategy: "skip",
    importResult: null,
    importing: false,
    // M2.9 — 导入目标项目（默认未选）
    importTargetProjectId: null,
    // M2.2 旁路初始状态
    bypasses: [],
    bypassesFilter: "all",
    bypassesLoading: false,
    overdueBypassStatus: { count: 0, oldestOverdueHours: null },
    _overdueTimer: 0,
    // 检验测试（IEC 61511-1 §16.3）
    proofTests: [],
    proofTestsFilterSifId: null as number | null,
    overdueProofTestStatus: { count: 0, oldestOverdueDays: null },
    proofTestSops: [],
    // M2.4 通用 history 初始状态
    entityHistory: [],
    entityHistoryFor: null,
    entityHistoryLoading: false,
    // M2.5 审计包导出初始状态
    auditFilters: { actions: [], actors: [], tables: [] },
    auditPreview: [],
    auditSummary: null,
    auditLoading: false,
    auditExporting: false,
    // M2.7 图表初始状态
    auditCharts: null,
    auditChartsLoading: false,
    // LOPA（IEC 61511-1 Annex E）初始状态
    lopaScenarios: [],
    lopaFilterProjectId: null as number | null,
    lopaLayers: [],
    lopaLayersForScenarioId: null as number | null,
  }),

  getters: {
    instrumentsByRole: (s) => (role: Instrument["role"]) =>
      s.instruments.filter((i) => i.role === role),
    detectors: (s) => s.instruments.filter((i) => i.role === "detector"),
    finals: (s) => s.instruments.filter((i) => i.role === "final"),
    auxes: (s) => s.instruments.filter((i) => i.role === "aux"),
    /**
     * M2.9 — 当前 store 已加载的项目下仪表（用于 inline 编辑器实时刷新）
     * 画图 picker 拿这份数据；上游应保证先调 refreshInstrumentsByProject(pid)
     */
    instrumentsByProject: (s) => (projectId: number | null | undefined) =>
      projectId == null
        ? s.instruments
        : s.instruments.filter((i) => i.projectId === projectId),
    sifByCode: (s) => (code: string) =>
      s.sifSummary.find((x) => x.code === code),
    // M2.2 — 旁路聚合
    activeBypasses: (s) => s.bypasses.filter((b) => b.status === "active"),
    overdueBypasses: (s) => s.bypasses.filter((b) => b.status === "overdue"),
    restoredBypasses: (s) => s.bypasses.filter((b) => b.status === "restored"),
    bypassesBySif: (s) => (sifId: number) =>
      s.bypasses.filter(
        (b) => b.sifId === sifId && (b.status === "active" || b.status === "overdue"),
      ),
  },

  actions: {
    // ------------------------------------------------------------------
    // 顶层 bootstrap —— main.ts 启动时调一次
    // ------------------------------------------------------------------
    async bootstrap() {
      try {
        await Promise.all([
          this.refreshMeta(),
          this.refreshInstruments(),
          this.refreshProjects(),
          this.refreshSifSummary(),
          this.refreshBypasses("all"),
          this.refreshOverdueBypassStatus(),
          this.refreshOverdueProofTests(),
        ]);
        // 启动 60s 轮询；count=0 也轮询（轻量）
        this.startOverduePolling();
        this.ready = true;
      } catch (e) {
        this.lastError = asError(e);
        throw e;
      }
    },

    async refreshMeta() {
      try {
        this.meta = await invoke<AppMeta>("db_version");
      } catch (e) {
        this.lastError = asError(e);
      }
    },

    /**
     * M2.2 增强：扫全站 overdue 计数（轻量，只 count + max(hours)）
     * - bootstrap 时调一次
     * - 60s 轮询再调一次（防后台忘记恢复）
     * - 失败 → 不抛、不弹错（横幅可降级隐藏）
     */
    async refreshOverdueBypassStatus() {
      try {
        const s = await invoke<{ count: number; oldestOverdueHours: number | null }>(
          "count_overdue_bypasses",
        );
        this.overdueBypassStatus = {
          count: Number(s?.count ?? 0),
          oldestOverdueHours: s?.oldestOverdueHours ?? null,
        };
      } catch (e) {
        // 横幅降级：保持上次值，不让启动崩
        this.lastError = asError(e);
      }
    },

    /** 60s 轮询 overdue 状态；返回 cancel 函数 */
    _overdueTimer: 0 as number,
    startOverduePolling() {
      this.stopOverduePolling();
      this._overdueTimer = window.setInterval(() => {
        // 轮询时拉详情也拉一次（让 Home 卡也更新）
        this.refreshOverdueBypassStatus();
        this.refreshOverdueProofTests();
      }, 60_000);
    },
    stopOverduePolling() {
      if (this._overdueTimer) {
        clearInterval(this._overdueTimer);
        this._overdueTimer = 0;
      }
    },

    // ------------------------------------------------------------------
    // 检验测试（IEC 61511-1 §16.3）
    // ------------------------------------------------------------------
    async refreshOverdueProofTests() {
      try {
        const s = await invoke<OverdueProofTestStatus>("count_overdue_proof_tests");
        this.overdueProofTestStatus = {
          count: Number(s?.count ?? 0),
          oldestOverdueDays: s?.oldestOverdueDays ?? null,
        };
      } catch (e) {
        this.lastError = asError(e);
      }
    },

    async refreshProofTests(sifId?: number) {
      this.proofTestsFilterSifId = sifId ?? null;
      this.proofTests = await invoke<ProofTest[]>("list_proof_tests", {
        sifId: sifId ?? null,
      });
    },

    async createProofTest(input: ProofTestInput): Promise<ProofTest> {
      const pt = await invoke<ProofTest>("create_proof_test", { input });
      await this.refreshProofTests(this.proofTestsFilterSifId ?? undefined);
      await this.refreshOverdueProofTests();
      return pt;
    },

    async updateProofTest(id: number, input: ProofTestInput): Promise<ProofTest> {
      const pt = await invoke<ProofTest>("update_proof_test", { id, input });
      await this.refreshProofTests(this.proofTestsFilterSifId ?? undefined);
      await this.refreshOverdueProofTests();
      return pt;
    },

    async deleteProofTest(id: number) {
      await invoke("delete_proof_test", { id });
      await this.refreshProofTests(this.proofTestsFilterSifId ?? undefined);
      await this.refreshOverdueProofTests();
    },

    // ------------------------------------------------------------------
    // 检验规程 SOP（IEC 61511-1 §16.2.2）
    // ------------------------------------------------------------------
    async refreshProofTestSops() {
      this.proofTestSops = await invoke<ProofTestSop[]>("list_proof_test_sops");
    },

    async createProofTestSop(input: ProofTestSopInput): Promise<ProofTestSop> {
      const sop = await invoke<ProofTestSop>("create_proof_test_sop", { input });
      await this.refreshProofTestSops();
      return sop;
    },

    async updateProofTestSop(id: number, input: ProofTestSopInput): Promise<ProofTestSop> {
      const sop = await invoke<ProofTestSop>("update_proof_test_sop", { id, input });
      await this.refreshProofTestSops();
      // 规程变更后刷新检验列表（快照 version/title 可能变）
      await this.refreshProofTests(this.proofTestsFilterSifId ?? undefined);
      return sop;
    },

    async deleteProofTestSop(id: number, force = false) {
      await invoke("delete_proof_test_sop", { id, force });
      await this.refreshProofTestSops();
      await this.refreshProofTests(this.proofTestsFilterSifId ?? undefined);
    },

    // ------------------------------------------------------------------
    // LOPA（IEC 61511-1 Annex E）
    // ------------------------------------------------------------------
    async refreshLopaScenarios(projectId?: number) {
      this.lopaFilterProjectId = projectId ?? null;
      this.lopaScenarios = await invoke<LopaScenario[]>("list_lopa_scenarios", {
        projectId: projectId ?? null,
      });
    },

    async createLopaScenario(input: LopaScenarioInput): Promise<LopaScenario> {
      const s = await invoke<LopaScenario>("create_lopa_scenario", { input });
      await this.refreshLopaScenarios(this.lopaFilterProjectId ?? undefined);
      return s;
    },

    async updateLopaScenario(id: number, input: LopaScenarioInput): Promise<LopaScenario> {
      const s = await invoke<LopaScenario>("update_lopa_scenario", { id, input });
      await this.refreshLopaScenarios(this.lopaFilterProjectId ?? undefined);
      if (this.lopaLayersForScenarioId === id) {
        await this.refreshLopaLayers(id);
      }
      return s;
    },

    async deleteLopaScenario(id: number) {
      await invoke("delete_lopa_scenario", { id });
      await this.refreshLopaScenarios(this.lopaFilterProjectId ?? undefined);
      if (this.lopaLayersForScenarioId === id) {
        this.lopaLayers = [];
        this.lopaLayersForScenarioId = null;
      }
    },

    async refreshLopaLayers(scenarioId: number) {
      this.lopaLayersForScenarioId = scenarioId;
      this.lopaLayers = await invoke<LopaLayer[]>("list_lopa_layers", {
        scenarioId,
      });
    },

    async createLopaLayer(input: LopaLayerInput): Promise<LopaLayer> {
      const l = await invoke<LopaLayer>("create_lopa_layer", { input });
      if (this.lopaLayersForScenarioId === input.scenarioId) {
        await this.refreshLopaLayers(input.scenarioId);
      }
      // 刷新场景的 layerCount
      await this.refreshLopaScenarios(this.lopaFilterProjectId ?? undefined);
      return l;
    },

    async updateLopaLayer(id: number, input: LopaLayerInput): Promise<LopaLayer> {
      const l = await invoke<LopaLayer>("update_lopa_layer", { id, input });
      if (this.lopaLayersForScenarioId === input.scenarioId) {
        await this.refreshLopaLayers(input.scenarioId);
      }
      return l;
    },

    async deleteLopaLayer(id: number, scenarioId: number) {
      await invoke("delete_lopa_layer", { id });
      if (this.lopaLayersForScenarioId === scenarioId) {
        await this.refreshLopaLayers(scenarioId);
      }
      await this.refreshLopaScenarios(this.lopaFilterProjectId ?? undefined);
    },

    // ------------------------------------------------------------------
    // Instruments
    // ------------------------------------------------------------------
    /**
     * 全量刷新（仪表管理页默认加载）
     * 注：M2.9 之后仪表按项目隔离，刷新全量仍有意义（仪表管理页需要看
     * 跨项目视图，或在切换筛选器时拿到所有候选）。但画图场景下应改用
     * refreshInstrumentsByProject(pid)，避免不必要的 IO + 渲染抖动。
     */
    async refreshInstruments() {
      this.instruments = await invoke<Instrument[]>("list_instruments");
    },

    /** M2.9 — 按项目刷新仪表（画图 picker 用，仪表台账项目级视图也用） */
    async refreshInstrumentsByProject(projectId: number) {
      const list = await invoke<Instrument[]>("list_instruments_by_project", {
        projectId,
      });
      // 把 store.instruments 替换为「该项目 + 已加载过的其他项目」
      // 简化：先合并再覆盖
      const others = this.instruments.filter(
        (i) => i.projectId !== projectId && i.projectId !== null,
      );
      this.instruments = [...others, ...list];
    },

    async createInstrument(input: Omit<Instrument, "id">): Promise<Instrument> {
      if (!input.projectId) {
        throw new Error("createInstrument: projectId is required (M2.9 起仪表必须归属项目)");
      }
      const created = await invoke<Instrument>("create_instrument", { input });
      await this.refreshInstruments();
      await this._refreshOpenInstrumentHistoryIfAny(created.id);
      return created;
    },

    async updateInstrument(
      id: number,
      input: Omit<Instrument, "id">,
    ): Promise<Instrument> {
      if (!input.projectId) {
        throw new Error("updateInstrument: projectId is required");
      }
      const updated = await invoke<Instrument>("update_instrument", {
        id,
        input,
      });
      await this.refreshInstruments();
      await this._refreshOpenInstrumentHistoryIfAny(id);
      return updated;
    },

    async deleteInstrument(id: number) {
      await invoke("delete_instrument", { id });
      await this.refreshInstruments();
      // 删除后抽屉仍开着（显示 delete 记录）→ 顺手刷新
      await this._refreshOpenInstrumentHistoryIfAny(id);
    },

    // ------------------------------------------------------------------
    // Alarms（报警台账，ISA-18.2；项目隔离，与联锁图/SIF 无关）
    // ------------------------------------------------------------------
    async refreshAlarms() {
      this.alarms = await invoke<Alarm[]>("list_alarms");
    },

    async createAlarm(input: Omit<Alarm, "id">): Promise<Alarm> {
      if (!input.projectId) {
        throw new Error("createAlarm: projectId is required（报警台账按项目隔离）");
      }
      const created = await invoke<Alarm>("create_alarm", { input });
      await this.refreshAlarms();
      await this._refreshOpenAlarmHistoryIfAny(created.id);
      return created;
    },

    async updateAlarm(id: number, input: Omit<Alarm, "id">): Promise<Alarm> {
      if (!input.projectId) {
        throw new Error("updateAlarm: projectId is required");
      }
      const updated = await invoke<Alarm>("update_alarm", { id, input });
      await this.refreshAlarms();
      await this._refreshOpenAlarmHistoryIfAny(id);
      return updated;
    },

    async deleteAlarm(id: number) {
      await invoke("delete_alarm", { id });
      await this.refreshAlarms();
      await this._refreshOpenAlarmHistoryIfAny(id);
    },

    async _refreshOpenAlarmHistoryIfAny(id: number) {
      if (
        this.entityHistoryFor?.kind === "alarm" &&
        this.entityHistoryFor?.id === id
      ) {
        await this.refreshEntityHistory();
      }
    },

// ------------------------------------------------------------------
// 通用修改历史（M2.3 仪表 → M2.4 扩到 SIF + Project）
//
// 设计：单一 entityHistoryFor / entityHistory 字段被三种实体共用。
// kind 后端命令分别走 list_instrument_history / list_sif_history / list_project_history。
// ------------------------------------------------------------------

    async openEntityHistory(kind: HistoryTarget["kind"], id: number) {
      this.entityHistoryFor = { kind, id };
      this.entityHistory = [];
      await this.refreshEntityHistory();
    },

    closeEntityHistory() {
      this.entityHistoryFor = null;
    },

    async refreshEntityHistory() {
      const target = this.entityHistoryFor;
      if (!target) return;
      this.entityHistoryLoading = true;
      try {
        let list: AuditEntry[] = [];
        if (target.kind === "instrument") {
          list = await invoke<AuditEntry[]>("list_instrument_history", {
            instrumentId: target.id,
            limit: 100,
          });
        } else if (target.kind === "sif") {
          list = await invoke<AuditEntry[]>("list_sif_history", {
            sifId: target.id,
            limit: 200,
          });
        } else if (target.kind === "project") {
          list = await invoke<AuditEntry[]>("list_project_history", {
            projectId: target.id,
            limit: 200,
          });
        } else if (target.kind === "alarm") {
          list = await invoke<AuditEntry[]>("list_alarm_history", {
            alarmId: target.id,
            limit: 100,
          });
        }
        this.entityHistory = list ?? [];
      } finally {
        this.entityHistoryLoading = false;
      }
    },

    /** M2.3 兼容：仪表历史快捷入口（路由到通用 API） */
    async openInstrumentHistory(id: number) {
      return this.openEntityHistory("instrument", id);
    },

    /** M2.3 兼容：删除/创建后若抽屉还开着则刷新 */
    async _refreshOpenInstrumentHistoryIfAny(id: number) {
      if (
        this.entityHistoryFor?.kind === "instrument" &&
        this.entityHistoryFor?.id === id
      ) {
        await this.refreshEntityHistory();
      }
    },

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------
    async refreshProjects() {
      this.projects = await invoke<Project[]>("list_projects");
    },

    async ensureDefaultProject(): Promise<Project> {
      const p = await invoke<Project>("ensure_default_project");
      await this.refreshProjects();
      return p;
    },

    async createProject(input: Omit<Project, "id" | "startedAt">) {
      const p = await invoke<Project>("create_project", { input });
      await this.refreshProjects();
      await this._refreshOpenProjectHistoryIfAny(p.id);
      return p;
    },

    /** M2.4：项目编辑入口 */
    async updateProject(
      id: number,
      input: Omit<Project, "id" | "startedAt">,
    ) {
      const p = await invoke<Project>("update_project", { id, input });
      await this.refreshProjects();
      await this._refreshOpenProjectHistoryIfAny(id);
      return p;
    },

    /** M2.4：删除项目 */
    async deleteProject(id: number) {
      const n = await invoke<number>("delete_project", { id });
      await this.refreshProjects();
      await this._refreshOpenProjectHistoryIfAny(id);
      return n;
    },

    async _refreshOpenProjectHistoryIfAny(id: number) {
      if (
        this.entityHistoryFor?.kind === "project" &&
        this.entityHistoryFor?.id === id
      ) {
        await this.refreshEntityHistory();
      }
    },

    async ensureDefaultDiagram(projectId: number): Promise<Diagram> {
      return await invoke<Diagram>("ensure_default_diagram", { projectId });
    },

    async listDiagrams(projectId?: number): Promise<DiagramSummary[]> {
      const list = await invoke<DiagramSummary[]>("list_diagrams", { projectId });
      if (projectId !== undefined) this.diagramsByProject[projectId] = list;
      return list;
    },

    async loadDiagram(id: number): Promise<Diagram> {
      return await invoke<Diagram>("get_diagram", { id });
    },

    async createDiagram(input: {
      projectId: number;
      code: string;
      name: string;
      sheetSize?: string;
      revision?: string;
      sifId?: number | null;
      data?: string;
    }): Promise<Diagram> {
      const d = await invoke<Diagram>("create_diagram", { input });
      // 建图时后端会自动配套创建 SIF（一张图 = 一个 SIF），同步刷新两处缓存
      await Promise.all([
        this.listDiagrams(input.projectId),
        this.refreshSifSummary(),
      ]);
      return d;
    },

    /** B5 — 乐观锁保存：带期望 version，成功返回 {id, version(新)}；过期抛 409 conflict */
    async saveDiagramData(
      id: number,
      data: string,
      version: number,
    ): Promise<{ id: number; version: number }> {
      return await invoke("save_diagram_data", { id, data, version });
    },

    // ------------------------------------------------------------------
    // SIFs（跨图汇总 + 关联仪表）
    // ------------------------------------------------------------------
    async refreshSifSummary(projectId?: number) {
      this.sifSummary = await invoke<SifSummary[]>("list_sifs", { projectId });
    },

    async createSif(input: {
      projectId: number;
      code: string;
      name: string;
      description?: string;
      silDesign?: string;
      silVerified?: string;
      demandMode?: string;
      pfdavgTarget?: number | null;
      proofInterval?: number;
      sensorArch?: string;
      logicArch?: string;
      finalArch?: string;
      mttrHours?: number;
      betaFactor?: number;
    }) {
      const s: SifBasic = await invoke("create_sif", { input });
      await this.refreshSifSummary();
      await this._refreshOpenSifHistoryIfAny((s as { id: number }).id);
      return s;
    },

    /** M2.4：SIF 编辑入口（HTML 表单同 create，沿用 SifInput） */
    async updateSif(
      id: number,
      input: {
        projectId: number;
        code: string;
        name: string;
        description?: string;
        silDesign?: string;
        silVerified?: string;
        demandMode?: string;
        pfdavgTarget?: number | null;
        proofInterval?: number;
        sensorArch?: string;
        logicArch?: string;
        finalArch?: string;
        mttrHours?: number;
        betaFactor?: number;
      },
    ) {
      const s: SifBasic = await invoke("update_sif", { id, input });
      await this.refreshSifSummary();
      await this._refreshOpenSifHistoryIfAny(id);
      return s;
    },

    async deleteSif(id: number): Promise<number> {
      const n = await invoke<number>("delete_sif", { id });
      await this.refreshSifSummary();
      await this._refreshOpenSifHistoryIfAny(id);
      return n;
    },

    async _refreshOpenSifHistoryIfAny(id: number) {
      if (
        this.entityHistoryFor?.kind === "sif" &&
        this.entityHistoryFor?.id === id
      ) {
        await this.refreshEntityHistory();
      }
    },

    async listSifLinks(sifId: number): Promise<SifLink[]> {
      return await invoke<SifLink[]>("list_sif_links", { sifId });
    },

    async linkInstrument(input: {
      sifId: number;
      instrumentId: number;
      role: string;
      portIndex: number;
      diagramId?: number | null;
      note?: string;
    }) {
      await invoke("link_instrument_to_sif", {
        sifId: input.sifId,
        instrumentId: input.instrumentId,
        role: input.role,
        portIndex: input.portIndex,
        diagramId: input.diagramId ?? null,
        note: input.note ?? "",
      });
      await this.refreshSifSummary();
      await this._refreshOpenSifHistoryIfAny(input.sifId);
    },

    async unlinkInstrument(linkId: number, sifId?: number) {
      await invoke("unlink_instrument_from_sif", { linkId });
      await this.refreshSifSummary();
      if (sifId !== undefined) {
        await this._refreshOpenSifHistoryIfAny(sifId);
      }
    },

    // ------------------------------------------------------------------
    // M2.1 仪表批量导入
    // ------------------------------------------------------------------

    /**
     * 在线版：multipart 上传文件到 POST /api/imports/preview（后端不落盘直接解析）。
     * 返回 ParsedSheet 并自动填充建议映射。
     */
    async previewImport(file: File): Promise<ParsedSheet> {
      this.importing = true;
      this.importResult = null;
      try {
        const form = new FormData();
        form.append("file", file, file.name);

        const res = await fetch("/api/imports/preview", {
          method: "POST",
          credentials: "same-origin",
          body: form,
        });
        if (!res.ok) {
          let kind = "http";
          let message = `${res.status} ${res.statusText}`;
          try {
            const body = (await res.json()) as {
              kind?: string;
              message?: string;
            };
            kind = body.kind ?? kind;
            message = body.message ?? message;
          } catch {
            /* 非 JSON 错误体 */
          }
          throw Object.assign(new Error(message), { kind });
        }

        const sheet = (await res.json()) as ParsedSheet;
        this.importSheet = sheet;
        // 用后端建议的 mapping 自动填默认
        this.importMapping = sheet.suggestedMapping
          .filter((m) => m.target != null)
          .map((m) => ({
            column: m.column,
            target: (m.target ?? "") as ImportTarget,
          }));
        return sheet;
      } finally {
        this.importing = false;
      }
    },

    /** 用户在 UI 上改了某列的映射 → 同步状态 */
    setMappingColumnTarget(column: number, target: ImportTarget | "") {
      const found = this.importMapping.find((m) => m.column === column);
      if (found) {
        found.target = target;
      } else if (target) {
        this.importMapping.push({ column, target });
      }
      // 清掉空目标
      this.importMapping = this.importMapping.filter((m) => m.target);
    },

    setImportStrategy(s: ImportStrategy) {
      this.importStrategy = s;
    },

    /** M2.9 — 设置导入目标项目 */
    setImportTargetProjectId(pid: number | null) {
      this.importTargetProjectId = pid;
    },

    /** 提交当前 sheet + mapping → 写库 + 审计 */
    async commitImport(): Promise<CommitImportResult> {
      if (!this.importSheet) throw new Error("no sheet loaded");
      if (this.importing) throw new Error("importing in progress");
      // M2.9 — 必须指定目标项目
      if (!this.importTargetProjectId) {
        throw new Error("import target project is required (M2.9 起导入必须指定项目)");
      }

      this.importing = true;
      try {
        const input = {
          sheet: this.importSheet,
          mapping: this.importMapping.filter((m) => m.target),
          onConflict: this.importStrategy,
          projectId: this.importTargetProjectId,
        };
        const r = await invoke<CommitImportResult>(
          "commit_import_instruments",
          { input },
        );
        this.importResult = r;
        // 顺手刷新仪表列表
        await this.refreshInstruments();
        return r;
      } finally {
        this.importing = false;
      }
    },

    /** 重置所有导入状态 */
    resetImport() {
      this.importSheet = null;
      this.importMapping = [];
      this.importResult = null;
      this.importTargetProjectId = null;
      this.importStrategy = "skip";
    },

    // ------------------------------------------------------------------
    // M2.2 旁路授权台账（IEC 61511-1 §11.5.2）
    // ------------------------------------------------------------------

    async refreshBypasses(filter?: "all" | BypassStatus, projectId?: number) {
      this.bypassesLoading = true;
      try {
        if (filter) this.bypassesFilter = filter;
        const status = this.bypassesFilter === "all" ? "all" : this.bypassesFilter;
        this.bypasses = await invoke<BypassListItem[]>("list_bypasses", {
          status,
          projectId: projectId ?? null,
        });
      } finally {
        this.bypassesLoading = false;
      }
    },

    async createBypass(input: BypassInput): Promise<BypassListItem> {
      const b = await invoke<BypassListItem>("create_bypass", { input });
      await this.refreshBypasses();
      return b;
    },

    async updateBypass(id: number, patch: BypassUpdate): Promise<BypassListItem> {
      const b = await invoke<BypassListItem>("update_bypass", { id, patch });
      await this.refreshBypasses();
      return b;
    },

    async restoreBypass(id: number, restoredAt?: string): Promise<BypassListItem> {
      const b = await invoke<BypassListItem>("restore_bypass", {
        id,
        restoredAt: restoredAt ?? null,
      });
      await this.refreshBypasses();
      return b;
    },

    async deleteBypass(id: number) {
      await invoke<number>("delete_bypass", { id });
      await this.refreshBypasses();
    },

    setBypassFilter(f: "all" | BypassStatus) {
      this.bypassesFilter = f;
    },

    // ------------------------------------------------------------------
    // 工具
    // ------------------------------------------------------------------
    clearError() {
      this.lastError = null;
    },

    // ------------------------------------------------------------------
    // M2.5 — 审计包导出（多维筛选 + 实时预览 + CSV 导出）
    // ------------------------------------------------------------------

    /** 拉下拉框的可选项（actions/actors/tables 去重全集） */
    async refreshAuditFilters() {
      try {
        this.auditFilters = await invoke<AuditFilterOptions>("list_audit_filters");
      } catch (e) {
        this.lastError = asError(e);
      }
    },

    /**
     * 拉当前 filter 的命中行预览（前 100 条）+ 统计（by action / by table）。
     * 两边并行；任一失败 → 错误进 lastError，不抛。
     */
    async refreshAuditPreview(filter: AuditFilter) {
      this.auditLoading = true;
      try {
        const [preview, summary] = await Promise.all([
          invoke<AuditEntry[]>("preview_audit_filtered", {
            filter,
            limit: 100,
          }),
          invoke<AuditSummary>("summarize_audit_filtered", { filter }),
        ]);
        this.auditPreview = preview ?? [];
        this.auditSummary = summary ?? null;
      } catch (e) {
        this.lastError = asError(e);
      } finally {
        this.auditLoading = false;
      }
    },

    /** 仅拉命中数（导出前的极轻量确认） */
    async countAuditFiltered(filter: AuditFilter): Promise<number> {
      try {
        return (await invoke<number>("count_audit_filtered", { filter })) ?? 0;
      } catch (e) {
        this.lastError = asError(e);
        return 0;
      }
    },

    /**
     * 在线版：GET /api/audit/export.csv 拉取 CSV 流并触发浏览器下载。
     * 返回文件名、字节数与是否被行数上限截断（UI 回显用）；失败按 {kind,message} 抛错。
     */
    async downloadAuditCsv(
      filter: AuditFilter,
    ): Promise<{ filename: string; bytes: number; truncated: boolean }> {
      this.auditExporting = true;
      try {
        const qs = new URLSearchParams();
        for (const [k, v] of Object.entries(filter)) {
          if (v !== undefined && v !== null && v !== "") qs.append(k, String(v));
        }

        const res = await fetch(`/api/audit/export.csv?${qs.toString()}`, {
          method: "GET",
          credentials: "same-origin",
        });
        if (!res.ok) {
          let kind = "http";
          let message = `${res.status} ${res.statusText}`;
          try {
            const body = (await res.json()) as {
              kind?: string;
              message?: string;
            };
            kind = body.kind ?? kind;
            message = body.message ?? message;
          } catch {
            /* 非 JSON 错误体 */
          }
          throw Object.assign(new Error(message), { kind });
        }

        // 优先用服务端 Content-Disposition 里的文件名
        const disposition = res.headers.get("Content-Disposition") ?? "";
        const m = /filename="?([^"]+)"?/.exec(disposition);
        const fallback = `audit-${new Date().toISOString().slice(0, 10)}.csv`;
        const filename = m?.[1] ?? fallback;

        const blob = await res.blob();
        const url = URL.createObjectURL(blob);
        const a = document.createElement("a");
        a.href = url;
        a.download = filename;
        document.body.appendChild(a);
        a.click();
        a.remove();
        URL.revokeObjectURL(url);

        return {
          filename,
          bytes: blob.size,
          truncated: res.headers.get("X-Audit-Truncated") === "true",
        };
      } finally {
        this.auditExporting = false;
      }
    },

    // ------------------------------------------------------------------
    // M2.6 — 审计包 PDF 报告生成
    // ------------------------------------------------------------------

    /**
     * 拉一份「更大的预览」专门给 PDF 用（默认 500 条，可覆盖大多数审计报告场景）。
     * 与 auditPreview (100 条) 隔离，避免大列表拖慢 UI 表格。
     */
    async fetchPdfPreview(filter: AuditFilter, limit: number = 500) {
      try {
        return await invoke<AuditEntry[]>("preview_audit_filtered", {
          filter,
          limit,
        });
      } catch (e) {
        this.lastError = asError(e);
        return [];
      }
    },

    /**
     * 生成 PDF 报告并触发浏览器下载。
     * - 数据源：auditPreview (100 条) 或 fetchPdfPreview(500 条)
     * - 返回 {bytes, rows} 给 UI 显示「上次 PDF 导出 · N 条 · M KB」
     */
    async exportAuditPdf(args: {
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
    }): Promise<{ bytes: number; rows: number; path: string | null }> {
      this.auditExporting = true;
      try {
        // 动态 import PDF 模块（懒加载）
        const { generateAuditPdf } = await import("../utils/auditPdf");
        const r = await generateAuditPdf(args);
        // PDF 是浏览器 Blob 下载，路径仅 mock 模式有意义
        const path =
          typeof args.defaultFilename === "string" ? args.defaultFilename : null;
        return { bytes: r.bytes, rows: r.rows, path };
      } finally {
        this.auditExporting = false;
      }
    },

    /** 重置 M2.5 全部状态（路由切换时调用，避免脏数据残留） */
    resetAuditState() {
      this.auditPreview = [];
      this.auditSummary = null;
      this.auditLoading = false;
      this.auditExporting = false;
      this.auditCharts = null;
      this.auditChartsLoading = false;
    },

    /**
     * M2.7 — 一次拿全 3 个图表数据（每日活动 / 旁路时长 / SIL 变更）
     * 用于 PDF 报告「四、分析图表」章节。失败返默认空图表，不阻断 PDF 生成。
     */
    async fetchAuditCharts(filter: AuditFilter): Promise<AuditCharts> {
      this.auditChartsLoading = true;
      try {
        const r = await invoke<AuditCharts>("fetch_audit_charts", { filter });
        this.auditCharts = r;
        return r;
      } catch (e) {
        this.lastError = asError(e);
        // 降级返空图表 —— PDF 仍能生成（章节显示「暂无数据」）
        const empty: AuditCharts = {
          dailyActivity: [],
          bypassDurationBuckets: [],
          silChangeTimeline: [],
        };
        this.auditCharts = empty;
        return empty;
      } finally {
        this.auditChartsLoading = false;
      }
    },
  },
});
