<script setup lang="ts">
/**
 * ImportPanel — M2.1 仪表批量导入三步向导（双模式面板）
 *
 * - 无 projectId prop：全局模式（/import 页面，STEP 3 手动选目标项目）
 * - 有 projectId：项目锁定模式（项目详情页 Tab，目标项目恒为该项目）
 *
 * 步骤：
 *   1. UPLOAD  — 选文件（CSV/TSV/XLSX），调 preview_import_instruments
 *   2. MAPPING — 显示预览表 + 每列下拉改映射
 *   3. COMMIT  — 选去重策略 + 看统计 + 提交 → 看结果
 *
 * UI 规范：工业图纸风（参见 docs/STYLE-GUIDE.md）
 *   - 顶部 ISO 7200 标题栏
 *   - 1px 黑边、无圆角、等宽数字、ISA 5.1 风格方块徽章
 */
import { ref, computed, onMounted, watch, onUnmounted } from "vue";
import { useRouter } from "vue-router";
import { useStudioStore, IMPORT_TARGETS } from "../stores/studio";
import type { ImportStrategy, ImportTarget } from "../stores/studio";

const props = defineProps<{ projectId?: number }>();
const locked = computed(() => props.projectId != null);

const router = useRouter();
const store = useStudioStore();

// 三步状态
const STEP = { UPLOAD: 1, MAPPING: 2, COMMIT: 3 } as const;
const step = ref<1 | 2 | 3>(STEP.UPLOAD);

// 文件选择
const fileInputRef = ref<HTMLInputElement | null>(null);
const selectedFile = ref<File | null>(null);

// 错误展示
const errorMessage = ref<string>("");

// ---------------------------------------------------------------------------
// 在线版：统一 <input type=file> + 拖拽，multipart 上传给 /api/imports/preview
// ---------------------------------------------------------------------------
async function handleFile(file: File) {
  errorMessage.value = "";
  selectedFile.value = file;
  try {
    await store.previewImport(file);
    step.value = STEP.MAPPING;
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    errorMessage.value = err.message ?? String(e);
  }
}

function onFileChange(ev: Event) {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  if (file) void handleFile(file);
}

function onDrop(ev: DragEvent) {
  ev.preventDefault();
  const file = ev.dataTransfer?.files?.[0];
  if (file) void handleFile(file);
}

function onDragOver(ev: DragEvent) {
  ev.preventDefault();
}

// 弹窗关闭时重置
function reset() {
  store.resetImport();
  // 锁定模式：目标项目被 resetImport 清掉了，立刻补回当前项目
  if (props.projectId != null) {
    store.setImportTargetProjectId(props.projectId);
  }
  selectedFile.value = null;
  doneResult.value = null;
  step.value = STEP.UPLOAD;
  errorMessage.value = "";
  if (fileInputRef.value) fileInputRef.value.value = "";
}

// 步骤 2：调整映射
const availableTargets: ImportTarget[] = [
  "tag",
  "service",
  "kind",
  "role",
  "manufacturer",
  "model",
  "psv_id",
  "range_min",
  "range_max",
  "unit",
  "setpoint",
  "sil_target",
  "proof_interval",
  "installed_at",
  "notes",
];

function targetForColumn(col: number): ImportTarget | "" {
  const m = store.importMapping.find((x) => x.column === col);
  return m?.target ?? "";
}

function setMapping(col: number, value: string) {
  store.setMappingColumnTarget(col, value as ImportTarget | "");
}

const tagMapped = computed(() =>
  store.importMapping.some((m) => m.target === "tag"),
);

// 步骤 3：策略 + 提交
const strategies: { value: ImportStrategy; label: string; help: string }[] = [
  { value: "skip", label: "SKIP", help: "tag 已存在则跳过" },
  { value: "overwrite", label: "OVERWRITE", help: "tag 已存在则覆盖" },
  { value: "create_with_suffix", label: "SUFFIX", help: "tag 重复时改名 _N" },
];

const submitting = ref(false);
const doneResult = ref<typeof store.importResult>(null);

