<template>
  <div class="instruments">
    <div class="head card">
      <input
        v-model="query"
        placeholder="按位号 / 服务 / 类型搜索…"
        class="search"
      />
      <select v-model="filterRole">
        <option value="">全部角色</option>
        <option value="detector">检测</option>
        <option value="final">最终</option>
        <option value="logic">逻辑</option>
        <option value="aux">旁路</option>
      </select>
      <!-- M2.9 — 项目筛选器（仅全局模式；项目详情页锁定 projectId） -->
      <select v-if="!locked" v-model="filterProjectId">
        <option value="">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">
          {{ p.code }} · {{ p.name }}
        </option>
      </select>
      <button v-if="auth.canWrite" class="primary" :disabled="!canCreateHere" @click="openCreate">+ 新建仪表</button>
    </div>

    <div class="count-row">
      <span class="count-key">RESULT</span>
      <span class="count-val mono">{{ filtered.length }}</span>
      <span class="count-unit">条</span>
      <span class="count-sep">|</span>
      <span class="count-key">PROJECT</span>
      <span class="count-val">{{ activeProjectId ? projectName(activeProjectId) : "全部" }}</span>
      <span class="count-sep">|</span>
      <span class="count-key">FILTER</span>
      <span class="count-val">{{ filterRole ? ROLE_CN[filterRole] : "全部" }}</span>
    </div>

    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 120px">位号 · TAG</th>
          <th style="width: 80px">类型 · KIND</th>
          <th style="width: 70px">角色 · ROLE</th>
          <th style="width: 70px">SIL</th>
          <th style="width: 160px">项目 · PROJECT</th>
          <th>服务 · SERVICE</th>
          <th>量程 · RANGE</th>
          <th>厂家 · MFR</th>
          <th style="width: 180px"></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="i in filtered" :key="i.id">
          <td class="mono">{{ i.tag }}</td>
          <td class="mono">{{ i.kind }}</td>
          <td>
            <span class="tag" :class="i.role">{{ roleCN(i.role) }}</span>
          </td>
          <td>
            <span class="tag" :class="`sil-${i.silTarget.toLowerCase()}`">{{ i.silTarget }}</span>
          </td>
          <td class="mono">{{ projectCode(i.projectId) }}</td>
          <td>{{ i.service }}</td>
          <td class="mono">{{ fmtRange(i.rangeMin, i.rangeMax) }} {{ i.unit }}</td>
          <td>{{ i.manufacturer || "—" }}</td>
          <td class="row-actions">
            <template v-if="auth.canWrite">
              <button class="sm" @click="openEdit(i)">编辑</button>
              <button class="danger sm" @click="onDelete(i.id, i.tag)">删</button>
            </template>
            <button class="sm" @click="store.openInstrumentHistory(i.id)">历史</button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="empty">
      还没仪表。请先在 P&amp;ID 截图 / DCS 导出里挑出常用位号，逐步建台账。
    </div>

    <!-- 编辑弹窗 -->
    <div v-if="editing" class="modal-mask" @click.self="close">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing.id ? "编辑仪表 · EDIT" : "新建仪表 · NEW" }}</span>
          <span class="ref">{{ editing.id ? `ID-${editing.id}` : "ID-NEW" }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <!-- M2.9 — 项目归属必填（锁定模式已隐含当前项目，不再显示选择器） -->
            <label v-if="!locked" class="full">归属项目 · PROJECT <span class="req">*</span>
              <select v-model.number="draft.projectId" :disabled="!!editing.id">
                <option :value="0" disabled>— 请选择 —</option>
                <option v-for="p in store.projects" :key="p.id" :value="p.id">
                  {{ p.code }} · {{ p.name }}
                </option>
              </select>
              <span v-if="!draft.projectId" class="hint warn">仪表台账按项目隔离，必须归属一个项目</span>
            </label>
            <label>位号 · TAG <input v-model="draft.tag" placeholder="PT-201" /></label>
            <label>类型 · KIND <input v-model="draft.kind" placeholder="PT / LT / XV" /></label>
            <label>角色 · ROLE
              <select v-model="draft.role">
                <option value="detector">检测 DETECTOR</option>
                <option value="final">最终 FINAL</option>
                <option value="logic">逻辑 LOGIC</option>
                <option value="aux">旁路 AUX</option>
              </select>
            </label>
            <label>SIL
              <select v-model="draft.silTarget">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label class="full">服务描述 · SERVICE <input v-model="draft.service" placeholder="反应器顶部压力" /></label>
            <label>厂家 · MFR <input v-model="draft.manufacturer" /></label>
            <label>型号 · MODEL <input v-model="draft.model" /></label>
            <label>量程下限 · RNG MIN <input type="number" v-model.number="draft.rangeMin" /></label>
            <label>量程上限 · RNG MAX <input type="number" v-model.number="draft.rangeMax" /></label>
            <label>单位 · UNIT <input v-model="draft.unit" placeholder="kPaG" /></label>
            <label>设定值 · SP <input type="number" v-model.number="draft.setpoint" /></label>
            <label>检验周期(月) · TI <input type="number" v-model.number="draft.proofInterval" /></label>
            <!-- IEC 61511-2 失效参数 -->
            <label>λDU(/h) · DANG UNDET <input type="number" step="1e-10" v-model.number="draft.lambdaDu" /></label>
            <label>λDD(/h) · DANG DET <input type="number" step="1e-10" v-model.number="draft.lambdaDd" /></label>
            <label>λSU(/h) · SAFE UNDET <input type="number" step="1e-10" v-model.number="draft.lambdaSu" /></label>
            <label>λSD(/h) · SAFE DET <input type="number" step="1e-10" v-model.number="draft.lambdaSd" /></label>
            <label>SFF(0~1) <input type="number" step="0.01" v-model.number="draft.sff" /></label>
            <label>PTC(0~1) <input type="number" step="0.01" v-model.number="draft.ptCoverage" /></label>
            <label>HFT(0/1/2) <input type="number" min="0" max="2" v-model.number="draft.hft" /></label>
            <label>设备类型 · TYPE
              <select v-model="draft.equipmentType">
                <option value="type_b">Type B · 复杂（PLC/智能表）</option>
                <option value="type_a">Type A · 简单（继电器/开关）</option>
              </select>
            </label>
            <label class="full">备注 · NOTE <textarea v-model="draft.notes" rows="3"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-INST-01 · IEC 61511</span>
          <button @click="close">取消</button>
          <button class="primary" :disabled="!draft.tag || !draft.projectId" @click="save">保存</button>
        </div>
      </div>
    </div>

    <!-- M2.3 — 修改历史抽屉 -->
    <EntityHistoryDrawer entity-type="instrument" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useStudioStore, type Instrument } from "../stores/studio";
