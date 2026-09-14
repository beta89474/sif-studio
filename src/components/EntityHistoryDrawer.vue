<!--
  EntityHistoryDrawer.vue —— M2.4 通用修改历史抽屉

  显示某实体（instrument / sif / project，由 prop 决定）的全部修改记录
    - 时间线（最新在最上）
    - 每条记录：时间 + 操作人 + 动作 chip（创建/更新/删除/+关联/−解除）
    - 字段级 diff 表格：字段名 → 旧值 → 新值（仅展示变了的字段）
    - 对 sif_link / sif_unlink 这种非字段级变更 → 显示 description 自然语言

  父组件用法：
    <EntityHistoryDrawer entity-type="instrument" />
    <EntityHistoryDrawer entity-type="sif" />
    <EntityHistoryDrawer entity-type="project" />
    store.openEntityHistory(kind, id) 触发显示；store.closeEntityHistory() 关闭
-->
<template>
  <Transition name="drawer">
    <aside v-if="open" class="drawer" role="dialog" :aria-label="`${label}修改历史`">
      <header class="head">
        <div class="head-text">
          <div class="head-eyebrow">SECTION · M2.{{ isInstrument ? "3" : "4" }} AUDIT TRAIL</div>
          <div class="head-title">
            <span class="tag mono">{{ displayName || `ID-${targetId}` }}</span>
            <span class="title">{{ label }}修改历史</span>
          </div>
          <div class="head-sub">
            共 {{ store.entityHistory.length }} 条 · 最近 {{ latestTs }}
          </div>
        </div>
        <button class="close" @click="store.closeEntityHistory" aria-label="关闭">×</button>
      </header>

      <div class="body">
        <div v-if="store.entityHistoryLoading" class="state">载入中…</div>
        <div v-else-if="!store.entityHistory.length" class="state empty">
          暂无修改记录。<br />
          <span class="hint">只有 M2.3/4 起（含本版本）的修改会被写入审计。</span>
        </div>
        <ol v-else class="timeline">
          <li v-for="entry in store.entityHistory" :key="entry.id" class="entry">
            <div class="rail">
              <span class="dot" :class="actionKind(entry.action)"></span>
            </div>
            <div class="card">
              <header class="card-head">
                <span class="action-chip" :class="actionKind(entry.action)">
                  {{ actionCN(entry.action) }}
                </span>
                <span class="ts mono">{{ entry.ts }}</span>
                <span class="actor">by {{ entry.actor || "—" }}</span>
              </header>

              <!-- 关联操作（sif_link / sif_unlink）：显示自然语言 description -->
              <div v-if="isLinkOp(entry.action)" class="link-op">
                <span class="link-op-label">{{ linkOpCN(entry.action) }}</span>
                <span class="link-op-desc">{{ entry.note || "—" }}</span>
              </div>

              <!-- 变更字段 chips -->
              <div v-else class="fields-changed">
                <span class="label">变更字段</span>
                <span v-if="entry.fieldsChanged.length === 1 && entry.fieldsChanged[0] === '*'"
                      class="chip all">全字段</span>
                <span v-else-if="entry.fieldsChanged.length === 0" class="chip none">
                  无实质变更（仅保存动作）
                </span>
                <span v-else class="chips">
                  <span v-for="f in entry.fieldsChanged" :key="f" class="chip">{{ fieldLabel(f) }}</span>
                </span>
              </div>

              <!-- diff 表（仅 update + 非空 + 非 ["*"]）-->
              <div v-if="showDiffTable(entry)" class="diff-table-wrap">
                <table class="diff-table">
                  <thead>
                    <tr>
                      <th>字段 · FIELD</th>
                      <th>修改前 · BEFORE</th>
                      <th>修改后 · AFTER</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr v-for="field in entry.fieldsChanged" :key="field">
                      <td class="field-name">{{ fieldLabel(field) }}</td>
                      <td class="old">{{ fmtValue(entry.before?.[field]) }}</td>
                      <td class="new">{{ fmtValue(entry.after?.[field]) }}</td>
                    </tr>
                  </tbody>
                </table>
              </div>

              <!-- create/delete 全字段概览（非关联操作）-->
              <div v-else-if="!isLinkOp(entry.action) && entry.action.includes('_create')" class="snapshot">
                <div class="snap-label">完整字段（新行）</div>
                <div class="kv">
                  <template v-for="(v, k) in entry.after" :key="k">
                    <span class="kv-key mono">{{ k }}</span>
                    <span class="kv-val">{{ fmtValue(v) }}</span>
                  </template>
                </div>
              </div>
              <div v-else-if="!isLinkOp(entry.action) && entry.action.includes('_delete')" class="snapshot">
                <div class="snap-label">完整字段（已删除）</div>
                <div class="kv">
                  <template v-for="(v, k) in entry.before" :key="k">
                    <span class="kv-key mono">{{ k }}</span>
                    <span class="kv-val">{{ fmtValue(v) }}</span>
                  </template>
                </div>
              </div>
            </div>
          </li>
        </ol>
      </div>
    </aside>
  </Transition>
  <Transition name="mask">
    <div v-if="open" class="mask" @click="store.closeEntityHistory" />
  </Transition>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useStudioStore } from "../stores/studio";

