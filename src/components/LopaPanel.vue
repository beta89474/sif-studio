<template>
  <div class="lopa">
    <!-- 统计 -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">场景数 · SCENARIOS</div>
        <div class="panel-value">{{ filtered.length }}</div>
        <div class="panel-foot">IEC 61511-1 Annex E</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">保护层 · LAYERS</div>
        <div class="panel-value">{{ totalLayers }}</div>
        <div class="panel-foot">IPL / 旁路 / 报警 / 程序</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">严重度 · MINOR</div>
        <div class="panel-value">{{ severityCounts.minor }}</div>
        <div class="panel-foot">轻微</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">严重度 · MEDIUM</div>
        <div class="panel-value">{{ severityCounts.medium }}</div>
        <div class="panel-foot">中等</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">严重度 · MAJOR</div>
        <div class="panel-value">{{ severityCounts.major }}</div>
        <div class="panel-foot">严重</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">严重度 · CATASTROPHIC</div>
        <div class="panel-value">{{ severityCounts.catastrophic }}</div>
        <div class="panel-foot">灾难</div>
      </div>
    </section>

    <!-- 过滤 -->
    <section class="head card">
      <input class="search" v-model="query" placeholder="按编号 / 标题搜索…" />
      <select v-model="projectFilter">
        <option value="">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
      </select>
      <button v-if="auth.canWrite" class="primary" @click="openCreate">
        <Plus :size="14" /> 新建场景
      </button>
    </section>

    <!-- 列表 -->
    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 32px"></th>
          <th style="width: 90px">项目 · PRJ</th>
          <th style="width: 120px">编号 · CODE</th>
          <th>标题 · TITLE</th>
          <th style="width: 90px">严重度 · SEV</th>
          <th style="width: 100px">SIL 推导</th>
          <th style="width: 110px">对照 · GAP</th>
          <th class="num" style="width: 70px">层数 · LAYERS</th>
          <th style="width: 200px">操作</th>
        </tr>
      </thead>
      <tbody>
        <template v-for="s in filtered" :key="s.id">
          <tr :class="{ active: expanded === s.id }">
            <td>
              <button class="icon" @click="toggle(s.id)">
                <ChevronRight v-if="expanded !== s.id" :size="14" />
                <ChevronDown v-else :size="14" />
              </button>
            </td>
            <td class="mono">{{ s.projectCode }}</td>
            <td class="mono">{{ s.code }}</td>
            <td>{{ s.title }}</td>
            <td>
              <span class="tag" :class="severityTagClass(s.severity)">{{ severityCN(s.severity) }}</span>
            </td>
            <td>
              <span class="tag" :class="`sil-${s.silClaim.toLowerCase()}`">SIL {{ s.silClaim }}</span>
              <span
                v-if="s.silFromRrf && s.silFromRrf !== s.silClaim"
                class="tag vs vs-overclaimed"
                :title="`RRF=${s.rrfRequired ?? ''} 反推 SIL ${s.silFromRrf}，与手填不一致`"
              >RRF→{{ s.silFromRrf }}</span>
            </td>
            <td>
              <span class="tag gap" :class="`gap-${s.gapStatus}`" :title="s.gapMessage">{{ gapCN(s.gapStatus) }}</span>
            </td>
            <td class="num">{{ s.layerCount }}</td>
            <td class="row-actions">
              <button v-if="auth.canWrite" class="sm" @click="openEdit(s)">编辑</button>
              <button v-if="auth.canWrite" class="danger sm" @click="onDelete(s.id)">删除</button>
              <button class="sm" @click="toggleLayers(s.id)">查看保护层</button>
            </td>
          </tr>
          <tr v-if="expanded === s.id" class="detail-row">
            <td colspan="9">
              <div class="detail">
                <div class="detail-block full">
                  <h4>场景描述 · SCENARIO</h4>
                  <dl class="kv">
                    <dt>项目</dt><dd class="mono">{{ s.projectCode }}</dd>
                    <dt>编号</dt><dd class="mono">{{ s.code }}</dd>
                    <dt>SIF</dt>
                    <dd class="mono">
                      <template v-if="s.sifCode">{{ s.sifCode }}</template>
                      <span v-else class="muted">定级前场景</span>
                    </dd>
                    <dt>严重度</dt><dd><span class="tag" :class="severityTagClass(s.severity)">{{ severityCN(s.severity) }}</span></dd>
                    <dt>SIL 推导</dt><dd><span class="tag" :class="`sil-${s.silClaim.toLowerCase()}`">SIL {{ s.silClaim }}</span></dd>
                    <dt>初始频率(/a)</dt><dd class="mono">{{ s.initFreq != null ? s.initFreq.toExponential(2) : "—" }}</dd>
                    <dt>风险可容忍(/a)</dt><dd class="mono">{{ s.riskTol != null ? s.riskTol.toExponential(2) : "—" }}</dd>
                    <dt>危害 · HAZARD</dt><dd class="full">{{ s.hazard || "—" }}</dd>
                    <dt>原因 · CAUSE</dt><dd class="full">{{ s.cause || "—" }}</dd>
                    <dt>后果 · CONSEQUENCE</dt><dd class="full">{{ s.consequence || "—" }}</dd>
                    <dt>备注 · NOTES</dt><dd class="full">{{ s.notes || "—" }}</dd>
                  </dl>
                </div>

                <!-- 定级追溯 · GAP ANALYSIS（LOPA claim ↔ SIF design/achieved） -->
                <div class="detail-block full">
                  <h4>定级追溯 · GAP ANALYSIS</h4>
                  <dl class="kv">
                    <dt>RRF 需求</dt>
                    <dd class="mono">{{ s.rrfRequired != null ? s.rrfRequired.toExponential(2) : "—" }}</dd>
                    <dt>RRF 反推 SIL</dt>
                    <dd>
                      <span v-if="s.silFromRrf" class="tag" :class="`sil-${s.silFromRrf.toLowerCase()}`">SIL {{ s.silFromRrf }}</span>
                      <span v-else class="muted">需填写频率与可容忍值</span>
                    </dd>
                    <dt>LOPA 定级</dt>
                    <dd><span class="tag" :class="`sil-${s.silClaim.toLowerCase()}`">SIL {{ s.silClaim }}</span></dd>
                    <dt>SIF 设计目标</dt>
                    <dd>
                      <span v-if="s.sifSilDesign" class="tag" :class="`sil-${s.sifSilDesign.toLowerCase()}`">SIL {{ s.sifSilDesign }}</span>
                      <span v-else class="muted">—</span>
                    </dd>
                    <dt>SIF 计算达到</dt>
                    <dd>
                      <span v-if="s.sifSilAchieved" class="tag" :class="`sil-${s.sifSilAchieved.toLowerCase()}`">SIL {{ s.sifSilAchieved }}</span>
                      <span v-else class="muted">无失效数据</span>
                    </dd>
                    <dt>对照状态</dt>
                    <dd><span class="tag gap" :class="`gap-${s.gapStatus}`">{{ gapCN(s.gapStatus) }}</span></dd>
                    <dt>对照说明</dt><dd class="full">{{ s.gapMessage || "—" }}</dd>
                  </dl>
                </div>

                <!-- 保护层子视图 -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>独立保护层 · IPL</h4>
                    <button v-if="auth.canWrite" class="sm primary" @click="openCreateLayer(s.id)">+ 添加保护层</button>
                  </div>

                  <!-- 保护层内联表单 -->
                  <div v-if="layerEditing != null" class="layer-form">
                    <div class="form-row">
                      <label class="f-seq">序号 · SEQ
                        <input type="number" min="1" v-model.number="layerForm.seq" />
                      </label>
                      <label class="f-type">类型 · TYPE
                        <select v-model="layerForm.layerType">
                          <option value="ipl">IPL 独立保护层</option>
                          <option value="bypass">旁路 BYPASS</option>
                          <option value="alarm">报警 ALARM</option>
                          <option value="procedural">程序 PROCEDURAL</option>
                        </select>
                      </label>
                      <label class="f-desc">描述 · DESC
                        <input v-model="layerForm.description" placeholder="如 反应器超高压联锁切断进料" />
                      </label>
                      <label class="f-pfd">PFD
                        <input type="number" step="0.0001" v-model.number="layerForm.pfd" />
                      </label>
                      <label class="f-credit">信用 · CREDIT
                        <input type="number" step="0.1" v-model.number="layerForm.credit" />
                      </label>
                      <div class="form-actions">
                        <button class="primary sm" @click="saveLayer">保存</button>
                        <button class="sm" @click="closeLayerForm">取消</button>
                      </div>
                    </div>
                  </div>

                  <div v-if="layersFor(s.id).length" class="layer-list">
                    <div v-for="l in layersFor(s.id)" :key="l.id" class="layer-row">
                      <span class="layer-seq mono">{{ l.seq }}</span>
                      <span class="tag" :class="layerTypeClass(l.layerType)">{{ layerTypeCN(l.layerType) }}</span>
                      <span class="layer-desc">{{ l.description || "—" }}</span>
                      <span class="layer-pfd mono">PFD {{ l.pfd != null ? l.pfd.toExponential(2) : "—" }}</span>
                      <span class="layer-credit mono">信用 {{ l.credit }}</span>
                      <span v-if="auth.canWrite" class="row-actions layer-actions">
                        <button class="sm" @click="openEditLayer(l)">编辑</button>
                        <button class="danger sm" @click="onDeleteLayer(l.id, s.id)">删除</button>
                      </span>
                    </div>
                  </div>
                  <div v-else class="muted">该场景尚未登记任何独立保护层。</div>
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
    <div v-else class="empty">
      还没有 LOPA 场景。请新建一个场景（如 LOPA-201），描述危害事件 / 原因 / 后果，再逐层登记独立保护层 ——
      层数与 PFD 将推导出该场景的 SIL 等级需求。
    </div>

    <!-- 新建 / 编辑场景弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="closeEditor">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing !== null ? "编辑场景 · EDIT" : "新建场景 · NEW" }}</span>
          <span class="ref">IEC 61511-1 Annex E</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label class="full">项目 · PROJECT
              <select v-model="form.projectId">
                <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
              </select>
            </label>
            <label>编号 · CODE <input v-model="form.code" placeholder="LOPA-201" /></label>
            <label>标题 · TITLE <input v-model="form.title" placeholder="反应器超压导致泄放失效" /></label>
            <label>关联 SIF · SIF
              <select v-model="form.sifId">
                <option :value="null">定级前场景</option>
                <option v-for="sif in sifsForProject" :key="sif.id" :value="sif.id">
                  {{ sif.code }} {{ sif.name }}
                </option>
              </select>
            </label>
            <label>严重度 · SEVERITY
              <select v-model="form.severity">
                <option value="minor">轻微 MINOR</option>
                <option value="medium">中等 MEDIUM</option>
                <option value="major">严重 MAJOR</option>
                <option value="catastrophic">灾难 CATASTROPHIC</option>
              </select>
            </label>
            <label>SIL 推导 · SIL CLAIM
              <select v-model="form.silClaim">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label>初始频率(/a) · INIT FREQ
              <input type="number" step="1e-3" v-model.number="form.initFreq" />
            </label>
            <label>风险可容忍(/a) · RISK TOL
              <input type="number" step="1e-4" v-model.number="form.riskTol" />
            </label>
            <label class="full">危害 · HAZARD <textarea v-model="form.hazard" rows="2" placeholder="如 反应器超压"></textarea></label>
            <label class="full">原因 · CAUSE <textarea v-model="form.cause" rows="2" placeholder="如 冷却失效 / 进料过量"></textarea></label>
            <label class="full">后果 · CONSEQUENCE <textarea v-model="form.consequence" rows="2" placeholder="如 安全阀开启 / 物料泄漏"></textarea></label>
            <label class="full">备注 · NOTES <textarea v-model="form.notes" rows="2"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-LOPA-01</span>
          <button @click="closeEditor">取消</button>
          <button class="primary" :disabled="!form.code || !form.title || !form.projectId" @click="saveNow">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ChevronRight, ChevronDown, Plus } from "lucide-vue-next";