import { useAuthStore } from "../stores/auth";
import EntityHistoryDrawer from "./EntityHistoryDrawer.vue";

/**
 * InstrumentsPanel — 仪表台账双模式面板
 * - 无 projectId prop：全局总览模式（项目筛选下拉，默认第一个项目）
 * - 有 projectId：项目锁定模式（隐藏筛选，归属恒为该项目）
 */
const props = defineProps<{ projectId?: number }>();

const store = useStudioStore();
const auth = useAuthStore();
const locked = computed(() => props.projectId != null);
const query = ref("");
const filterRole = ref("");
// M2.9 — 项目筛选器（默认空 = 全部项目；可在 store 启动后选具体项目）
const filterProjectId = ref<number | "">(0);

const empty: Omit<Instrument, "id"> = {
  tag: "",
  service: "",
  kind: "",
  role: "detector",
  psvId: "",
  manufacturer: "",
  model: "",
  rangeMin: null,
  rangeMax: null,
  unit: "",
  setpoint: null,
  silTarget: "NA",
  proofInterval: 12,
  installedAt: "",
  notes: "",
  // M2.9 — 默认无归属项目（弹窗必选）
  projectId: 0,
  // IEC 61511-2 — 失效参数（默认 0，不填则 PFDavg 不计算）
  lambdaDu: 0,
  lambdaDd: 0,
  lambdaSu: 0,
  lambdaSd: 0,
  sff: 0,
  ptCoverage: 1,
  hft: 0,
  // IEC 61508-2 — 默认 Type B（智能电子设备，更保守的架构约束）
  equipmentType: "type_b",
};