const props = defineProps<{ entityType: "instrument" | "sif" | "project" }>();

const store = useStudioStore();
const isInstrument = computed(() => props.entityType === "instrument");
const label = computed(() =>
  props.entityType === "instrument" ? "仪表"
    : props.entityType === "sif" ? "SIF"
    : "项目",
);

const targetId = computed(() => store.entityHistoryFor?.id ?? 0);
const displayName = computed(() => {
  const id = store.entityHistoryFor?.id;
  if (!id) return "";
  if (props.entityType === "instrument") {
    return store.instruments.find((i) => i.id === id)?.tag ?? "";
  }
  if (props.entityType === "sif") {
    return store.sifSummary.find((s) => s.id === id)?.code ?? "";
  }
  if (props.entityType === "project") {
    return store.projects.find((p) => p.id === id)?.code ?? "";
  }
  return "";
});

const open = computed(() => store.entityHistoryFor?.kind === props.entityType);
const latestTs = computed(() => store.entityHistory[0]?.ts ?? "—");

// --------------------------------------------------------------------------
// 字段名 → 中文标签（按 entityType 三套）
// --------------------------------------------------------------------------
const INSTRUMENT_LABELS: Record<string, string> = {
  tag: "位号", service: "服务描述", kind: "类型", role: "角色",
  psvId: "PSV 编号", manufacturer: "厂家", model: "型号",
  rangeMin: "量程下限", rangeMax: "量程上限", unit: "单位",
  setpoint: "设定值", silTarget: "目标 SIL", proofInterval: "检验周期",
  installedAt: "安装日期", notes: "备注",
};

const SIF_LABELS: Record<string, string> = {
  code: "编号", name: "名称", description: "说明",
  projectId: "项目 ID", silDesign: "设计 SIL", silVerified: "验证 SIL",
  demandMode: "需求模式", pfdavgTarget: "PFDavg 目标",
  proofInterval: "检验周期",
};

const PROJECT_LABELS: Record<string, string> = {
  code: "编号", name: "名称", client: "业主",
  location: "位置", phase: "阶段",
  finishedAt: "结束日期", notes: "备注",
};

const ROLE_CN: Record<string, string> = {
  detector: "检测", final: "最终", logic: "逻辑", aux: "旁路",
};

function fieldLabel(k: string): string {
  const table = isInstrument.value
    ? INSTRUMENT_LABELS
    : props.entityType === "sif" ? SIF_LABELS : PROJECT_LABELS;
  return table[k] ?? k;
}

// --------------------------------------------------------------------------
// 通用 helpers
// --------------------------------------------------------------------------
function fmtValue(v: unknown): string {
  if (v == null) return "—";
  if (typeof v === "string") return v === "" ? "（空）" : v;
  if (typeof v === "number") return v.toString();
  if (typeof v === "boolean") return v ? "是" : "否";
  return JSON.stringify(v);
}

