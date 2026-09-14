<!--
  AuditCenter.vue — M2.5 审计包导出中心

  功能：
    1. 顶部筛选表单：6 维（table / action / actor / ts_from / ts_to / target_id）全可选
    2. 实时预览：命中前 100 条 + 汇总统计（按 action / table 分组）
    3. 一键导出 CSV：
       - GET /api/audit/export.csv（BOM + UTF-8，Excel 友好）
       - 浏览器端 Blob 下载，成功 → 显示 字节数 / 行数 / 文件名

  设计要点：
    - 工业图纸风：零圆角 / 1px 黑边 / 等宽数字 / 三段式 ISO 7200
    - 在线版纯浏览器下载（a[download] + Blob），不依赖任何原生对话框
    - 筛选条件变化 → 防抖（300ms）拉预览，避免连点浪费后端
-->
<template>
  <div class="audit-center">
    <!-- 顶栏：ISO 7200 标题块 -->
    <header class="head card">
      <div class="head-text">
        <div class="head-eyebrow">SECTION 07 · M2.5 AUDIT EXPORT</div>
        <h2 class="head-title">审计中心 · AUDIT CENTER</h2>
        <div class="head-sub">
          把 audit_log 按筛选维度导出为 CSV（含字段级 diff 快照）—— 用于项目移交、合规审计、外部分析
        </div>
      </div>
      <div class="head-block">
        <div class="head-row">
          <span class="head-key">TOTAL</span>
          <span class="head-val mono">{{ auditSummary?.total ?? "—" }}</span>
        </div>
        <div class="head-row">
          <span class="head-key">TABLES</span>
          <span class="head-val mono">{{ Object.keys(auditSummary?.byTable ?? {}).length }}</span>
        </div>
        <div class="head-row">
          <span class="head-key">ACTIONS</span>
          <span class="head-val mono">{{ Object.keys(auditSummary?.byAction ?? {}).length }}</span>
        </div>
      </div>
    </header>

    <!-- 筛选表单 -->
    <section class="card filter-card">
      <div class="card-title">
        <span>筛选条件 · FILTERS</span>
        <button class="sm" @click="resetFilters">重置</button>
      </div>

      <div class="filter-grid">
        <label>
          <span class="lbl">实体表 · TABLE</span>
          <select v-model="filter.targetTable">
            <option value="">（全部）</option>
            <option v-for="t in auditFilters.tables" :key="t" :value="t">{{ t }}</option>
          </select>
        </label>
        <label>
          <span class="lbl">动作 · ACTION</span>
          <select v-model="filter.action">
            <option value="">（全部）</option>
            <option v-for="a in auditFilters.actions" :key="a" :value="a">{{ a }}</option>
          </select>
        </label>
        <label>
          <span class="lbl">操作人 · ACTOR</span>
          <select v-model="filter.actor">
            <option value="">（全部）</option>
            <option v-for="u in auditFilters.actors" :key="u" :value="u">{{ u }}</option>
          </select>
        </label>
        <label>
          <span class="lbl">起始时间 · TS FROM</span>
          <input
            v-model="filter.tsFrom"
            type="text"
            placeholder="2026-09-01T00:00:00Z"
            spellcheck="false"
          />
        </label>
        <label>
          <span class="lbl">结束时间 · TS TO</span>
          <input
            v-model="filter.tsTo"
            type="text"
            placeholder="2026-09-30T23:59:59Z"
            spellcheck="false"
          />
        </label>
        <label>
          <span class="lbl">实体 ID · TARGET ID</span>
          <input v-model.number="filter.targetId" type="number" placeholder="可选" />
        </label>
      </div>

      <div class="filter-foot">
        <span class="result-count">
          <span class="rc-key">RESULT</span>
          <span class="rc-val mono">{{ auditSummary?.total ?? "…" }}</span>
          <span class="rc-unit">条命中</span>
          <span v-if="auditPreview.length" class="rc-note">
            · 预览前 {{ auditPreview.length }} 条
          </span>
        </span>
        <span class="filter-tip">
          改筛选条件自动刷新预览（300ms 防抖）
        </span>
      </div>
    </section>

    <!-- 汇总统计 -->
    <section v-if="auditSummary && auditSummary.total > 0" class="card summary-card">
      <div class="card-title">
        <span>汇总统计 · SUMMARY</span>
        <span class="ref mono">GROUP BY action / target_table</span>
      </div>
      <div class="summary-grid">
        <div class="sum-col">
          <div class="sum-h">按动作 · BY ACTION</div>
          <div class="bar-list">
            <div
              v-for="(n, k) in sortedActions"
              :key="k"
              class="bar-row"
            >
              <span class="bar-key mono">{{ k }}</span>
              <span class="bar-track">
                <span
                  class="bar-fill"
                  :class="`bar-${kindClass(k)}`"
                  :style="{ width: pct(n, auditSummary.total) + '%' }"
                ></span>
              </span>
              <span class="bar-val mono">{{ n }}</span>
            </div>
          </div>
        </div>
        <div class="sum-col">
          <div class="sum-h">按实体表 · BY TABLE</div>
          <div class="bar-list">
            <div
              v-for="(n, k) in sortedTables"
              :key="k"
              class="bar-row"
            >
              <span class="bar-key mono">{{ k }}</span>
              <span class="bar-track">
                <span
                  class="bar-fill"
                  :class="`bar-${tableClass(k)}`"
                  :style="{ width: pct(n, auditSummary.total) + '%' }"
                ></span>
              </span>
              <span class="bar-val mono">{{ n }}</span>
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- 预览表 -->
    <section class="card preview-card">
      <div class="card-title">
        <span>预览 · PREVIEW</span>
        <span class="ref mono">
          {{ store.auditLoading ? "LOADING…" : `${auditPreview.length} ROWS` }}
        </span>
      </div>

      <div v-if="store.auditLoading" class="state">载入中…</div>
      <div v-else-if="!auditPreview.length" class="state empty">
        没有命中的审计记录。<br />
        <span class="hint">试着放宽筛选条件（如取消 ts 区间、改为「全部」）</span>
      </div>
      <table v-else class="preview-table">
        <thead>
          <tr>
            <th style="width: 130px">时间 · TS</th>
            <th style="width: 90px">操作人 · ACTOR</th>
            <th style="width: 150px">动作 · ACTION</th>
            <th style="width: 110px">表 · TABLE</th>
            <th style="width: 70px">ID</th>
            <th>变更 · SUMMARY</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="e in auditPreview" :key="e.id">
            <td class="mono">{{ e.ts }}</td>
            <td>{{ e.actor || "—" }}</td>
            <td>
              <span class="action-chip" :class="actionKind(e.action)">
                {{ actionCN(e.action) }}
              </span>
            </td>
            <td class="mono">{{ e.targetTable }}</td>
            <td class="mono num">{{ e.targetId ?? "—" }}</td>
            <td>
              <span v-if="isLinkOp(e.action)" class="link-desc">{{ e.note || "—" }}</span>
              <span v-else-if="e.fieldsChanged.length === 1 && e.fieldsChanged[0] === '*'" class="chips">
                <span class="chip all">全字段</span>
              </span>
              <span v-else-if="e.fieldsChanged.length === 0" class="chips">
                <span class="chip none">无变更</span>
              </span>
              <span v-else class="chips">
                <span v-for="f in e.fieldsChanged" :key="f" class="chip">{{ f }}</span>
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <!-- 导出动作条（F3：只读角色可在线查看审计，不能导出全量审计包） -->
    <footer v-if="auth.canWrite" class="export-bar card">
      <div class="export-text">
        <div class="export-h">导出审计包 · EXPORT AUDIT PACK</div>
        <div class="export-sub">
          CSV：机器可读，含完整 before/after JSON，Excel 双击不乱码<br />
          PDF：人读工程报告（A4 横版，嵌入中文字体子集，跨设备可读）
        </div>
      </div>
      <div class="export-actions">
        <span v-if="lastResult" class="last-result" data-test="csv-last">
          <span class="ok-mark">✓</span>
          CSV · {{ lastResult.rows }} 行 · {{ fmtBytes(lastResult.bytes) }} · {{ lastResult.path }}
          <strong v-if="lastResult.truncated" data-test="csv-truncated">
            已达 10 万行导出上限，仅含最新记录
          </strong>
        </span>
        <span v-if="lastPdfResult" class="last-result pdf" data-test="pdf-last">
          <span class="ok-mark">✓</span>
          PDF · {{ lastPdfResult.rows }} 行 · {{ fmtBytes(lastPdfResult.bytes) }} · {{ lastPdfResult.path }}
        </span>
        <button
          class="primary"
          :disabled="store.auditExporting || (auditSummary?.total ?? 0) === 0"
          @click="onExport"
          data-test="export-csv-btn"
        >
          {{ store.auditExporting && !pdfGenerating ? "导出 CSV…" : pdfGenerating ? "导出 PDF…" : "导出 CSV…" }}
        </button>
        <button
          class="primary pdf"
          :disabled="store.auditExporting || (auditSummary?.total ?? 0) === 0"
          @click="onExportPdf"
          data-test="export-pdf-btn"
        >
          {{ pdfGenerating ? "生成中…" : "导出 PDF…" }}
        </button>
      </div>
    </footer>

    <!-- F3：viewer 只读提示（服务端 /api/audit/export.csv 同时强制 403） -->
    <footer v-else class="export-bar card export-locked">
      <div class="export-text">
        <div class="export-h">审计包导出已锁定 · EXPORT LOCKED</div>
        <div class="export-sub">
          当前角色为只读 VIEWER，可在线浏览本页审计记录，但不能导出 CSV / PDF
          审计包（含字段级 before/after 快照）。如需导出，请联系工程师或所有者。
        </div>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { invoke } from "../api/rpc";