import {
  useStudioStore,
  type LopaScenario,
  type LopaScenarioInput,
  type LopaLayer,
  type LopaLayerInput,
} from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const store = useStudioStore();
const auth = useAuthStore();

onMounted(async () => {
  // 拉取场景列表 + 项目 + SIF 汇总（用于弹窗下拉）
  await Promise.all([
    store.refreshLopaScenarios(),
    store.projects.length === 0 ? store.refreshProjects() : Promise.resolve(),
    store.sifSummary.length === 0 ? store.refreshSifSummary() : Promise.resolve(),
  ]);
});

const query = ref("");
const projectFilter = ref<string | number>("");
const expanded = ref<number | null>(null);

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return store.lopaScenarios.filter((s) => {
    if (projectFilter.value !== "" && String(s.projectId) !== String(projectFilter.value)) {
      return false;
    }
    if (!q) return true;
    return s.code.toLowerCase().includes(q) || s.title.toLowerCase().includes(q);
  });
});

const totalLayers = computed(() =>
  store.lopaScenarios.reduce((a, s) => a + s.layerCount, 0),
);

const severityCounts = computed(() => {
  const c = { minor: 0, medium: 0, major: 0, catastrophic: 0 };
  for (const s of store.lopaScenarios) {
    if (s.severity in c) {
      (c as Record<string, number>)[s.severity] += 1;
    }
  }
  return c;
});