async function submit() {
  if (!tagMapped.value || submitting.value) return;
  // M2.9 — 必须指定目标项目（锁定模式由详情页预置）
  if (!store.importTargetProjectId) {
    errorMessage.value = "请先选择导入目标项目（仪表台账按项目隔离）";
    return;
  }
  submitting.value = true;
  errorMessage.value = "";
  try {
    doneResult.value = await store.commitImport();
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    errorMessage.value = err.message ?? String(e);
  } finally {
    submitting.value = false;
  }
}

function back() {
  if (step.value === STEP.COMMIT) step.value = STEP.MAPPING;
  else if (step.value === STEP.MAPPING) step.value = STEP.UPLOAD;
  else router.push("/instruments");
}

function next() {
  if (step.value === STEP.UPLOAD && store.importSheet) {
    step.value = STEP.MAPPING;
  } else if (step.value === STEP.MAPPING && tagMapped.value) {
    step.value = STEP.COMMIT;
  }
}

const sheet = computed(() => store.importSheet);
const showPreview = computed(() => !!sheet.value);

// 路由变化时若离开页面则清状态（仅全局模式；锁定模式由详情页管理）
onMounted(() => {
  if (!sheet.value) step.value = STEP.UPLOAD;
});

if (props.projectId == null) {
  watch(
    () => router.currentRoute.value.fullPath,
    (to, from) => {
      if (to !== from && !to.endsWith("/import")) {
        store.resetImport();
      }
    },
  );
} else {
  onUnmounted(() => {
    // 离开项目详情页时清掉目标项目，避免泄漏到全局 /import
    store.setImportTargetProjectId(null);
  });
}
</script>