import { useStudioStore, type AuditFilter, type ExportResult } from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const store = useStudioStore();
const auth = useAuthStore();
const auditFilters = computed(() => store.auditFilters);
const auditPreview = computed(() => store.auditPreview);
const auditSummary = computed(() => store.auditSummary);

// 当前筛选条件（响应式 + 6 维全可选）
const filter = reactive<AuditFilter>({
  targetTable: "",
  action: "",
  actor: "",
  tsFrom: "",
  tsTo: "",
  targetId: undefined,
});

// 上次导出结果（成功提示横幅）
const lastResult = ref<ExportResult | null>(null);

// M2.6 — 上次 PDF 导出结果（独立横幅，与 CSV 并存）
const lastPdfResult = ref<{ rows: number; bytes: number; path: string } | null>(null);
// PDF 生成中标志（独立于 store.auditExporting，避免两个按钮互相打架）
const pdfGenerating = ref(false);

let debounceTimer: number | null = null;
function debouncedRefresh() {
  if (debounceTimer) clearTimeout(debounceTimer);
  debounceTimer = window.setTimeout(() => {
    store.refreshAuditPreview(sanitize(filter));
  }, 300);
}

watch(
  filter,
  () => {
    debouncedRefresh();
  },
  { deep: true },
);

function sanitize(f: AuditFilter): AuditFilter {
  // 把空字符串剔除（与后端的 build_filter 语义对齐）
  const out: AuditFilter = {};
  if (f.targetTable) out.targetTable = f.targetTable;
  if (f.action) out.action = f.action;
  if (f.actor) out.actor = f.actor;
  if (f.tsFrom) out.tsFrom = f.tsFrom;
  if (f.tsTo) out.tsTo = f.tsTo;
  if (f.targetId !== undefined && f.targetId !== null && !Number.isNaN(f.targetId)) {
    out.targetId = Number(f.targetId);
  }
  return out;
}