const SEVERITY_CN: Record<string, string> = {
  minor: "轻微",
  medium: "中等",
  major: "严重",
  catastrophic: "灾难",
};
function severityCN(s: string) {
  return SEVERITY_CN[s] ?? s;
}
function severityTagClass(s: string) {
  switch (s) {
    case "minor": return "sil-na";
    case "medium": return "sil-a";
    case "major": return "sil-b";
    case "catastrophic": return "sil-d";
    default: return "sil-na";
  }
}

const LAYER_TYPE_CN: Record<string, string> = {
  ipl: "IPL",
  bypass: "旁路",
  alarm: "报警",
  procedural: "程序",
};
function layerTypeCN(t: string) {
  return LAYER_TYPE_CN[t] ?? t;
}

// LOPA ↔ SIF 三方对照状态
const GAP_CN: Record<string, string> = {
  unlinked: "未关联",
  na: "无需 SIS",
  pending_data: "待数据",
  covered: "已覆盖",
  gap: "缺口",
  drift: "目标漂移",
};
function gapCN(s: string) {
  return GAP_CN[s] ?? s;
}
function layerTypeClass(t: string) {
  switch (t) {
    case "ipl": return "logic";
    case "bypass": return "aux";
    case "alarm": return "final";
    case "procedural": return "detector";
    default: return "logic";
  }
}