<template>
  <div class="iw">
    <!-- 顶部 ISO 7200 标题栏 -->
    <header class="iw-head">
      <div class="head-cell head-title">
        <span class="head-k">TITLE</span>
        <span class="head-v">INSTRUMENT IMPORT WIZARD · M2.1</span>
      </div>
      <div class="head-cell head-meta">
        <span class="head-k">SOURCE</span>
        <span class="head-v">{{ sheet?.sourceName || "—" }}</span>
      </div>
      <div class="head-cell head-meta">
        <span class="head-k">ROWS</span>
        <span class="head-v">{{ sheet?.totalRows ?? "—" }}</span>
      </div>
      <div class="head-cell head-status">
        <span class="head-k">STATE</span>
        <span class="head-v" :class="`state-${step}`">{{ step }}/3</span>
      </div>
    </header>

    <!-- 三步指示条 -->
    <nav class="iw-steps">
      <div class="step" :class="{ active: step === 1, done: step > 1 }">
        <span class="step-no">01</span>
        <span class="step-label">UPLOAD</span>
      </div>
      <div class="step-arrow">▶</div>
      <div class="step" :class="{ active: step === 2, done: step > 2 }">
        <span class="step-no">02</span>
        <span class="step-label">MAPPING</span>
      </div>
      <div class="step-arrow">▶</div>
      <div class="step" :class="{ active: step === 3 }">
        <span class="step-no">03</span>
        <span class="step-label">COMMIT</span>
      </div>
    </nav>

    <!-- 错误横幅（贯穿所有步骤） -->
    <div v-if="errorMessage" class="iw-error">
      <span class="err-tag">ERR</span>
      <span class="err-msg">{{ errorMessage }}</span>
      <button class="btn-close" @click="errorMessage = ''">×</button>
    </div>

    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <!-- STEP 1: UPLOAD -->
    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <section v-if="step === 1" class="iw-body">
      <div
        class="dropzone"
        :class="{ active: selectedFile }"
        @drop="onDrop"
        @dragover="onDragOver"
        @click="fileInputRef?.click()"
      >
        <input
          ref="fileInputRef"
          type="file"
          accept=".csv,.tsv,.xlsx,.xls,.xlsb,.xlsm"
          @change="onFileChange"
          hidden
        />
        <div class="drop-icon">[ FILE ]</div>
        <div class="drop-title">
          {{ selectedFile ? selectedFile.name : "选择或拖入 CSV / TSV / XLSX" }}
        </div>
        <div class="drop-meta">
          <span v-if="selectedFile">
            {{ (selectedFile.size / 1024).toFixed(1) }} KB · {{ selectedFile.type || "unknown" }}
          </span>
          <span v-else>
            支持格式：CSV · TSV · XLSX · XLS · XLSB · XLSM
          </span>
        </div>
      </div>
      <div class="upload-hint">
        <p><strong>建议：</strong></p>
        <ul>
          <li>列名含「位号 / TAG / P&ID」等关键词会自动映射</li>
          <li>数字列（range_min / range_max / setpoint）会被自动识别</li>
          <li>中文表头也支持（UTF-8 / GBK 自动探测）</li>
        </ul>
      </div>
    </section>

    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <!-- STEP 2: MAPPING -->
    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <section v-else-if="step === 2 && showPreview" class="iw-body">
      <div class="mapping-summary">
        <div class="ms-item">
          <span class="ms-k">COLS</span>
          <span class="ms-v">{{ sheet!.headers.length }}</span>
        </div>
        <div class="ms-item">
          <span class="ms-k">ROWS</span>
          <span class="ms-v">{{ sheet!.rows.length }}</span>
        </div>
        <div class="ms-item">
          <span class="ms-k">FMT</span>
          <span class="ms-v">{{ sheet!.format.toUpperCase() }}</span>
        </div>
        <div class="ms-item">
          <span class="ms-k">MAP</span>
          <span class="ms-v" :class="{ warn: !tagMapped }">
            {{ store.importMapping.length }}{{ tagMapped ? "" : " (缺 tag)" }}
          </span>
        </div>
      </div>

      <div class="mapping-table-wrap">
        <table class="mapping-table">
          <thead>
            <tr>
              <th class="th-no">#</th>
              <th class="th-name">COLUMN NAME</th>
              <th class="th-kind">KIND</th>
              <th class="th-samples">SAMPLE VALUES</th>
              <th class="th-target">MAP TO →</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="h in sheet!.headers" :key="h.index" :class="{ unmapped: !targetForColumn(h.index) }">
              <td class="t-no">{{ h.index + 1 }}</td>
              <td class="t-name">{{ h.name || `(列 ${h.index + 1})` }}</td>
              <td class="t-kind">
                <span class="kind-tag" :class="`kind-${h.kind}`">{{ h.kind }}</span>
              </td>
              <td class="t-samples">
                <code>{{ h.samples.join(' / ') || '—' }}</code>
              </td>
              <td class="t-target">
                <select
                  :value="targetForColumn(h.index)"
                  @change="setMapping(h.index, ($event.target as HTMLSelectElement).value)"
                >
                  <option value="">— 不映射 —</option>
                  <option v-for="t in availableTargets" :key="t" :value="t">{{ t }}</option>
                </select>
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- 预览前 5 行 -->
      <details v-if="sheet!.previewRows.length > 0" class="preview-data">
        <summary>DATA PREVIEW (前 5 行 / 共 {{ sheet!.rows.length }} 行)</summary>
        <table class="preview-table">
          <thead>
            <tr>
              <th v-for="h in sheet!.headers" :key="`ph-${h.index}`">{{ h.name }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, ri) in sheet!.previewRows.slice(0, 5)" :key="`pr-${ri}`">
              <td v-for="(cell, ci) in row" :key="`pc-${ri}-${ci}`">{{ cell }}</td>
            </tr>
          </tbody>
        </table>
      </details>
    </section>

    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <!-- STEP 3: COMMIT -->
    <!-- ═══════════════════════════════════════════════════════════════════ -->
    <section v-else-if="step === 3" class="iw-body">
      <!-- 未提交 -->
      <template v-if="!doneResult">
        <!-- M2.9 — 目标项目选择器（仪表按项目隔离；锁定模式恒为当前项目） -->
        <div class="commit-config">
          <div class="cc-k">目标项目 · TARGET PROJECT <span class="req">*</span></div>
          <select
            class="cc-project-select"
            :value="store.importTargetProjectId ?? 0"
            :disabled="locked"
            @change="(ev) => store.setImportTargetProjectId(Number((ev.target as HTMLSelectElement).value) || null)"
          >
            <option :value="0" disabled>— 请选择项目 —</option>
            <option v-for="p in store.projects" :key="p.id" :value="p.id">
              {{ p.code }} · {{ p.name }}
            </option>
          </select>
          <div v-if="!locked && !store.importTargetProjectId" class="cc-hint warn">
            ⚠ 仪表台账按项目隔离，必须先选择导入目标项目
          </div>
        </div>

        <div class="commit-config">
          <div class="cc-k">CONFLICT STRATEGY（tag 已存在时）</div>
          <div class="cc-options">
            <label v-for="s in strategies" :key="s.value" class="strategy-opt">
              <input
                type="radio"
                name="strategy"
                :value="s.value"
                :checked="store.importStrategy === s.value"
                @change="store.setImportStrategy(s.value)"
              />
              <span class="opt-label">{{ s.label }}</span>
              <span class="opt-help">{{ s.help }}</span>
            </label>
          </div>
        </div>

        <div class="commit-summary">
          <div class="cs-item">
            <span class="cs-k">FILE</span>
            <span class="cs-v">{{ sheet!.sourceName }}</span>
          </div>
          <div class="cs-item">
            <span class="cs-k">ROWS</span>
            <span class="cs-v">{{ sheet!.rows.length }}</span>
          </div>
          <div class="cs-item">
            <span class="cs-k">MAPPED</span>
            <span class="cs-v">{{ store.importMapping.length }}</span>
          </div>
          <div class="cs-item">
            <span class="cs-k">STRATEGY</span>
            <span class="cs-v">{{ store.importStrategy.toUpperCase() }}</span>
          </div>
        </div>

        <div v-if="!tagMapped" class="iw-warn">
          必须映射 <code>tag</code> 列才能导入
        </div>
      </template>

      <!-- 已提交 -->
      <template v-else>
        <div class="result-grid">
          <div class="rg-card ok">
            <span class="rg-k">INSERTED</span>
            <span class="rg-v">{{ doneResult.inserted }}</span>
          </div>
          <div class="rg-card upd">
            <span class="rg-k">UPDATED</span>
            <span class="rg-v">{{ doneResult.updated }}</span>
          </div>
          <div class="rg-card skip">
            <span class="rg-k">SKIPPED</span>
            <span class="rg-v">{{ doneResult.skipped }}</span>
          </div>
          <div class="rg-card" :class="{ err: doneResult.failed > 0 }">
            <span class="rg-k">FAILED</span>
            <span class="rg-v">{{ doneResult.failed }}</span>
          </div>
        </div>

        <div class="result-meta">
          <div><span class="rm-k">BATCH</span><span class="rm-v">#{{ doneResult.batchId }}</span></div>
          <div><span class="rm-k">SOURCE</span><span class="rm-v">{{ sheet!.sourceName }}</span></div>
        </div>

        <div v-if="doneResult.errors.length > 0" class="result-errors">
          <div class="re-k">失败明细 ({{ doneResult.errors.length }})</div>
          <table class="err-table">
            <thead>
              <tr><th>ROW</th><th>REASON</th></tr>
            </thead>
            <tbody>
              <tr v-for="(e, i) in doneResult.errors.slice(0, 50)" :key="`err-${i}`">
                <td class="e-row">{{ e.row }}</td>
                <td class="e-reason">{{ e.reason }}</td>
              </tr>
            </tbody>
          </table>
          <div v-if="doneResult.errors.length > 50" class="err-overflow">
            + {{ doneResult.errors.length - 50 }} more（控制台可见）
          </div>
        </div>
      </template>
    </section>

    <!-- 底部按钮条 -->
    <footer class="iw-foot">
      <button class="btn btn-ghost" @click="back" :disabled="step === 1 && !sheet">
        ← 返回
      </button>
      <div class="foot-spacer"></div>
      <button v-if="step === 3 && doneResult" class="btn btn-ghost" @click="reset">
        再导一批
      </button>
      <button v-if="step === 3 && doneResult" class="btn" @click="router.push('/instruments')">
        → 仪表台账
      </button>
      <button v-if="step === 2" class="btn" @click="next" :disabled="!tagMapped">
        下一步：提交 →
      </button>
      <button v-if="step === 3 && !doneResult" class="btn btn-accent" :disabled="!tagMapped || submitting" @click="submit">
        {{ submitting ? "处理中…" : "提交导入" }}
      </button>
    </footer>
  </div>