function resetFilters() {
  filter.targetTable = "";
  filter.action = "";
  filter.actor = "";
  filter.tsFrom = "";
  filter.tsTo = "";
  filter.targetId = undefined;
  lastResult.value = null;
}

// 排序：按数量降序 → 让看板上大头朝上
const sortedActions = computed<Record<string, number>>(() => {
  const m = auditSummary.value?.byAction ?? {};
  const entries = Object.entries(m) as [string, number][];
  entries.sort((a, b) => b[1] - a[1]);
  return Object.fromEntries(entries);
});
const sortedTables = computed<Record<string, number>>(() => {
  const m = auditSummary.value?.byTable ?? {};
  const entries = Object.entries(m) as [string, number][];
  entries.sort((a, b) => b[1] - a[1]);
  return Object.fromEntries(entries);
});

function pct(n: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(2, Math.round((n / total) * 100)); // 至少 2% 宽度可见
}

// action chip 颜色 —— 与 EntityHistoryDrawer 一致
function actionKind(a: string): "create" | "update" | "delete" | "link" | "unlink" | "other" {
  if (a.endsWith("_create")) return "create";
  if (a.endsWith("_delete")) return "delete";
  if (a.endsWith("_link")) return "link";
  if (a.endsWith("_unlink")) return "unlink";
  if (a.endsWith("_update") || a.endsWith("_restore")) return "update";
  return "other";
}
function actionCN(a: string): string {
  const k = actionKind(a);
  if (k === "create") return "创建";
  if (k === "delete") return "删除";
  if (k === "link") return "+ 关联";
  if (k === "unlink") return "− 解除";
  if (k === "update") return "更新/复役";
  return a;
}
function isLinkOp(a: string): boolean {
  return a.endsWith("_link") || a.endsWith("_unlink");
}
// 动作 → 颜色类
function kindClass(a: string): string {
  const k = actionKind(a);
  if (k === "create") return "create";
  if (k === "delete") return "delete";
  if (k === "link") return "link";
  if (k === "unlink") return "unlink";
  return "update";
}
// 表 → 颜色类（仪表/sif/project/bypass_record 各一个色）
function tableClass(t: string): string {
  if (t === "instrument") return "table-instrument";
  if (t === "sif") return "table-sif";
  if (t === "project") return "table-project";
  if (t === "bypass_record") return "table-bypass";
  return "table-other";
}

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

