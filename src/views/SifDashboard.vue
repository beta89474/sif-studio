<template>
  <div class="sif">
    <!-- 统计 -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">检测元件 · DETECTOR</div>
        <div class="panel-value">{{ totalDetector }}</div>
        <div class="panel-foot">全库汇总</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">最终元件 · FINAL</div>
        <div class="panel-value">{{ totalFinal }}</div>
        <div class="panel-foot">安全动作</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">旁路关联 · AUX</div>
        <div class="panel-value">{{ totalAux }}</div>
        <div class="panel-foot">需限时 + 报警</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">SIF 数 · COUNT</div>
        <div class="panel-value">{{ filtered.length }}</div>
        <div class="panel-foot">跨图汇总</div>
      </div>
    </section>

    <!-- 过滤 -->
    <section class="head card">
      <input class="search" v-model="query" placeholder="按编号 / 名称搜索…" />
      <select v-model="silFilter">
        <option value="">全部 SIL</option>
        <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
      </select>
      <select v-model="projectFilter">
        <option value="">全部项目</option>
        <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
      </select>
      <button v-if="auth.canWrite" class="primary" @click="openCreate">+ 新建 SIF</button>
    </section>

    <!-- 列表 -->
    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 32px"></th>
          <th style="width: 120px">编号 · CODE</th>
          <th>名称 · NAME</th>
          <th style="width: 80px">SIL</th>
          <th class="num" style="width: 60px">图</th>
          <th class="num" style="width: 60px">检测</th>
          <th class="num" style="width: 60px">最终</th>
          <th class="num" style="width: 60px">旁路</th>
          <th style="width: 140px">操作</th>
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
            <td class="mono">{{ s.code }}</td>
            <td>{{ s.name }}</td>
            <td><span class="tag" :class="`sil-${s.silVerified.toLowerCase()}`">{{ s.silVerified }}</span></td>
            <td class="num">{{ s.diagramCount }}</td>
            <td class="num">{{ s.detectorCount }}</td>
            <td class="num">{{ s.finalCount }}</td>
            <td class="num">{{ s.auxCount }}</td>
            <td class="row-actions">
              <button v-if="auth.canWrite" class="sm" @click="openEdit(s)">编辑</button>
              <button class="sm" @click="openHistory(s.id)">历史</button>
            </td>
          </tr>
          <tr v-if="expanded === s.id" class="detail-row">
            <td colspan="8">
              <div class="detail">
                <!-- 元数据 -->
                <div class="detail-block">
                  <h4>元数据 · META</h4>
                  <dl class="kv">
                    <dt>项目</dt><dd class="mono">{{ s.projectCode }}</dd>
                    <dt>设计 SIL</dt><dd class="mono">SIL {{ s.silDesign }}</dd>
                    <dt>需求模式</dt><dd>{{ s.demandMode }}</dd>
                    <dt>PFDavg 目标</dt><dd class="mono">{{ s.pfdavgTarget ?? "—" }}</dd>
                    <dt>检验周期</dt><dd class="mono">{{ s.proofInterval }} 月</dd>
                    <dt>说明</dt><dd class="full">{{ s.description || "—" }}</dd>
                  </dl>
                </div>

                <!-- 检测位号（ISA 5.1 bubble 风格） -->
                <div class="detail-block">
                  <h4>检测位号 · DETECTOR</h4>
                  <div class="chips">
                    <span v-for="t in s.detectorsCsv.split(',').filter(Boolean)" :key="t" class="bubble detector mono">{{ t }}</span>
                    <span v-if="!s.detectorsCsv" class="muted">未关联</span>
                  </div>
                </div>

                <!-- 最终元件 -->
                <div class="detail-block">
                  <h4>最终元件 · FINAL</h4>
                  <div class="chips">
                    <span v-for="t in s.finalsCsv.split(',').filter(Boolean)" :key="t" class="bubble final mono">{{ t }}</span>
                    <span v-if="!s.finalsCsv" class="muted">未关联</span>
                  </div>
                </div>

                <!-- 出现位置 -->
                <div class="detail-block">
                  <h4>出现位置 · LOC</h4>
                  <div class="chips">
                    <span v-for="t in s.diagramsCsv.split(',').filter(Boolean)" :key="t" class="chip-loc mono">{{ t }}</span>
                    <span v-if="!s.diagramsCsv" class="muted">未挂图</span>
                  </div>
                </div>

                <!-- 已关联仪表 -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>已关联的仪表 · LINKED INSTRUMENTS</h4>
                    <button v-if="auth.canWrite" class="sm primary" @click="openLink(s.id)">+ 关联仪表</button>
                  </div>
                  <table v-if="sifLinks.length" class="links">
                    <thead>
                      <tr>
                        <th>位号</th><th>角色</th><th>类型</th><th>SIL</th><th>备注</th><th></th>
                      </tr>
                    </thead>
                    <tbody>
                      <tr v-for="l in sifLinks" :key="l.id">
                        <td class="mono">{{ l.tag }}</td>
                        <td><span class="tag" :class="l.role">{{ roleCN(l.role) }}</span></td>
                        <td class="mono">{{ l.kind }}</td>
                        <td><span class="tag" :class="`sil-${l.silTarget.toLowerCase()}`">{{ l.silTarget }}</span></td>
                        <td>{{ l.note || "—" }}</td>
                        <td v-if="auth.canWrite" class="row-actions"><button class="danger sm" @click="unlink(l.id, s.id)">解除</button></td>
                        <td v-else></td>
                      </tr>
                    </tbody>
                  </table>
                  <div v-else class="muted">还没有任何关联，点击上方按钮加一条。</div>
                </div>

                <!-- 活动旁路（M2.2 IEC 61511-1 §11.5.2） -->
                <div class="detail-block full">
                  <div class="block-head">
                    <h4>活动旁路 · ACTIVE BYPASS</h4>
                    <router-link to="/bypass" class="block-link">登记新旁路 →</router-link>
                  </div>
                  <BypassCard :sif-id="s.id" :max="5" />
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
    <div v-else class="empty">
      还没 SIF 记录。请新建一条 SIF（如 SIF-201），再从仪表台账里挑真实位号绑上去 —— 这样生成的台账才能直接出审计资料。
    </div>

    <!-- 修改历史抽屉（M2.4） -->
    <EntityHistoryDrawer entity-type="sif" />

    <!-- 新建 / 编辑 SIF 弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="closeEditor">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing ? "编辑 SIF · EDIT" : "新建 SIF · NEW" }}</span>
          <span class="ref">{{ editing ? `SIF-${editing}` : "FORM-SIF-01" }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label class="full">项目 · PROJECT
              <select v-model="form.projectId">
                <option v-for="p in store.projects" :key="p.id" :value="p.id">{{ p.code }} {{ p.name }}</option>
              </select>
            </label>
            <label>SIF 编号 · CODE <input v-model="form.code" placeholder="SIF-201" /></label>
            <label>SIF 名称 · NAME <input v-model="form.name" placeholder="反应器超压联锁" /></label>
            <label>设计 SIL · SIL DESIGN
              <select v-model="form.silDesign">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label>验证 SIL · SIL VERIFIED
              <select v-model="form.silVerified">
                <option v-for="s in ['NA','A','B','C','D']" :key="s" :value="s">SIL {{ s }}</option>
              </select>
            </label>
            <label>需求模式 · DEMAND
              <select v-model="form.demandMode">
                <option value="low">低需求 LOW</option>
                <option value="high">高需求 HIGH</option>
              </select>
            </label>
            <label>PFDavg 目标 · PFD <input type="number" step="1e-7" v-model.number="form.pfdavgTarget" /></label>
            <label>检验周期(月) · TI <input type="number" v-model.number="form.proofInterval" /></label>
            <label class="full">说明 · NOTE <textarea v-model="form.description" rows="3"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">IEC 61511-1 §11.5</span>
          <button @click="closeEditor">取消</button>
          <button class="primary" :disabled="!form.code || !form.name || !form.projectId" @click="saveNow">保存</button>
        </div>
      </div>
    </div>

    <!-- 关联仪表弹窗 -->
    <div v-if="linking" class="modal-mask" @click.self="linking = null">
      <div class="modal">
        <div class="modal-header">
          <span>关联仪表 · LINK</span>
          <span class="ref">SIF-{{ linking.sifId }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label class="full">仪表位号 · INSTRUMENT
              <select v-model="linkForm.instrumentId">
                <option value="">—</option>
                <option v-for="i in store.instruments" :key="i.id" :value="i.id">
                  {{ i.tag }}（{{ roleCN(i.role) }} · SIL {{ i.silTarget }}）
                </option>
              </select>
            </label>
            <label>角色 · ROLE
              <select v-model="linkForm.role">
                <option value="detector">检测 DETECTOR</option>
                <option value="final">最终 FINAL</option>
                <option value="logic">逻辑 LOGIC</option>
                <option value="aux">旁路 AUX</option>
              </select>
            </label>
            <label>端口 · PORT <input type="number" v-model.number="linkForm.portIndex" :min="1" /></label>
            <label class="full">备注 · NOTE <input v-model="linkForm.note" /></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-LINK-01</span>
          <button @click="linking = null">取消</button>
          <button class="primary" :disabled="!linkForm.instrumentId" @click="linkNow">关联</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, reactive, ref, onMounted } from "vue";
import { ChevronRight, ChevronDown } from "lucide-vue-next";
import { useStudioStore, type SifLink, type SifSummary } from "../stores/studio";
import { useAuthStore } from "../stores/auth";
import BypassCard from "../components/BypassCard.vue";
import EntityHistoryDrawer from "../components/EntityHistoryDrawer.vue";

const store = useStudioStore();
const auth = useAuthStore();

onMounted(async () => {
  // 旁路台账预先拉（详情展开时 BypassCard 直接读 store）
  if (store.bypasses.length === 0) {
    await store.refreshBypasses("all");
  }
});

const query = ref("");
const silFilter = ref("");
const projectFilter = ref("");

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return store.sifSummary.filter((s) => {
    if (silFilter.value && s.silVerified !== silFilter.value) return false;
    if (projectFilter.value && String(s.projectId) !== projectFilter.value) return false;
    if (!q) return true;
    return s.code.toLowerCase().includes(q) || s.name.toLowerCase().includes(q);
  });
});