</template>

<style scoped>
/* ══════════════════════════════════════════════════════════════════
 * ImportPanel — 工业图纸风（docs/STYLE-GUIDE.md）
 * 零圆角 / 零阴影 / 零渐变 / 1px 黑边 / 等宽数字
 * ══════════════════════════════════════════════════════════════════ */

.iw {
  display: flex;
  flex-direction: column;
  gap: 0;
  min-height: 100%;
  background: var(--paper-2);
}

/* ─── 顶部 ISO 7200 标题栏 ─── */
.iw-head {
  display: grid;
  grid-template-columns: 2.4fr 1.4fr 0.7fr 0.7fr;
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  border-bottom: var(--rule-mid) solid var(--rule);
}
.head-cell {
  padding: var(--s-2) var(--s-3);
  border-right: var(--rule-fine) solid var(--rule-2);
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.head-cell:last-child { border-right: none; }
.head-k {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--ink-3);
}
.head-v {
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-sm);
  color: var(--ink-1);
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.head-title .head-v { font-size: var(--fs-md); letter-spacing: 0.02em; }
.state-1 { color: var(--acc); }
.state-2 { color: var(--warn); }
.state-3 { color: var(--ok); }

/* ─── 步骤指示条 ─── */
.iw-steps {
  display: flex;
  align-items: stretch;
  gap: 0;
  border: var(--rule-fine) solid var(--rule);
  border-top: none;
  background: var(--paper);
}
.step {
  flex: 1;
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  border-right: var(--rule-fine) solid var(--rule-2);
  color: var(--ink-4);
}
.step:last-child { border-right: none; }
.step-no {
  font-family: var(--font-mono);
  font-size: var(--fs-md);
  font-weight: 700;
  border: var(--rule-fine) solid currentColor;
  padding: 1px 6px;
  min-width: 30px;
  text-align: center;
}
.step-label {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
}
.step.active { background: var(--paper); color: var(--ink-1); }
.step.active .step-no { background: var(--ink-1); color: var(--paper); border-color: var(--ink-1); }
.step.done { color: var(--ok); }
.step.done .step-no { background: var(--ok); color: var(--paper); border-color: var(--ok); }
.step-arrow {
  display: flex;
  align-items: center;
  padding: 0 4px;
  color: var(--rule-2);
  font-size: 10px;
  background: var(--paper);
}

