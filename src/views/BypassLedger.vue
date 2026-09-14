<script setup lang="ts">
/**
 * BypassLedger.vue — M2.2 旁路授权台账（IEC 61511-1 §11.5.2）
 *
 * 视图：
 *   1. 顶部 4 个统计卡（活动 / 逾期 / 已恢复 / 总数）
 *   2. 过滤栏（状态 + 项目）+ + 新建旁路 按钮
 *   3. 主表格（状态 / SIF / 操作人 / 批准人 / 票号 / 理由 / 计划恢复 / 剩余 / 操作）
 *   4. 新建 / 编辑 / 恢复 弹窗
 *
 * UI 规范：工业图纸风（参见 docs/STYLE-GUIDE.md）
 */
import { computed, onMounted, reactive, ref } from "vue";
import { useStudioStore } from "../stores/studio";
import { useAuthStore } from "../stores/auth";
import type { BypassInput, BypassListItem, BypassUpdate } from "../stores/studio";

const store = useStudioStore();
const auth = useAuthStore();

onMounted(async () => {
  await store.refreshBypasses("all");
});

// ===== 过滤 =====
const filter = ref<"all" | "active" | "overdue" | "restored">("all");
const projectFilter = ref<number | null>(null);

const filtered = computed(() => {
  let list = store.bypasses;
  if (filter.value !== "all") {
    list = list.filter((b) => b.status === filter.value);
  }
  if (projectFilter.value !== null) {
    list = list.filter((b) => b.projectId === projectFilter.value);
  }
  return list;
});

async function setFilter(f: "all" | "active" | "overdue" | "restored") {
  filter.value = f;
  await store.refreshBypasses(f, projectFilter.value ?? undefined);
}
async function setProjectFilter(pid: number | null) {
  projectFilter.value = pid;
  await store.refreshBypasses(filter.value, pid ?? undefined);
}

// ===== 统计 =====
const activeCount = computed(() => store.activeBypasses.length);
const overdueCount = computed(() => store.overdueBypasses.length);
const restoredCount = computed(() => store.restoredBypasses.length);
const totalCount = computed(() => store.bypasses.length);

// ===== 工具 =====
function formatHours(h: number): string {
  if (h >= 24) return `${(h / 24).toFixed(1)} 天`;
  if (h >= 0) return `${h.toFixed(1)} h`;
  // 负数 = 已逾期
  const d = Math.floor(-h / 24);
  const r = -h - d * 24;
  return d > 0 ? `逾期 ${d} 天 ${r.toFixed(0)} h` : `逾期 ${r.toFixed(1)} h`;
}

function formatPlanned(s: string): string {
  // ISO 8601 → "YYYY-MM-DD HH:mm"
  if (!s) return "—";
  return s.replace("T", " ").slice(0, 16);
}

function statusCN(s: string): string {
  switch (s) {
    case "active": return "活动";
    case "overdue": return "逾期";
    case "restored": return "已恢复";
    default: return s;
  }
}

// ===== 新建 / 编辑 =====
type Mode = "create" | "edit" | "restore" | null;
const mode = ref<Mode>(null);
const draft = reactive<BypassInput & { id?: number }>({
  projectId: 0,
  sifId: 0,
  bypassedBy: "",
  reason: "",
  plannedRestore: "",
  permitNo: "",
  approvedBy: "",
});
const editError = ref<string>("");
const submitting = ref(false);

function defaultPlannedISO(): string {
  // 默认 8 小时后（合规安全时间）
  const d = new Date(Date.now() + 8 * 3600 * 1000);
  // 转本地 ISO（无 Z = 本地时间，SQLite 字典序仍可比）
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
}

function openCreate() {
  mode.value = "create";
  editError.value = "";
  const firstProj = store.projects[0];
  const firstSif = firstProj ? store.sifSummary.find((s) => s.projectId === firstProj.id) : undefined;
  Object.assign(draft, {
    id: undefined,
    projectId: firstProj?.id ?? 0,
    sifId: firstSif?.id ?? 0,
    bypassedBy: "",
    reason: "",
    plannedRestore: defaultPlannedISO(),
    permitNo: "",
    approvedBy: "",
  });
}