const totalDetector = computed(() => store.sifSummary.reduce((a, s) => a + s.detectorCount, 0));
const totalFinal = computed(() => store.sifSummary.reduce((a, s) => a + s.finalCount, 0));
const totalAux = computed(() => store.sifSummary.reduce((a, s) => a + s.auxCount, 0));

const ROLE_CN: Record<string, string> = {
  detector: "检测",
  final: "最终",
  logic: "逻辑",
  aux: "旁路",
};
function roleCN(r: string) {
  return ROLE_CN[r] ?? r;
}

const expanded = ref<number | null>(null);
const sifLinks = ref<SifLink[]>([]);
async function toggle(id: number) {
  if (expanded.value === id) {
    expanded.value = null;
    sifLinks.value = [];
    return;
  }
  expanded.value = id;
  sifLinks.value = await store.listSifLinks(id);
}

async function unlink(linkId: number, sifId: number) {
  if (!confirm("确认解除该仪表与 SIF 的关联？")) return;
  await store.unlinkInstrument(linkId);
  sifLinks.value = await store.listSifLinks(sifId);
}

const creating = ref<boolean | null>(false);
const editing = ref<number | null>(null);
const form = reactive({
  projectId: 0,
  code: "",
  name: "",
  description: "",
  silDesign: "NA",
  silVerified: "NA",
  demandMode: "low",
  pfdavgTarget: null as number | null,
  proofInterval: 12,
});
function openCreate() {
  form.projectId = store.projects[0]?.id ?? 0;
  form.code = "";
  form.name = "";
  form.description = "";
  form.silDesign = "NA";
  form.silVerified = "NA";
  form.demandMode = "low";
  form.pfdavgTarget = null;
  form.proofInterval = 12;
  editing.value = null;
  creating.value = true;
}
async function openEdit(s: SifSummary) {
  form.projectId = s.projectId;
  form.code = s.code;
  form.name = s.name;
  form.description = s.description || "";
  form.silDesign = s.silDesign;
  form.silVerified = s.silVerified;
  form.demandMode = s.demandMode;
  form.pfdavgTarget = s.pfdavgTarget;
  form.proofInterval = s.proofInterval;
  editing.value = s.id;
  creating.value = true;
}
function closeEditor() {
  creating.value = null;
  editing.value = null;
}
function openHistory(id: number) {
  store.openEntityHistory("sif", id);
}
async function saveNow() {
  if (!form.code || !form.name) return;
  if (!form.projectId) {
    await store.ensureDefaultProject();
  }
  const payload = {
    projectId: form.projectId,
    code: form.code,
    name: form.name,
    description: form.description,
    silDesign: form.silDesign,
    silVerified: form.silVerified,
    demandMode: form.demandMode,
    pfdavgTarget: form.pfdavgTarget,
    proofInterval: form.proofInterval,
  };
  if (editing.value !== null) {
    await store.updateSif(editing.value, payload);
  } else {
    await store.createSif({
      ...payload,
      projectId:
        form.projectId || (await store.ensureDefaultProject()).id,
    });
  }
  creating.value = false;
  editing.value = null;
}