/* ─── 错误横幅 ─── */
.iw-error {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  background: var(--err-bg);
  border: var(--rule-fine) solid var(--err);
  border-top: none;
}
.err-tag {
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.16em;
  background: var(--err);
  color: #fff;
  padding: 2px 6px;
}
.err-msg { flex: 1; color: var(--err); font-size: var(--fs-sm); }
.btn-close {
  border: var(--rule-fine) solid var(--err);
  background: transparent;
  color: var(--err);
  font-size: 14px;
  line-height: 1;
  padding: 2px 7px;
  cursor: pointer;
}
.btn-close:hover { background: var(--err); color: #fff; }

/* ─── 主体 ─── */
.iw-body {
  flex: 1;
  padding: var(--s-4);
  display: flex;
  flex-direction: column;
  gap: var(--s-3);
  background: var(--paper-2);
}

/* ─── STEP 1: 拖拽区 ─── */
.dropzone {
  border: var(--rule-mid) dashed var(--rule-2);
  background: var(--paper);
  padding: var(--s-6) var(--s-4);
  text-align: center;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
  align-items: center;
  transition: border-color 0.12s, background 0.12s;
}
.dropzone:hover,
.dropzone.active {
  border-color: var(--acc);
  border-style: solid;
  background: var(--acc-bg);
}
.drop-icon {
  font-family: var(--font-mono);
  font-size: var(--fs-lg);
  font-weight: 700;
  letter-spacing: 0.08em;
  color: var(--ink-3);
}
.dropzone.active .drop-icon { color: var(--acc); }
.drop-title {
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--ink-1);
}
.drop-meta {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ink-3);
}