function openEdit(b: BypassListItem) {
  mode.value = "edit";
  editError.value = "";
  Object.assign(draft, {
    id: b.id,
    projectId: b.projectId,
    sifId: b.sifId,
    bypassedBy: b.bypassedBy,
    reason: b.reason,
    plannedRestore: b.plannedRestore,
    permitNo: b.permitNo,
    approvedBy: b.approvedBy,
  });
}

function close() {
  mode.value = null;
  editError.value = "";
}

const sifOptions = computed(() => {
  if (!draft.projectId) return [];
  return store.sifSummary.filter((s) => s.projectId === draft.projectId);
});

async function saveCreate() {
  editError.value = "";
  submitting.value = true;
  try {
    await store.createBypass({
      projectId: draft.projectId,
      sifId: draft.sifId,
      bypassedBy: draft.bypassedBy,
      reason: draft.reason,
      plannedRestore: draft.plannedRestore,
      permitNo: draft.permitNo,
      approvedBy: draft.approvedBy,
    });
    close();
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    editError.value = err.message ?? String(e);
  } finally {
    submitting.value = false;
  }
}

async function saveEdit() {
  if (!draft.id) return;
  editError.value = "";
  submitting.value = true;
  try {
    const patch: BypassUpdate = {
      bypassedBy: draft.bypassedBy,
      reason: draft.reason,
      plannedRestore: draft.plannedRestore,
      permitNo: draft.permitNo,
      approvedBy: draft.approvedBy,
    };
    await store.updateBypass(draft.id, patch);
    close();
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    editError.value = err.message ?? String(e);
  } finally {
    submitting.value = false;
  }
}

// ===== 恢复 =====
const restoreId = ref<number | null>(null);
const restoreAt = ref<string>("");

function openRestore(b: BypassListItem) {
  mode.value = "restore";
  restoreId.value = b.id;
  // 默认 now (本地 ISO)
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  restoreAt.value = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:00`;
  editError.value = "";
}

async function doRestore() {
  if (restoreId.value === null) return;
  editError.value = "";
  submitting.value = true;
  try {
    await store.restoreBypass(restoreId.value, restoreAt.value);
    close();
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    editError.value = err.message ?? String(e);
  } finally {
    submitting.value = false;
  }
}

// ===== 删除 =====
async function doDelete(b: BypassListItem) {
  if (!confirm(`确认删除旁路 #${b.id}？\n理由：${b.reason}\n\n该操作不可逆且会写入审计。`)) return;
  try {
    await store.deleteBypass(b.id);
  } catch (e) {
    const err = e as { kind?: string; message?: string };
    alert(`删除失败：${err.message ?? String(e)}`);
  }
}
</script>