function actionKind(a: string): "create" | "update" | "delete" | "link" | "unlink" {
  if (a.endsWith("_create")) return "create";
  if (a.endsWith("_delete")) return "delete";
  if (a.endsWith("_link")) return "link";
  if (a.endsWith("_unlink")) return "unlink";
  return "update";
}

function actionCN(a: string): string {
  const k = actionKind(a);
  if (k === "create") return "创建";
  if (k === "delete") return "删除";
  if (k === "link") return "+ 关联";
  if (k === "unlink") return "− 解除";
  return "更新";
}

function isLinkOp(a: string): boolean {
  return a.endsWith("_link") || a.endsWith("_unlink");
}

function linkOpCN(a: string): string {
  if (a.endsWith("_link")) return "关联操作";
  if (a.endsWith("_unlink")) return "解除操作";
  return "操作";
}

function showDiffTable(entry: { action: string; fieldsChanged: string[] }): boolean {
  if (isLinkOp(entry.action)) return false;
  if (!entry.action.endsWith("_update")) return false;
  if (entry.fieldsChanged.length === 0) return false;
  if (entry.fieldsChanged.length === 1 && entry.fieldsChanged[0] === "*") return false;
  return true;
}
</script>

<style scoped>
/* ============== 抽屉容器 ============== */
.drawer {
  position: fixed;
  top: 0;
  right: 0;
  bottom: 0;
  width: 560px;
  max-width: 92vw;
  background: var(--paper);
  border-left: var(--rule-bold) solid var(--ink-1);
  box-shadow: -8px 0 24px rgba(0, 0, 0, 0.12);
  /* 必须高于 BypassAlertBanner 的 1000 */
  z-index: 1100;
  display: flex;
  flex-direction: column;
}
.mask {
  position: fixed; inset: 0;
  background: rgba(0, 0, 0, 0.18);
  z-index: 1090;
}

/* ============== 头 ============== */
.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  padding: var(--s-4) var(--s-4) var(--s-3);
  border-bottom: var(--rule-fine) solid var(--rule-2);
  background: var(--paper-2);
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
  display: flex;
  align-items: baseline;
  gap: var(--s-2);
  margin-bottom: 4px;
}
.head-title .tag {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}
.head-title .title {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}
.head-sub {
  font-size: var(--fs-xs);
  color: var(--ink-3);
}
.close {
  background: transparent;
  border: var(--rule-fine) solid var(--rule);
  width: 28px; height: 28px;
  font-size: 20px;
  cursor: pointer;
  color: var(--ink-2);
  line-height: 1;
}
.close:hover { background: var(--paper); border-color: var(--ink-1); }

/* ============== body ============== */
.body {
  flex: 1;
  overflow-y: auto;
  padding: var(--s-3) var(--s-4);
}
.state {
  padding: var(--s-5) var(--s-3);
  text-align: center;
  color: var(--ink-3);
  font-size: var(--fs-sm);
}
.state.empty .hint { font-size: var(--fs-xs); color: var(--ink-3); }

/* ============== 时间线 ============== */
.timeline {
  list-style: none;
  padding: 0;
  margin: 0;
  position: relative;
}
.entry {
  display: grid;
  grid-template-columns: 24px 1fr;
  gap: var(--s-2);
  padding-bottom: var(--s-4);
  position: relative;
}
.rail {
  position: relative;
  display: flex;
  justify-content: center;
}
.rail::before {
  content: "";
  position: absolute;
  top: 18px; bottom: -16px;
  width: 1px;
  background: var(--rule-2);
}
.entry:last-child .rail::before { display: none; }
.dot {
  position: relative;
  z-index: 1;
  width: 12px; height: 12px;
  margin-top: 6px;
  border: var(--rule-bold) solid var(--paper);
  border-radius: 0;
}
.dot.create { background: var(--acc); border-color: var(--acc); }
.dot.update { background: var(--warn); border-color: var(--warn); }
.dot.delete { background: var(--err); border-color: var(--err); }
.dot.link   { background: var(--ok); border-color: var(--ok); }
.dot.unlink { background: var(--ink-3); border-color: var(--ink-3); }