.upload-hint {
  border: var(--rule-fine) solid var(--rule-2);
  border-left: var(--rule-mid) solid var(--ink-1);
  background: var(--paper);
  padding: var(--s-3);
  font-size: var(--fs-sm);
  color: var(--ink-2);
}
.upload-hint p { margin: 0 0 var(--s-1); }
.upload-hint ul { margin: 0; padding-left: 1.2em; }
.upload-hint li { line-height: 1.7; }

/* ─── STEP 2: 映射 ─── */
.mapping-summary {
  display: flex;
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
}
.ms-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--s-2) var(--s-3);
  border-right: var(--rule-fine) solid var(--rule-2);
}
.ms-item:last-child { border-right: none; }
.ms-k {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--ink-3);
}
.ms-v {
  font-family: var(--font-mono);
  font-size: var(--fs-lg);
  font-weight: 700;
  color: var(--ink-1);
  font-variant-numeric: tabular-nums;
}
.ms-v.warn { color: var(--fin); }

.mapping-table-wrap {
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  overflow: auto;
  max-height: 46vh;
}
.mapping-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-sm);
}
.mapping-table th {
  background: var(--paper-3);
  color: var(--ink-2);
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  text-align: left;
  padding: var(--s-1) var(--s-2);
  border: var(--rule-fine) solid var(--rule-2);
  border-top: none;
  position: sticky;
  top: 0;
  z-index: 1;
}
.mapping-table th:first-child { border-left: none; }
.mapping-table th:last-child  { border-right: none; }
.mapping-table td {
  padding: var(--s-1) var(--s-2);
  border: var(--rule-hair) solid var(--rule-3);
  vertical-align: middle;
}
.mapping-table tr.unmapped td { background: #fdfaf5; }
.th-no, .t-no { width: 40px; }
.th-kind, .t-kind { width: 90px; }
.th-target, .t-target { width: 180px; }
.t-no {
  font-family: var(--font-mono);
  color: var(--ink-4);
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.t-name {
  font-family: var(--font-mono);
  font-weight: 600;
  color: var(--ink-1);
}
.t-samples code {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ink-3);
}
.t-target select {
  width: 100%;
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
}

.kind-tag {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.1em;
  text-transform: uppercase;
  padding: 1px 5px;
  border: var(--rule-fine) solid var(--rule-2);
  color: var(--ink-3);
  background: var(--paper-3);
}
.kind-number { color: var(--acc); border-color: var(--acc); background: var(--acc-bg); }
.kind-text   { color: var(--ink-2); }
.kind-mixed  { color: var(--warn); border-color: var(--warn); background: var(--warn-bg); }
.kind-empty  { color: var(--ink-4); border-style: dashed; }

.preview-data {
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  padding: var(--s-2) var(--s-3);
}
.preview-data summary {
  cursor: pointer;
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  color: var(--ink-2);
}
.preview-table {
  width: 100%;
  border-collapse: collapse;
  margin-top: var(--s-2);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
}
.preview-table th, .preview-table td {
  border: var(--rule-hair) solid var(--rule-3);
  padding: 3px var(--s-2);
  text-align: left;
  white-space: nowrap;
}
.preview-table th { background: var(--paper-3); color: var(--ink-2); }

/* ─── STEP 3: 提交 ─── */
.commit-config {
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  padding: var(--s-3);
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
}
.cc-k {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--ink-3);
}
.cc-options { display: flex; flex-direction: column; gap: var(--s-1); }

/* M2.9 — 项目选择器 */
.cc-project-select {
  width: 100%;
  padding: 6px 10px;
  font-family: var(--font-mono);
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
}
.cc-hint.warn {
  font-size: var(--fs-micro);
  color: var(--warn);
  margin-top: 4px;
}
.req { color: var(--err); font-weight: 700; margin-left: 2px; }

.strategy-opt {
  display: grid;
  grid-template-columns: 20px 110px 1fr;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-1) var(--s-2);
  border: var(--rule-fine) solid var(--rule-3);
  cursor: pointer;
}
.strategy-opt:hover { background: var(--paper-2); }
.strategy-opt input[type="radio"] { accent-color: var(--acc); }
.opt-label {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  font-weight: 700;
  letter-spacing: 0.1em;
}
.opt-help { font-size: var(--fs-xs); color: var(--ink-3); }

