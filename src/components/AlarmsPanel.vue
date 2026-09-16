<template>
  <div class="alarms">
    <div class="head card">
      <input
        v-model="query"
        placeholder="按报警位号 / 说明 / 响应动作搜索…"
        class="search"
      />
      <select v-model="filterPriority">
        <option value="">全部优先级</option>
        <option value="critical">紧急 CRITICAL</option>
        <option value="high">高 HIGH</option>
        <option value="medium">中 MEDIUM</option>
        <option value="low">低 LOW</option>
      </select>
      <select v-model="filterType">
        <option value="">全部类型</option>
        <option v-for="t in ALARM_TYPES" :key="t.v" :value="t.v">{{ t.label }}</option>
      </select>
      <!-- 项目筛选器仅全局模式显示；项目详情页锁定 projectId -->
      <select v-if="!locked" v-model.number="filterProjectId">
        <option :value="0">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">
          {{ p.code }} · {{ p.name }}
        </option>
      </select>
      <button v-if="auth.canWrite" class="primary" :disabled="!canCreateHere" @click="openCreate">+ 新建报警</button>
    </div>

    <div class="count-row">
      <span class="count-key">RESULT</span>
      <span class="count-val mono">{{ filtered.length }}</span>
      <span class="count-unit">条</span>
      <span class="count-sep">|</span>
      <span class="count-key">PROJECT</span>
      <span class="count-val">{{ activeProjectId ? projectName(activeProjectId) : "全部" }}</span>
      <span class="count-sep">|</span>
      <span class="count-key">PRIORITY</span>
      <span class="count-val">{{ filterPriority ? PRIORITY_CN[filterPriority] : "全部" }}</span>
    </div>

    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 120px">报警位号 · TAG</th>
          <th style="width: 70px">类型 · TYPE</th>
          <th style="width: 80px">优先级 · PRI</th>
          <th style="width: 80px">状态 · STS</th>
          <th style="width: 110px">设定值 · SP</th>
          <th style="width: 110px">关联仪表 · INST</th>
          <th style="width: 150px">项目 · PROJECT</th>
          <th>说明 · DESCRIPTION</th>
          <th style="width: 180px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="a in filtered" :key="a.id">
          <td class="mono">{{ a.tag }}</td>
          <td class="mono">{{ typeLabel(a.alarmType) }}</td>
          <td>
            <span class="tag pri" :class="`pri-${a.priority}`">{{ PRIORITY_CN[a.priority] ?? a.priority }}</span>
          </td>
          <td>
            <span class="tag sts" :class="`sts-${a.status}`">{{ STATUS_CN[a.status] ?? a.status }}</span>
          </td>
          <td class="mono">{{ a.setpoint == null ? "—" : a.setpoint }} {{ a.unit }}</td>
          <td class="mono">{{ instrumentTag(a.instrumentId) }}</td>
          <td class="mono">{{ projectCode(a.projectId) }}</td>
          <td>{{ a.description || "—" }}</td>
          <td class="row-actions">
            <template v-if="auth.canWrite">
              <button class="sm" @click="openEdit(a)">编辑</button>
              <button class="danger sm" @click="onDelete(a.id, a.tag)">删</button>
            </template>
            <button class="sm" @click="store.openEntityHistory('alarm', a.id)">历史</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="empty">
      还没有报警记录。可按 DCS 报警清单（PAHH/PALL…）逐步建立项目报警台账。
    </div>

    <!-- 编辑弹窗 -->
    <div v-if="editing" class="modal-mask" @click.self="close">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing.id ? "编辑报警 · EDIT" : "新建报警 · NEW ALARM" }}</span>
          <span class="ref">{{ editing.id ? `ALM-${editing.id}` : "ALM-NEW" }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label v-if="!locked" class="full">归属项目 · PROJECT <span class="req">*</span>
              <select v-model.number="draft.projectId" :disabled="!!editing.id">
                <option :value="0" disabled>— 请选择 —</option>
                <option v-for="p in store.projects" :key="p.id" :value="p.id">
                  {{ p.code }} · {{ p.name }}
                </option>
              </select>
              <span v-if="!draft.projectId" class="hint warn">报警台账按项目隔离，必须归属一个项目</span>
            </label>
            <label>报警位号 · TAG <input v-model="draft.tag" placeholder="PAHH-201" /></label>
            <label>类型 · TYPE
              <select v-model="draft.alarmType">
                <option v-for="t in ALARM_TYPES" :key="t.v" :value="t.v">{{ t.label }}</option>
              </select>
            </label>
            <label>优先级 · PRIORITY
              <select v-model="draft.priority">
                <option value="critical">紧急 CRITICAL</option>
                <option value="high">高 HIGH</option>
                <option value="medium">中 MEDIUM</option>
                <option value="low">低 LOW</option>
              </select>
            </label>
            <label>类别 · CATEGORY
              <select v-model="draft.category">
                <option value="process">工艺 PROCESS</option>
                <option value="equipment">设备 EQUIPMENT</option>
                <option value="safety">安全 SAFETY</option>
              </select>
            </label>
            <label>关联仪表 · INSTRUMENT <span class="hint">（可选，仅列本项目仪表）</span>
              <select v-model.number="draft.instrumentId">
                <option :value="null">— 不关联 —</option>
                <option v-for="i in projectInstruments" :key="i.id" :value="i.id">
                  {{ i.tag }} · {{ i.service }}
                </option>
              </select>
            </label>
            <label>状态 · STATUS
              <select v-model="draft.status">
                <option value="normal">正常 NORMAL</option>
                <option value="active">报警中 ACTIVE</option>
                <option value="bypassed">旁路 BYPASSED</option>
                <option value="shelved">搁置 SHELVED</option>
              </select>
            </label>
            <label>设定值 · SETPOINT <input type="number" v-model.number="draft.setpoint" /></label>
            <label>单位 · UNIT <input v-model="draft.unit" placeholder="MPa / ℃" /></label>
            <label>死区 · DEADBAND <input type="number" v-model.number="draft.deadband" /></label>
            <label>延时(秒) · DELAY <input type="number" min="0" v-model.number="draft.delaySeconds" /></label>
            <label class="full">说明 · DESCRIPTION <input v-model="draft.description" placeholder="反应器顶压力高高报警" /></label>
            <label class="full">响应动作 · RESPONSE <input v-model="draft.responseAction" placeholder="声光报警 / 操作员切断进料" /></label>
            <label class="full">备注 · NOTE <textarea v-model="draft.notes" rows="3"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-ALM-01 · ISA-18.2</span>
          <button @click="close">取消</button>
          <button class="primary" :disabled="!draft.tag || !draft.projectId" @click="save">保存</button>
        </div>
      </div>
    </div>

    <!-- 修改历史抽屉 -->
    <EntityHistoryDrawer entity-type="alarm" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useStudioStore, type Alarm } from "../stores/studio";