interface Linking {
  sifId: number | null;
}
const linking = ref<Linking | null>(null);
const linkForm = reactive({
  instrumentId: 0,
  role: "detector",
  portIndex: 1,
  note: "",
});
function openLink(sifId: number) {
  linkForm.instrumentId = 0;
  linkForm.role = "detector";
  linkForm.portIndex = 1;
  linkForm.note = "";
  linking.value = { sifId };
}
async function linkNow() {
  if (!linking.value || !linkForm.instrumentId) return;
  await store.linkInstrument({
    sifId: linking.value.sifId!,
    instrumentId: linkForm.instrumentId,
    role: linkForm.role,
    portIndex: linkForm.portIndex,
    note: linkForm.note,
  });
  if (expanded.value) {
    sifLinks.value = await store.listSifLinks(expanded.value);
  }
  linking.value = null;
}
</script>

<style scoped>
.sif { display: flex; flex-direction: column; gap: var(--s-3); }
.stats { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--s-3); }
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
  grid-template-columns: 1fr 1fr 1fr 1fr;
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
.block-link {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--acc);
  text-decoration: none;
  letter-spacing: 0.04em;
}
.block-link:hover { text-decoration: underline; }

.chips { display: flex; flex-wrap: wrap; gap: var(--s-1); align-items: center; }
.chip-loc {
  display: inline-block;
  padding: 2px 8px;
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  background: var(--paper);
  color: var(--ink-1);
  border: var(--rule-fine) solid var(--rule);
  font-weight: 500;
}

table.links {
  border: var(--rule-fine) solid var(--rule);
  margin-top: var(--s-2);
}
table.links thead th { background: var(--paper-3); }

.row-actions { display: flex; gap: var(--s-1); justify-content: flex-end; }
</style>