.commit-summary {
  display: flex;
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
}
.cs-item {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: var(--s-2) var(--s-3);
  border-right: var(--rule-fine) solid var(--rule-2);
}
.cs-item:last-child { border-right: none; }
.cs-k {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--ink-3);
}
.cs-v {
  font-family: var(--font-mono);
  font-size: var(--fs-md);
  font-weight: 600;
  color: var(--ink-1);
}

.iw-warn {
  border: var(--rule-fine) solid var(--warn);
  background: var(--warn-bg);
  color: var(--warn);
  padding: var(--s-2) var(--s-3);
  font-size: var(--fs-sm);
}

/* ─── 结果 ─── */
.result-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
}
.rg-card {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
  padding: var(--s-3);
  border-right: var(--rule-fine) solid var(--rule-2);
}
.rg-card:last-child { border-right: none; }
.rg-k {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.18em;
  text-transform: uppercase;
  color: var(--ink-3);
}
.rg-v {
  font-family: var(--font-mono);
  font-size: var(--fs-huge);
  font-weight: 700;
  line-height: 1;
  color: var(--ink-2);
  font-variant-numeric: tabular-nums;
}
.rg-card.ok  .rg-v { color: var(--ok); }
.rg-card.upd .rg-v { color: var(--acc); }
.rg-card.skip .rg-v { color: var(--ink-3); }
.rg-card.err { background: var(--err-bg); }
.rg-card.err .rg-v { color: var(--err); }

.result-meta {
  display: flex;
  gap: var(--s-4);
  padding: var(--s-2) var(--s-3);
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
}
.result-meta > div { display: flex; gap: var(--s-1); align-items: baseline; }
.rm-k { color: var(--ink-3); letter-spacing: 0.14em; }
.rm-v { color: var(--ink-1); font-weight: 600; }

.result-errors {
  border: var(--rule-fine) solid var(--err);
  background: var(--paper);
}
.re-k {
  background: var(--err-bg);
  color: var(--err);
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  padding: var(--s-1) var(--s-3);
  border-bottom: var(--rule-fine) solid var(--err);
}
.err-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
}
.err-table th {
  background: var(--paper-3);
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  text-transform: uppercase;
  text-align: left;
  padding: var(--s-1) var(--s-3);
  border-bottom: var(--rule-hair) solid var(--rule-3);
  color: var(--ink-2);
}
.err-table td {
  padding: var(--s-1) var(--s-3);
  border-bottom: var(--rule-hair) solid var(--rule-3);
}
.e-row {
  font-family: var(--font-mono);
  color: var(--err);
  width: 60px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.e-reason { color: var(--ink-2); font-family: var(--font-mono); }
.err-overflow {
  padding: var(--s-1) var(--s-3);
  font-size: var(--fs-xs);
  color: var(--ink-3);
  border-top: var(--rule-hair) solid var(--rule-3);
}

/* ─── 底部按钮条 ─── */
.iw-foot {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  border: var(--rule-fine) solid var(--rule);
  border-top: var(--rule-mid) solid var(--rule);
  background: var(--paper);
}
.foot-spacer { flex: 1; }
.btn {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  letter-spacing: 0.08em;
  padding: var(--s-1) var(--s-3);
  border: var(--rule-fine) solid var(--ink-1);
  background: var(--paper);
  color: var(--ink-1);
  cursor: pointer;
}
.btn:hover:not(:disabled) { background: var(--ink-1); color: var(--paper); }
.btn:disabled { opacity: 0.4; cursor: not-allowed; }
.btn-ghost { border-color: var(--rule-2); color: var(--ink-2); }
.btn-ghost:hover { background: var(--paper-3); color: var(--ink-1); border-color: var(--ink-1); }
.btn-accent {
  background: var(--ink-1);
  color: var(--paper);
  border-color: var(--ink-1);
}
.btn-accent:hover:not(:disabled) { background: var(--acc); border-color: var(--acc); }
</style>