// ===========================================================================
// 导出动作 —— 在线版：浏览器直连下载端点（带会话 Cookie），不再弹本地路径对话框
// ===========================================================================
async function onExport() {
  const total = auditSummary.value?.total ?? 0;
  if (total === 0) return;

  try {
    const { filename, bytes, truncated } = await store.downloadAuditCsv(
      sanitize(filter),
    );
    lastResult.value = { path: filename, bytes, rows: total, truncated };
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[audit-center] export failed", e);
    window.alert(
      "导出失败：" +
        ((e as { kind?: string; message?: string })?.message ?? String(e)),
    );
  }
}

// ---------------------------------------------------------------------------
// M2.6 — 导出 PDF 报告
// ---------------------------------------------------------------------------
async function onExportPdf() {
  const total = auditSummary.value?.total ?? 0;
  if (total === 0) return;

  const stamp = new Date().toISOString().slice(0, 10);
  const tblPart = filter.targetTable ? `_${filter.targetTable}` : "";
  const filename = `sif-studio-audit${tblPart}_${stamp}.pdf`;

  pdfGenerating.value = true;
  try {
    // PDF 用更大的预览（500 条），避免只印 100 条显得稀疏
    const filterSan = sanitize(filter);
    const [bigPreview, charts] = await Promise.all([
      store.fetchPdfPreview(filterSan, 500),
      store.fetchAuditCharts(filterSan),
    ]);
    // 取首个项目作为「项目名」（PDF 抬头用）—— 简化：从 auditPreview 找首个有 project_id 的，或者用占位
    const projectName = "全项目 / All Projects";
    const r = await store.exportAuditPdf({
      filter: filterSan,
      preview: bigPreview,
      summary: auditSummary.value,
      totalRows: total,
      charts,
      defaultFilename: filename,
      companyName: "上海联锁工坊科技有限公司",
      projectName,
      reportVersion: "v1.0",
      preparedBy: "",
      reviewedBy: "",
      approvedBy: "",
    });
    lastPdfResult.value = { ...r, path: filename };
  } catch (e) {
    // eslint-disable-next-line no-console
    console.error("[audit-center] PDF export failed", e);
    window.alert(
      "PDF 生成失败：" +
        ((e as { kind?: string; message?: string })?.message ?? String(e)),
    );
  } finally {
    pdfGenerating.value = false;
  }
}

// ===========================================================================
// 生命周期
// ===========================================================================
onMounted(async () => {
  store.resetAuditState();
  await Promise.all([
    store.refreshAuditFilters(),
    store.refreshAuditPreview(sanitize(filter)),
  ]);
});
</script>

<style scoped>
/* ============================================================================
 * 整体布局
 * ========================================================================== */
.audit-center {
  display: flex;
  flex-direction: column;
  gap: var(--s-3);
}

/* ============================================================================
 * ISO 7200 顶栏
 * ========================================================================== */