<template>
  <div class="bypass">
    <!-- ===== 统计 ===== -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">活动 · ACTIVE</div>
        <div class="panel-value" :class="{ alert: activeCount > 0 }">{{ activeCount }}</div>
        <div class="panel-foot">限时旁路在用</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">逾期 · OVERDUE</div>
        <div class="panel-value" :class="{ alert: overdueCount > 0 }">{{ overdueCount }}</div>
        <div class="panel-foot">超期未恢复</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">已恢复 · RESTORED</div>
        <div class="panel-value">{{ restoredCount }}</div>
        <div class="panel-foot">已写票收回</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">总票数 · TOTAL</div>
        <div class="panel-value">{{ totalCount }}</div>
        <div class="panel-foot">IEC 61511 §11.5.2</div>
      </div>
    </section>

    <!-- ===== 过滤 ===== -->
    <section class="head card">
      <div class="filter-group">
        <span class="filter-key">STATUS</span>
        <button
          v-for="f in ['all', 'active', 'overdue', 'restored'] as const"
          :key="f"
          class="seg"
          :class="{ active: filter === f }"
          @click="setFilter(f)"
        >
          {{ f === 'all' ? '全部' : statusCN(f) }}
        </button>
      </div>
      <select
        :value="projectFilter ?? ''"
        @change="setProjectFilter(($event.target as HTMLSelectElement).value === '' ? null : Number(($event.target as HTMLSelectElement).value))"
      >
        <option value="">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">
          {{ p.code }} {{ p.name }}
        </option>
      </select>
      <button v-if="auth.canWrite" class="primary" @click="openCreate">+ 新建旁路</button>
    </section>

    <!-- ===== 表格 ===== -->
    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 84px">状态 · STATUS</th>
          <th style="width: 100px">SIF</th>
          <th style="width: 70px">SIL</th>
          <th>项目 · PROJ</th>
          <th style="width: 110px">操作人 · BY</th>
          <th style="width: 110px">批准人 · APR</th>
          <th style="width: 110px">票号 · WP</th>
          <th>理由 · REASON</th>
          <th style="width: 130px">计划恢复</th>
          <th style="width: 110px">剩余 / 逾期</th>
          <th style="width: 160px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="b in filtered" :key="b.id" :class="{ overdue: b.status === 'overdue' }">
          <td>
            <span class="status-pill" :class="b.status">{{ statusCN(b.status) }}</span>
          </td>
          <td class="mono">{{ b.sifCode }}</td>
          <td>
            <span class="tag" :class="`sil-${b.sifSil.toLowerCase()}`">{{ b.sifSil }}</span>
          </td>
          <td class="mono">{{ b.projectCode }}</td>
          <td>{{ b.bypassedBy || '—' }}</td>
          <td>{{ b.approvedBy || '—' }}</td>
          <td class="mono">{{ b.permitNo || '—' }}</td>
          <td class="reason-cell" :title="b.reason">{{ b.reason }}</td>
          <td class="mono">{{ formatPlanned(b.plannedRestore) }}</td>
          <td
            class="mono num"
            :class="{
              ok: b.status === 'active' && b.hoursToRestore > 24,
              warn: b.status === 'active' && b.hoursToRestore <= 24,
              err: b.status === 'overdue',
            }"
          >
            {{ b.status === 'restored' ? '—' : formatHours(b.hoursToRestore) }}
          </td>
          <td class="row-actions">
            <template v-if="auth.canWrite">
              <button
                v-if="b.status !== 'restored'"
                class="sm primary"
                @click="openRestore(b)"
                title="恢复旁路"
              >恢复</button>
              <button v-if="b.status !== 'restored'" class="sm" @click="openEdit(b)" title="编辑">编辑</button>
              <button class="sm danger" @click="doDelete(b)" title="删除">删</button>
            </template>
            <span v-else class="muted mono">只读</span>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="empty">
      还没有旁路登记。点击「+ 新建旁路」登记第一条。
    </div>

    <!-- ===== 新建 / 编辑 弹窗 ===== -->
    <div
      v-if="mode === 'create' || mode === 'edit'"
      class="modal-mask"
      @click.self="close"
    >
      <div class="modal">
        <div class="modal-header">
          <span>{{ mode === 'create' ? '新建旁路 · NEW' : '编辑旁路 · EDIT' }}</span>
          <span class="ref">{{ mode === 'create' ? 'FORM-BYP-01' : `ID-${draft.id}` }}</span>
        </div>
        <div class="modal-body">
          <div v-if="editError" class="err-banner-inline">
            <span class="err-kind">ERR</span>
            <span class="err-msg">{{ editError }}</span>
          </div>
          <div class="form">
            <label>项目 · PROJECT
              <select v-model.number="draft.projectId" :disabled="mode === 'edit'">
                <option v-for="p in store.projects" :key="p.id" :value="p.id">
                  {{ p.code }} {{ p.name }}
                </option>
              </select>
            </label>
            <label>SIF
              <select v-model.number="draft.sifId" :disabled="mode === 'edit'">
                <option v-for="s in sifOptions" :key="s.id" :value="s.id">
                  {{ s.code }} · {{ s.name }} · SIL {{ s.silVerified }}
                </option>
              </select>
            </label>
            <label>操作人 · BYPASSED BY <input v-model="draft.bypassedBy" placeholder="姓名 / 工号" /></label>
            <label>批准人 · APPROVED BY <input v-model="draft.approvedBy" placeholder="姓名 / 工号" /></label>
            <label>票号 · PERMIT NO <input v-model="draft.permitNo" placeholder="WP-2026-0913" /></label>
            <label class="full">理由 · REASON
              <textarea v-model="draft.reason" rows="2" placeholder="≥ 5 字；如：反应器 PT-201 变送器送检，临时旁路" />
            </label>
            <label class="full">计划恢复时间 · PLANNED RESTORE
              <input v-model="draft.plannedRestore" placeholder="2026-09-15T18:00:00Z" />
              <span class="hint">ISO 8601；最少 1 小时后；建议 ≤ 8h（短期维护）</span>
            </label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">IEC 61511-1 §11.5.2 · MOC</span>
          <button @click="close">取消</button>
          <button
            class="primary"
            :disabled="submitting || !draft.sifId || !draft.bypassedBy || !draft.approvedBy || !draft.reason || !draft.plannedRestore"
            @click="mode === 'create' ? saveCreate() : saveEdit()"
          >
            {{ submitting ? '提交中…' : (mode === 'create' ? '登记' : '保存') }}
          </button>
        </div>
      </div>
    </div>

    <!-- ===== 恢复弹窗 ===== -->
    <div v-if="mode === 'restore'" class="modal-mask" @click.self="close">
      <div class="modal">
        <div class="modal-header">
          <span>恢复旁路 · RESTORE</span>
          <span class="ref">ID-{{ restoreId }}</span>
        </div>
        <div class="modal-body">
          <div v-if="editError" class="err-banner-inline">
            <span class="err-kind">ERR</span>
            <span class="err-msg">{{ editError }}</span>
          </div>
          <p class="restore-note">
            确认已物理恢复旁路（重新投用联锁）？<br />
            写票时间与现场实际恢复时间一致。
          </p>
          <div class="form">
            <label class="full">恢复时间 · RESTORED AT
              <input v-model="restoreAt" placeholder="2026-09-13T17:00:00Z" />
              <span class="hint">ISO 8601；不早于计划恢复前 24 小时</span>
            </label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">IEC 61511-1 §11.5.2</span>
          <button @click="close">取消</button>
          <button class="primary" :disabled="submitting" @click="doRestore">
            {{ submitting ? '提交中…' : '确认恢复' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.bypass { display: flex; flex-direction: column; gap: var(--s-3); }

/* ===== 统计 ===== */
.stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--s-3); }
.stat-card .panel-value.alert {
  color: var(--err);
}