// M2.9 — 启动时：全局模式默认筛「第一个项目」；锁定模式直接用 projectId
onMounted(async () => {
  if (!store.projects.length) {
    await store.refreshProjects();
  }
  if (!locked.value && store.projects.length && filterProjectId.value === 0) {
    filterProjectId.value = store.projects[0].id;
  }
  // 仪表列表首次进入时全量拉一次
  if (!store.instruments.length) {
    await store.refreshInstruments();
  }
});

const activeProjectId = computed<number | "">(() => {
  if (locked.value) return props.projectId!;
  return typeof filterProjectId.value === "number" && filterProjectId.value > 0
    ? filterProjectId.value
    : "";
});

// 当前筛选条件下是否允许新建（锁定模式有项目即可）
const canCreateHere = computed(() => (locked.value ? true : !!activeProjectId.value));

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  const pid = activeProjectId.value;
  return store.instruments.filter((i) => {
    if (pid && i.projectId !== pid) return false;
    if (filterRole.value && i.role !== filterRole.value) return false;
    if (!q) return true;
    return (
      i.tag.toLowerCase().includes(q) ||
      i.service.toLowerCase().includes(q) ||
      i.kind.toLowerCase().includes(q)
    );
  });
});

/** M2.9 — 项目 id → 编号 / 名称查询辅助 */
function projectCode(pid: number | null | undefined): string {
  if (pid == null) return "—";
  const p = store.projects.find((x) => x.id === pid);
  return p ? p.code : `id=${pid}`;
}
function projectName(pid: number): string {
  const p = store.projects.find((x) => x.id === pid);
  return p ? `${p.code} ${p.name}` : `id=${pid}`;
}

const editing = ref<Instrument | null>(null);
const draft = reactive<Omit<Instrument, "id">>({ ...empty });

function openCreate() {
  editing.value = { id: 0, ...empty };
  Object.assign(draft, empty);
  // M2.9 — 默认选中当前筛选的项目（如选了具体项目），避免每次必选
  draft.projectId =
    typeof activeProjectId.value === "number" ? activeProjectId.value : 0;
}
function openEdit(i: Instrument) {
  editing.value = i;
  const { id, ...rest } = i;
  Object.assign(draft, rest);
}
function close() {
  editing.value = null;
}

async function save() {
  if (!draft.tag) return;
  if (editing.value && editing.value.id) {
    await store.updateInstrument(editing.value.id, { ...draft });
  } else {
    await store.createInstrument({ ...draft });
  }
  close();
}
async function onDelete(id: number, tag: string) {
  if (!confirm(`确定删除 ${tag} ？已被 SIF 引用的关联会一并清理。`)) return;
  await store.deleteInstrument(id);
}

const ROLE_CN: Record<string, string> = {
  detector: "检测",
  final: "最终",
  logic: "逻辑",
  aux: "旁路",
};
function roleCN(r: string) {
  return ROLE_CN[r] ?? r;
}
function fmtRange(min: number | null, max: number | null) {
  if (min == null && max == null) return "—";
  if (min == null) return `~${max}`;
  if (max == null) return `${min}~`;
  return `${min}~${max}`;
}
</script>

<style scoped>
.instruments { display: flex; flex-direction: column; gap: var(--s-3); }
.head { display: flex; gap: var(--s-2); align-items: center; }
.search { flex: 1; }

/* 计数条（工程图式） */
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
.count-val {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}
.count-unit { color: var(--ink-3); font-size: var(--fs-xs); }
.count-sep { color: var(--rule-2); }

.row-actions {
  display: flex;
  gap: var(--s-1);
  justify-content: flex-end;
}

/* M2.9 — 项目必填提示 */
.req { color: var(--err); font-weight: 700; margin-left: 2px; }
.hint.warn { font-size: var(--fs-micro); color: var(--warn); margin-top: 2px; display: block; }
</style>