async function toggle(id: number) {
  if (expanded.value === id) {
    expanded.value = null;
    return;
  }
  expanded.value = id;
  if (store.lopaLayersForScenarioId !== id) {
    await store.refreshLopaLayers(id);
  }
}

async function toggleLayers(id: number) {
  await toggle(id);
}

function layersFor(id: number): LopaLayer[] {
  if (store.lopaLayersForScenarioId === id) {
    return store.lopaLayers;
  }
  return [];
}

// ============================================================================
// 场景 新建 / 编辑
// ============================================================================
const creating = ref<boolean | null>(null);
const editing = ref<number | null>(null);
const form = reactive<LopaScenarioInput>({
  projectId: 0,
  sifId: null as number | null,
  code: "",
  title: "",
  hazard: "",
  cause: "",
  consequence: "",
  severity: "medium",
  initFreq: null as number | null,
  riskTol: null as number | null,
  silClaim: "NA",
  notes: "",
});

/** 弹窗里可选 SIF：与当前选定项目一致；切项目时自动过滤 */
const sifsForProject = computed(() =>
  form.projectId
    ? store.sifSummary.filter((s) => s.projectId === form.projectId)
    : store.sifSummary,
);

function openCreate() {
  form.projectId = store.projects[0]?.id ?? 0;
  form.sifId = null;
  form.code = "";
  form.title = "";
  form.hazard = "";
  form.cause = "";
  form.consequence = "";
  form.severity = "medium";
  form.initFreq = null;
  form.riskTol = null;
  form.silClaim = "NA";
  form.notes = "";
  editing.value = null;
  creating.value = true;
}

function openEdit(s: LopaScenario) {
  form.projectId = s.projectId;
  form.sifId = s.sifId;
  form.code = s.code;
  form.title = s.title;
  form.hazard = s.hazard || "";
  form.cause = s.cause || "";
  form.consequence = s.consequence || "";
  form.severity = s.severity;
  form.initFreq = s.initFreq;
  form.riskTol = s.riskTol;
  form.silClaim = s.silClaim;
  form.notes = s.notes;
  editing.value = s.id;
  creating.value = true;
}

function closeEditor() {
  creating.value = null;
  editing.value = null;
}

async function saveNow() {
  if (!form.code || !form.title || !form.projectId) return;
  if (editing.value !== null) {
    await store.updateLopaScenario(editing.value, { ...form });
  } else {
    await store.createLopaScenario({ ...form });
  }
  creating.value = null;
  editing.value = null;
}

async function onDelete(id: number) {
  if (!confirm("确认删除该场景及其所有保护层？")) return;
  await store.deleteLopaScenario(id);
  if (expanded.value === id) expanded.value = null;
}

// ============================================================================
// 保护层 新建 / 编辑（内联表单）
// ============================================================================
/** null=关闭；number=编辑某 layer.id；-1=新建 */
const layerEditing = ref<number | null>(null);
const layerForm = reactive<LopaLayerInput>({
  scenarioId: 0,
  seq: 1,
  layerType: "ipl",
  description: "",
  pfd: null as number | null,
  credit: 1,
});