/* ===== 过滤 ===== */
.head {
  display: flex;
  align-items: center;
  gap: var(--s-3);
}
.filter-group {
  display: flex;
  align-items: center;
  gap: var(--s-2);
}
.filter-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.seg {
  font-size: var(--fs-xs);
  padding: 4px 10px;
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule-2);
  color: var(--ink-2);
  font-family: var(--font-mono);
  letter-spacing: 0.04em;
}
.seg.active {
  background: var(--ink-1);
  color: var(--paper);
  border-color: var(--ink-1);
  font-weight: 700;
}

/* ===== 表格 ===== */
.reason-cell {
  max-width: 280px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
tr.overdue td { background: var(--err-bg); }
tr.overdue td:first-child { border-left: var(--rule-mid) solid var(--err); }

.status-pill {
  display: inline-block;
  padding: 1px 8px;
  font-family: var(--font-mono);
  font-size: var(--fs-micro);
  font-weight: 700;
  letter-spacing: 0.04em;
  border: var(--rule-fine) solid currentColor;
  text-transform: uppercase;
}
.status-pill.active   { color: var(--warn); background: var(--warn-bg); border-color: var(--warn); }
.status-pill.overdue  { color: var(--err);  background: var(--err-bg);  border-color: var(--err); }
.status-pill.restored { color: var(--ok);   background: var(--ok-bg);   border-color: var(--ok); }

td.ok   { color: var(--ok); }
td.warn { color: var(--warn); font-weight: 700; }
td.err  { color: var(--err); font-weight: 700; }

.row-actions {
  display: flex;
  gap: var(--s-1);
  justify-content: flex-end;
}

/* ===== 弹窗内嵌 ===== */
.err-banner-inline {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  margin-bottom: var(--s-3);
  padding: var(--s-2) var(--s-3);
  background: var(--err-bg);
  border: var(--rule-fine) solid var(--err);
  border-left-width: 4px;
  color: var(--err);
}
.err-banner-inline .err-kind {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.16em;
  padding: 1px 6px;
  border: var(--rule-fine) solid var(--err);
  background: var(--paper);
}
.err-banner-inline .err-msg {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ink-1);
  word-break: break-all;
}

.restore-note {
  margin: 0 0 var(--s-3) 0;
  padding: var(--s-2) var(--s-3);
  background: var(--paper-3);
  border-left: var(--rule-mid) solid var(--ink-1);
  font-size: var(--fs-sm);
  color: var(--ink-2);
  line-height: var(--lh-normal);
}
.form .hint {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-4);
  letter-spacing: 0.04em;
  text-transform: uppercase;
  margin-top: 2px;
}
</style>