import { useAuthStore } from "../stores/auth";
import EntityHistoryDrawer from "./EntityHistoryDrawer.vue";

/**
 * AlarmsPanel — 报警台账双模式面板
 * - 无 projectId prop：全局总览模式（项目筛选下拉）
 * - 有 projectId：项目锁定模式（隐藏筛选，归属恒为该项目）
 *
 * 报警与联锁逻辑图 / SIF 无业务关联，仅可选软关联本项目仪表。
 */
const props = defineProps<{ projectId?: number }>();

const store = useStudioStore();
const auth = useAuthStore();
const locked = computed(() => props.projectId != null);

const query = ref("");
const filterPriority = ref("");
const filterType = ref("");
const filterProjectId = ref<number>(0);

const ALARM_TYPES = [
  { v: "HH", label: "高高报 HH" },
  { v: "H", label: "高报 H" },
  { v: "LL", label: "低低报 LL" },
  { v: "L", label: "低报 L" },
  { v: "DEV", label: "偏差 DEV" },
  { v: "RATE", label: "变化率 RATE" },
  { v: "DISC", label: "断线/故障 DISC" },
  { v: "OTHER", label: "其他 OTHER" },
] as const;

const PRIORITY_CN: Record<string, string> = {
  critical: "紧急",
  high: "高",
  medium: "中",
  low: "低",
};
const STATUS_CN: Record<string, string> = {
  normal: "正常",
  active: "报警中",
  bypassed: "旁路",
  shelved: "搁置",
};
function typeLabel(v: string) {
  return ALARM_TYPES.find((t) => t.v === v)?.label ?? v;
}

onMounted(async () => {
  if (!store.projects.length) await store.refreshProjects();
  // 报警列表首次进入时全量拉一次
  if (!store.alarms.length) await store.refreshAlarms();
  // 关联仪表下拉需要仪表字典
  if (!store.instruments.length) await store.refreshInstruments();
});