function openCreateLayer(scenarioId: number) {
  const existing = layersFor(scenarioId);
  const maxSeq = existing.reduce((m, l) => Math.max(m, l.seq), 0);
  layerForm.scenarioId = scenarioId;
  layerForm.seq = maxSeq + 1;
  layerForm.layerType = "ipl";
  layerForm.description = "";
  layerForm.pfd = null;
  layerForm.credit = 1;
  layerEditing.value = -1;
}

function openEditLayer(l: LopaLayer) {
  layerForm.scenarioId = l.scenarioId;
  layerForm.seq = l.seq;
  layerForm.layerType = l.layerType;
  layerForm.description = l.description;
  layerForm.pfd = l.pfd;
  layerForm.credit = l.credit;
  layerEditing.value = l.id;
}

function closeLayerForm() {
  layerEditing.value = null;
}

async function saveLayer() {
  if (layerEditing.value === null) return;
  if (!layerForm.scenarioId) return;
  const payload: LopaLayerInput = {
    scenarioId: layerForm.scenarioId,
    seq: layerForm.seq,
    layerType: layerForm.layerType,
    description: layerForm.description,
    pfd: layerForm.pfd,
    credit: layerForm.credit,
  };
  if (layerEditing.value === -1) {
    await store.createLopaLayer(payload);
  } else {
    await store.updateLopaLayer(layerEditing.value, payload);
  }
  layerEditing.value = null;
}

async function onDeleteLayer(id: number, scenarioId: number) {
  if (!confirm("确认删除该保护层？")) return;
  await store.deleteLopaLayer(id, scenarioId);
}
</script>

<style scoped>
.lopa { display: flex; flex-direction: column; gap: var(--s-3); }
.stats {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: var(--s-3);
}
.head { display: flex; gap: var(--s-2); align-items: center; }
.search { flex: 1; }

tr.active td { background: var(--acc-bg); }
tr.active td:first-child { border-left: var(--rule-mid) solid var(--acc); }

.detail-row td {
  background: var(--paper-2);
  padding: var(--s-4) var(--s-5);
  border-top: var(--rule-fine) solid var(--rule-2);
}
.detail {
  display: grid;
  grid-template-columns: 1fr;
  gap: var(--s-3) var(--s-4);
}
.detail-block h4 {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.12em;
  margin: 0 0 var(--s-2);
  padding-bottom: var(--s-1);
  border-bottom: var(--rule-fine) solid var(--rule-2);
}
.detail-block.full { grid-column: 1 / -1; }
.block-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--s-2);
}
.block-head h4 { margin: 0; padding: 0; border: 0; }

.row-actions { display: flex; gap: var(--s-1); justify-content: flex-end; }

/* 保护层列表 */
.layer-list {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
  margin-top: var(--s-2);
}
.layer-row {
  display: grid;
  grid-template-columns: 40px 80px 1fr 130px 110px 130px;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-1) var(--s-2);
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule);
}
.layer-seq {
  text-align: right;
  font-weight: 700;
}
.layer-desc { color: var(--ink-1); }
.layer-pfd, .layer-credit { color: var(--ink-2); }
.layer-actions { gap: var(--s-1); }

/* 保护层内联表单 */
.layer-form {
  margin-top: var(--s-2);
  padding: var(--s-2) var(--s-3);
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule);
}
.form-row {
  display: grid;
  grid-template-columns: 100px 140px 1fr 130px 130px auto;
  gap: var(--s-2);
  align-items: end;
}
.form-row label {
  display: flex;
  flex-direction: column;
  gap: var(--s-1);
  font-size: var(--fs-micro);
  color: var(--ink-3);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  font-weight: 600;
}
.form-row label input,
.form-row label select {
  font-size: var(--fs-sm);
  text-transform: none;
  letter-spacing: 0;
  font-weight: 400;
}
.form-actions {
  display: flex;
  gap: var(--s-1);
  align-items: center;
}

/* GAP 对照状态标签（IEC 61511-1 Annex E 定级追溯） */
.tag.gap { white-space: nowrap; }
.tag.gap-covered    { background: var(--ok-bg, #e8f5e9); color: var(--ok, #2e7d32); }
.tag.gap-gap        { background: var(--danger-bg, #fdecea); color: var(--danger, #c62828); }
.tag.gap-drift      { background: var(--warn-bg, #fff8e1); color: var(--warn, #f57f17); }
.tag.gap-pending_data { background: var(--paper-3, #eceff1); color: var(--ink-2, #546e7a); }
.tag.gap-unlinked   { background: var(--paper-3, #eceff1); color: var(--ink-2, #78909c); }
.tag.gap-na         { background: var(--paper-3, #eceff1); color: var(--ink-2, #546e7a); }
td .tag + .tag { margin-left: 4px; }
</style>