.head {
  display: grid;
  grid-template-columns: 1fr 280px;
  gap: var(--s-4);
  padding: 0;
}
.head-text {
  padding: var(--s-4);
  border-right: var(--rule-fine) solid var(--rule-2);
}
.head-eyebrow {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
  margin-bottom: var(--s-1);
}
.head-title {
  font-family: var(--font-title);
  font-size: var(--fs-lg);
  font-weight: 700;
  color: var(--ink-1);
  margin: 0 0 var(--s-1);
  letter-spacing: 0.02em;
}
.head-sub {
  font-size: var(--fs-xs);
  color: var(--ink-3);
}
.head-block {
  padding: var(--s-4);
  background: var(--paper-3);
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
}
.head-row {
  display: grid;
  grid-template-columns: 80px 1fr;
  gap: var(--s-2);
  align-items: baseline;
  padding: var(--s-1) 0;
  border-bottom: var(--rule-hair) solid var(--rule-2);
}
.head-row:last-child { border-bottom: 0; }
.head-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.head-val {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}

/* ============================================================================
 * 筛选表单
 * ========================================================================== */
.filter-card { padding: 0; }
.card-title {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--s-2) var(--s-4);
  border-bottom: var(--rule-mid) solid var(--ink-1);
  background: var(--paper-3);
  font-family: var(--font-title);
  font-weight: 700;
  font-size: var(--fs-sm);
  color: var(--ink-1);
  letter-spacing: 0.04em;
}
.card-title .ref {
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 400;
  color: var(--ink-3);
  letter-spacing: 0.12em;
}

.filter-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--s-3);
  padding: var(--s-4);
}
.filter-grid label {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
}
.filter-grid .lbl {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.12em;
  text-transform: uppercase;
}
.filter-grid input,
.filter-grid select {
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  padding: 4px 8px;
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  color: var(--ink-1);
  border-radius: 0;
}
.filter-grid input:focus,
.filter-grid select:focus {
  outline: none;
  border-color: var(--acc);
}

.filter-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--s-2) var(--s-4);
  background: var(--paper-2);
  border-top: var(--rule-fine) solid var(--rule-2);
}
.result-count {
  display: flex;
  align-items: baseline;
  gap: var(--s-2);
}
.rc-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.rc-val {
  font-size: var(--fs-lg);
  font-weight: 900;
  color: var(--acc);
}
.rc-unit {
  font-size: var(--fs-xs);
  color: var(--ink-3);
}
.rc-note {
  font-size: var(--fs-xs);
  color: var(--ink-3);
  margin-left: var(--s-1);
}
.filter-tip {
  font-size: var(--fs-xs);
  color: var(--ink-3);
  font-style: italic;
}

/* ============================================================================
 * 汇总统计
 * ========================================================================== */
.summary-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--s-4);
  padding: var(--s-4);
}
.sum-h {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
  margin-bottom: var(--s-2);
  padding-bottom: var(--s-1);
  border-bottom: var(--rule-fine) solid var(--rule-2);
}
.bar-list { display: flex; flex-direction: column; gap: 4px; }
.bar-row {
  display: grid;
  grid-template-columns: 130px 1fr 50px;
  gap: var(--s-2);
  align-items: center;
  font-size: var(--fs-xs);
}
.bar-key {
  color: var(--ink-2);
  font-weight: 500;
  word-break: break-all;
}
.bar-track {
  height: 10px;
  background: var(--paper-3);
  border: var(--rule-hair) solid var(--rule-2);
  position: relative;
  overflow: hidden;
}
.bar-fill {
  display: block;
  height: 100%;
  border-right: var(--rule-fine) solid var(--ink-1);
  transition: none;
}
.bar-fill.bar-create  { background: var(--acc); }
.bar-fill.bar-update  { background: var(--warn); }
.bar-fill.bar-delete  { background: var(--err); }
.bar-fill.bar-link    { background: var(--ok); }
.bar-fill.bar-unlink  { background: var(--ink-3); }
.bar-fill.bar-other   { background: var(--ink-2); }
.bar-fill.bar-table-instrument { background: var(--acc); }
.bar-fill.bar-table-sif        { background: var(--ok); }
.bar-fill.bar-table-project    { background: var(--reset); }
.bar-fill.bar-table-bypass     { background: var(--warn); }
.bar-fill.bar-table-other      { background: var(--ink-3); }
.bar-val {
  text-align: right;
  font-weight: 700;
  color: var(--ink-1);
}

/* ============================================================================
 * 预览表
 * ========================================================================== */