/* ============== 单条记录卡 ============== */
.card {
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule-2);
  border-left: var(--rule-mid) solid var(--ink-1);
  padding: var(--s-2) var(--s-3);
}
.card-head {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  font-size: var(--fs-xs);
  margin-bottom: var(--s-2);
}
.action-chip {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  padding: 2px 6px;
  border: var(--rule-fine) solid currentColor;
  color: var(--ink-1);
}
.action-chip.create { color: var(--acc); background: rgba(0, 102, 153, 0.08); }
.action-chip.update { color: var(--warn); background: rgba(204, 102, 0, 0.08); }
.action-chip.delete { color: var(--err);  background: rgba(204, 0, 0, 0.08); }
.action-chip.link   { color: var(--ok);   background: rgba(0, 153, 102, 0.10); }
.action-chip.unlink { color: var(--ink-3);background: rgba(80, 80, 80, 0.08); }
.ts { color: var(--ink-3); }
.actor { color: var(--ink-2); margin-left: auto; }

/* ============== 关联操作（sif_link / sif_unlink）============== */
.link-op {
  display: flex;
  align-items: flex-start;
  gap: var(--s-2);
  background: var(--paper-2);
  border: var(--rule-fine) solid var(--rule-2);
  border-left: var(--rule-mid) solid var(--ok);
  padding: var(--s-2) var(--s-3);
  margin-bottom: var(--s-2);
}
.link-op-label {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.12em;
  text-transform: uppercase;
  flex-shrink: 0;
  padding-top: 2px;
}
.link-op-desc {
  font-size: var(--fs-xs);
  color: var(--ink-1);
  font-weight: 500;
}

/* ============== 变更字段 chips ============== */
.fields-changed {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  flex-wrap: wrap;
  margin-bottom: var(--s-2);
}
.fields-changed .label {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.12em;
  text-transform: uppercase;
}
.chip {
  display: inline-block;
  padding: 2px 8px;
  font-size: var(--fs-xs);
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
.chips { display: inline-flex; flex-wrap: wrap; gap: 4px; }

/* ============== diff 表 ============== */
.diff-table-wrap {
  margin-top: var(--s-2);
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper-2);
}
.diff-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
}
.diff-table th {
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
.diff-table td {
  padding: 6px 8px;
  border-bottom: var(--rule-hair) solid var(--rule-2);
  vertical-align: top;
}
.diff-table tr:last-child td { border-bottom: 0; }
.field-name {
  font-weight: 600;
  color: var(--ink-1);
  width: 25%;
}
.diff-table .old {
  color: var(--ink-3);
  background: rgba(204, 0, 0, 0.04);
  text-decoration: line-through;
  text-decoration-color: var(--err);
  text-decoration-thickness: 1px;
  width: 37.5%;
}
.diff-table .new {
  color: var(--ink-1);
  background: rgba(0, 102, 153, 0.05);
  font-weight: 500;
  width: 37.5%;
}

/* ============== snapshot (create/delete) ============== */
.snapshot {
  margin-top: var(--s-2);
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper-2);
  padding: var(--s-2);
}
.snap-label {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.12em;
  color: var(--ink-3);
  text-transform: uppercase;
  margin-bottom: var(--s-1);
}
.kv {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 2px var(--s-2);
  font-size: var(--fs-xs);
}
.kv-key {
  color: var(--ink-3);
  padding-right: var(--s-1);
  border-right: var(--rule-hair) solid var(--rule-2);
}
.kv-val {
  color: var(--ink-2);
  word-break: break-all;
}

/* ============== 过渡 ============== */
.drawer-enter-active, .drawer-leave-active { transition: transform 0.22s ease; }
.drawer-enter-from, .drawer-leave-to { transform: translateX(100%); }
.mask-enter-active, .mask-leave-active { transition: opacity 0.22s ease; }
.mask-enter-from, .mask-leave-to { opacity: 0; }
</style>