const activeProjectId = computed<number | "">(() => {
  if (locked.value) return props.projectId!;
  return filterProjectId.value > 0 ? filterProjectId.value : "";
});

const canCreateHere = computed(() => (locked.value ? true : !!activeProjectId.value));

/** 当前项目的仪表（弹窗关联下拉用；全局模式取筛选项目） */
const projectInstruments = computed(() => {
  const pid = activeProjectId.value;
  if (!pid) return [];
  return store.instruments.filter((i) => i.projectId === pid);
});

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  const pid = activeProjectId.value;
  return store.alarms.filter((a) => {
    if (pid && a.projectId !== pid) return false;
    if (filterPriority.value && a.priority !== filterPriority.value) return false;
    if (filterType.value && a.alarmType !== filterType.value) return false;
    if (!q) return true;
    return (
      a.tag.toLowerCase().includes(q) ||
      a.description.toLowerCase().includes(q) ||
      a.responseAction.toLowerCase().includes(q)
    );
  });
});

function projectCode(pid: number): string {
  const p = store.projects.find((x) => x.id === pid);
  return p ? p.code : `id=${pid}`;
}
function projectName(pid: number): string {
  const p = store.projects.find((x) => x.id === pid);
  return p ? `${p.code} ${p.name}` : `id=${pid}`;
}
function instrumentTag(iid: number | null): string {
  if (iid == null) return "—";
  const i = store.instruments.find((x) => x.id === iid);
  return i ? i.tag : `id=${iid}`;
}

const empty: Omit<Alarm, "id"> = {
  projectId: 0,
  tag: "",
  instrumentId: null,
  description: "",
  alarmType: "HH",
  priority: "high",
  category: "process",
  setpoint: null,
  unit: "",
  deadband: null,
  delaySeconds: 0,
  status: "normal",
  responseAction: "",
  notes: "",
};

const editing = ref<Alarm | null>(null);
const draft = reactive<Omit<Alarm, "id">>({ ...empty });

function openCreate() {
  editing.value = { id: 0, ...empty };
  Object.assign(draft, empty);
  draft.projectId = typeof activeProjectId.value === "number" ? activeProjectId.value : 0;
}
function openEdit(a: Alarm) {
  editing.value = a;
  const { id, ...rest } = a;
  Object.assign(draft, rest);
}
function close() {
  editing.value = null;
}

async function save() {
  if (!draft.tag || !draft.projectId) return;
  if (editing.value && editing.value.id) {
    await store.updateAlarm(editing.value.id, { ...draft });
  } else {
    await store.createAlarm({ ...draft });
  }
  close();
}
async function onDelete(id: number, tag: string) {
  if (!confirm(`确定删除报警 ${tag} ？`)) return;
  await store.deleteAlarm(id);
}
</script>

<style scoped>
.alarms { display: flex; flex-direction: column; gap: var(--s-3); }
.head { display: flex; gap: var(--s-2); align-items: center; }
.search { flex: 1; }

.count-row {
  display: flex;
  align-items: baseline;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule-2);
  border-left: var(--rule-mid) solid var(--ink-1);
  font-size: var(--fs-xs);
}
.count-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.count-val { font-size: var(--fs-md); font-weight: 700; color: var(--ink-1); }
.count-unit { color: var(--ink-3); font-size: var(--fs-xs); }
.count-sep { color: var(--rule-2); }

.row-actions { display: flex; gap: var(--s-1); justify-content: flex-end; }

/* 优先级标签配色 */
.tag.pri { font-weight: 700; }
.pri-critical { color: #fff; background: #b91c1c; border-color: #b91c1c; }
.pri-high     { color: var(--err);  background: #fde8e8; border-color: var(--err); }
.pri-medium   { color: var(--warn); background: #fef3e2; border-color: var(--warn); }
.pri-low      { color: var(--ink-3); background: var(--paper-3); border-color: var(--ink-3); }

.tag.sts { font-weight: 600; }
.sts-active   { color: var(--err); border-color: var(--err); }
.sts-bypassed { color: var(--warn); border-color: var(--warn); }
.sts-shelved  { color: var(--reset); border-color: var(--reset); }
.sts-normal   { color: var(--ok); border-color: var(--ok); }

.req { color: var(--err); font-weight: 700; margin-left: 2px; }
.hint { font-weight: 400; color: var(--ink-3); font-size: var(--fs-micro); }
.hint.warn { color: var(--warn); margin-top: 2px; display: block; }
</style>