.preview-card { padding: 0; }
.state {
  padding: var(--s-5) var(--s-3);
  text-align: center;
  color: var(--ink-3);
  font-size: var(--fs-sm);
}
.state.empty .hint { font-size: var(--fs-xs); }

.preview-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
}
.preview-table th {
  background: var(--paper-3);
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  color: var(--ink-3);
  padding: 4px 8px;
  text-align: left;
  border-bottom: var(--rule-fine) solid var(--rule-2);
  text-transform: uppercase;
}
.preview-table td {
  padding: 6px 8px;
  border-bottom: var(--rule-hair) solid var(--rule-2);
  vertical-align: top;
}
.preview-table tr:last-child td { border-bottom: 0; }
.preview-table td.num { text-align: right; font-variant-numeric: tabular-nums; }

/* action chip（与 EntityHistoryDrawer 一致）*/
.action-chip {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 1px 6px;
  border: var(--rule-fine) solid currentColor;
  color: var(--ink-1);
}
.action-chip.create { color: var(--acc); background: rgba(0, 102, 153, 0.08); }
.action-chip.update { color: var(--warn); background: rgba(204, 102, 0, 0.08); }
.action-chip.delete { color: var(--err); background: rgba(204, 0, 0, 0.08); }
.action-chip.link   { color: var(--ok); background: rgba(0, 153, 102, 0.10); }
.action-chip.unlink { color: var(--ink-3); background: rgba(80, 80, 80, 0.08); }
.action-chip.other  { color: var(--reset); background: rgba(80, 80, 200, 0.08); }

/* 字段 chip */
.chips { display: inline-flex; flex-wrap: wrap; gap: 4px; }
.chip {
  display: inline-block;
  padding: 1px 6px;
  font-family: var(--font-mono);
  font-size: 10px;
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper-2);
  color: var(--ink-2);
}
.chip.all {
  background: var(--acc);
  color: var(--paper);
  border-color: var(--acc);
}
.chip.none {
  background: transparent;
  color: var(--ink-3);
  border-color: var(--rule-3);
  font-style: italic;
}

/* link/unlink 自然语言描述 */
.link-desc {
  font-size: var(--fs-xs);
  color: var(--ink-1);
}

/* ============================================================================
 * 导出动作条
 * ========================================================================== */
.export-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--s-4);
  padding: var(--s-3) var(--s-4);
  background: var(--paper);
  border: var(--rule-mid) solid var(--ink-1);
}
.export-h {
  font-family: var(--font-title);
  font-weight: 700;
  font-size: var(--fs-sm);
  color: var(--ink-1);
}
.export-sub {
  font-size: var(--fs-xs);
  color: var(--ink-3);
  margin-top: 2px;
}
.export-locked {
  border-style: dashed;
  opacity: 0.85;
}
.export-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: var(--s-2);
}
/* 两按钮并列：CSV 在上，PDF 在下 */
.export-actions > .primary {
  align-self: stretch;
  min-width: 160px;
}
.export-actions > .primary.pdf {
  background: var(--paper);
  color: var(--ink-1);
  border: 1.5px solid var(--ink-1);
}
.export-actions > .primary.pdf:not(:disabled):hover {
  background: var(--ink-1);
  color: var(--paper);
}
.last-result {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ok);
  word-break: break-all;
  text-align: right;
  max-width: 480px;
}
.last-result.pdf {
  color: var(--ink-2);
  border-left: 2px solid var(--ink-2);
  padding-left: 6px;
}
.ok-mark {
  font-weight: 900;
  color: var(--ok);
  margin-right: 4px;
}

/* ============================================================================
 * 按钮（沿用全局 .primary .sm）
 * ========================================================================== */
button {
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-sm);
  cursor: pointer;
  border-radius: 0;
}
button.sm {
  font-size: var(--fs-xs);
  padding: 2px 8px;
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  color: var(--ink-2);
}
button.sm:hover { background: var(--paper-2); color: var(--ink-1); }
button.primary {
  padding: 6px 18px;
  border: var(--rule-mid) solid var(--ink-1);
  background: var(--ink-1);
  color: var(--paper);
  font-weight: 700;
  letter-spacing: 0.04em;
}
button.primary:hover { background: #000; }
button.primary:disabled {
  background: var(--paper-3);
  color: var(--ink-3);
  border-color: var(--rule-2);
  cursor: not-allowed;
}
</style>