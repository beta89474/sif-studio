<template>
  <div class="proj">
    <div class="head card">
      <input class="search" v-model="query" placeholder="按编号 / 名称 / 业主搜索…" />
      <button v-if="auth.canWrite" class="primary" @click="openCreate">+ 新建项目</button>
    </div>

    <div v-if="filtered.length" class="grid">
      <article v-for="p in filtered" :key="p.id" class="card proj-card">
        <header class="proj-head">
          <div class="proj-id">
            <div class="proj-code-key">PROJECT · CODE</div>
            <div class="proj-code mono">{{ p.code }}</div>
          </div>
          <div class="proj-title">
            <h3 class="proj-name">{{ p.name }}</h3>
            <span class="phase" :class="`phase-${p.phase}`">{{ phaseCN(p.phase) }}</span>
            <span class="proj-actions">
              <template v-if="auth.canWrite">
                <button class="sm" @click="openEdit(p)">编辑</button>
                <button class="sm danger" @click="remove(p.id, p.code)">删除</button>
              </template>
              <button class="sm" @click="openHistory(p.id)">历史</button>
            </span>
          </div>
        </header>

        <dl class="kv meta">
          <dt>业主 · CLIENT</dt><dd>{{ p.client || "—" }}</dd>
          <dt>位置 · LOC</dt><dd>{{ p.location || "—" }}</dd>
          <dt>开始 · START</dt><dd class="mono">{{ format(p.startedAt) || "—" }}</dd>
          <dt>结束 · END</dt><dd class="mono">{{ format(p.finishedAt) || "—" }}</dd>
          <dt v-if="p.notes" class="full">备注 · NOTE</dt>
          <dd v-if="p.notes" class="full">{{ p.notes }}</dd>
        </dl>

        <section class="diagrams">
          <header class="diagram-head">
            <span class="dh-title">联锁图 · DIAGRAMS</span>
            <span class="dh-count mono">{{ diagramsOf(p.id).length }}</span>
            <button v-if="auth.canWrite" class="sm" @click="newDiagram(p.id)">+ 新建图</button>
          </header>
          <ul v-if="diagramsOf(p.id).length">
            <li v-for="d in diagramsOf(p.id)" :key="d.id">
              <router-link :to="`/diagram/${d.id}`" class="diagram-link">
                <span class="dl-key">DWG</span>
                <span class="mono dl-code">{{ d.code }}</span>
                <span class="dl-name">{{ d.name }}</span>
                <span class="dl-size mono">{{ d.sheetSize }} · {{ d.revision }}</span>
                <span v-if="d.sifCode" class="tag final">{{ d.sifCode }}</span>
                <span v-if="d.sifSilDesign" class="tag" :class="`sil-${d.sifSilDesign.toLowerCase()}`">SIL {{ d.sifSilDesign }}</span>
              </router-link>
            </li>
          </ul>
          <div v-else class="muted">暂无图纸，点击「+ 新建图」加一张</div>
        </section>
      </article>
    </div>
    <div v-else class="empty">
      还没有项目。可点击「新建项目」或等启动时自动创建默认项目。
    </div>

    <!-- 修改历史抽屉（M2.4） -->
    <EntityHistoryDrawer entity-type="project" />

    <!-- 新建 / 编辑项目弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="closeEditor">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing ? "编辑项目 · EDIT" : "新建项目 · NEW PROJECT" }}</span>
          <span class="ref">{{ editing ? `PRJ-${editing}` : "FORM-PRJ-01" }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label>编号 · CODE <input v-model="form.code" placeholder="PRJ-002" :disabled="!!editing" /></label>
            <label class="full">名称 · NAME <input v-model="form.name" placeholder="催化裂化装置反应再生部分" /></label>
            <label>业主 · CLIENT <input v-model="form.client" /></label>
            <label>位置 · LOC <input v-model="form.location" /></label>
            <label>阶段 · PHASE
              <select v-model="form.phase">
                <option value="design">设计 DESIGN</option>
                <option value="construction">建设 CONSTRUCTION</option>
                <option value="commissioning">调试 COMMISSIONING</option>
                <option value="operation">运营 OPERATION</option>
                <option value="closed">归档 CLOSED</option>
              </select>
            </label>
            <label class="full">备注 · NOTE <textarea v-model="form.notes" rows="3"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">ISO 7200 · 8 FIELDS</span>
          <button @click="closeEditor">取消</button>
          <button class="primary" :disabled="!form.code || !form.name" @click="save">{{ editing ? "保存" : "保存" }}</button>
        </div>
      </div>
    </div>

    <!-- 新建 diagram 弹窗 -->
    <div v-if="newDgm" class="modal-mask" @click.self="newDgm = null">
      <div class="modal">
        <div class="modal-header">
          <span>新建联锁图 · NEW DIAGRAM</span>
          <span class="ref">PROJECT-{{ newDgm }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label>图纸编号 · DWG NO <input v-model="newDiagramForm.code" placeholder="DGM-002" /></label>
            <label class="full">图名 · TITLE <input v-model="newDiagramForm.name" placeholder="…" /></label>
            <label>图幅 · SHEET
              <select v-model="newDiagramForm.sheetSize">
                <option value="A0">A0 · 1189×841</option>
                <option value="A1">A1 · 841×594</option>
                <option value="A2">A2 · 594×420</option>
                <option value="A3">A3 · 420×297</option>
                <option value="A4">A4 · 297×210</option>
              </select>
            </label>
            <label>版次 · REV <input v-model="newDiagramForm.revision" /></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">ISO 5457</span>
          <button @click="newDgm = null">取消</button>
          <button class="primary" :disabled="!newDiagramForm.code || !newDiagramForm.name" @click="saveDiagram">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { useStudioStore, type DiagramSummary, type Project } from "../stores/studio";
import { useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";
import EntityHistoryDrawer from "../components/EntityHistoryDrawer.vue";

const store = useStudioStore();
const auth = useAuthStore();
const router = useRouter();

const query = ref("");

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  if (!q) return store.projects;
  return store.projects.filter(
    (p) =>
      p.code.toLowerCase().includes(q) ||
      p.name.toLowerCase().includes(q) ||
      p.client.toLowerCase().includes(q),
  );
});

const PHASE_CN: Record<string, string> = {
  design: "设计 DESIGN",
  construction: "建设 CONST",
  commissioning: "调试 COMM",
  operation: "运营 OPER",
  closed: "归档 CLOSED",
};
function phaseCN(p: string) {
  return PHASE_CN[p] ?? p;
}
function format(s: string) {
  if (!s) return "";
  return s.split(" ")[0];
}

const diagramsCache = ref<Record<number, DiagramSummary[]>>({});
async function loadDiagrams(pid: number) {
  // 用 in 区分"未加载"和"加载过但为空"（cascade 删除后会撞上，truthy 判断把
  // 空数组当作"已加载"→ 永远显示 0 张图）
  if (pid in diagramsCache.value) return;
  diagramsCache.value[pid] = await store.listDiagrams(pid);
}
function diagramsOf(pid: number): DiagramSummary[] {
  return diagramsCache.value[pid] ?? [];
}

onMounted(async () => {
  if (!store.projects.length) {
    await store.ensureDefaultProject();
  }
  for (const p of store.projects) {
    loadDiagrams(p.id);
  }
});

const creating = ref(false);
const editing = ref<number | null>(null);
const form = reactive({
  code: "",
  name: "",
  client: "",
  location: "",
  phase: "design",
  notes: "",
});
function openCreate() {
  Object.assign(form, {
    code: "",
    name: "",
    client: "",
    location: "",
    phase: "design",
    notes: "",
  });
  editing.value = null;
  creating.value = true;
}
function openEdit(p: Project) {
  Object.assign(form, {
    code: p.code,
    name: p.name,
    client: p.client,
    location: p.location,
    phase: p.phase,
    notes: p.notes,
  });
  editing.value = p.id;
  creating.value = true;
}
function openHistory(id: number) {
  store.openEntityHistory("project", id);
}
function closeEditor() {
  creating.value = false;
  editing.value = null;
}
async function remove(id: number, code: string) {
  if (!confirm(`确认删除项目「${code}」？此操作不可撤销，相关 SIF / 图 / 旁路 / 工单会一并清空（CASCADE）。`)) return;
  await store.deleteProject(id);
}
async function save() {
  const payload = {
    code: form.code,
    name: form.name,
    client: form.client,
    location: form.location,
    phase: form.phase,
    finishedAt: "",
    notes: form.notes,
  };
  if (editing.value !== null) {
    await store.updateProject(editing.value, payload);
  } else {
    const p = await store.createProject(payload);
    if (p) loadDiagrams(p.id);
  }
  creating.value = false;
  editing.value = null;
}

const newDgm = ref<number | null>(null);
const newDiagramForm = reactive({
  code: "",
  name: "",
  sheetSize: "A1",
  revision: "A0",
});
function newDiagram(pid: number) {
  Object.assign(newDiagramForm, { code: "", name: "", sheetSize: "A1", revision: "A0" });
  newDgm.value = pid;
}
async function saveDiagram() {
  if (!newDgm.value) return;
  const d = await store.createDiagram({
    projectId: newDgm.value,
    code: newDiagramForm.code,
    name: newDiagramForm.name,
    sheetSize: newDiagramForm.sheetSize,
    revision: newDiagramForm.revision,
  });
  newDgm.value = null;
  diagramsCache.value[d.projectId] = await store.listDiagrams(d.projectId);
  router.push(`/diagram/${d.id}`);
}
</script>

<style scoped>
.proj { display: flex; flex-direction: column; gap: var(--s-3); }
.head { display: flex; gap: var(--s-2); align-items: center; }
.search { flex: 1; }

.grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: var(--s-3); }

/* 项目卡：标题栏化 */
.proj-card { display: flex; flex-direction: column; gap: var(--s-3); padding: 0; }
.proj-head {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: var(--s-3);
  padding: var(--s-3) var(--s-4);
  background: var(--paper-3);
  border-bottom: var(--rule-mid) solid var(--ink-1);
}
.proj-id { display: flex; flex-direction: column; gap: 2px; }
.proj-code-key {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.proj-code {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}
.proj-title {
  display: flex;
  align-items: center;
  gap: var(--s-3);
}
.proj-name {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
  margin: 0;
  flex: 1;
}

/* 阶段标签（方块化） */
.phase {
  padding: 2px 8px;
  border: var(--rule-fine) solid currentColor;
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-micro);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  border-radius: var(--r-0);
}
.phase-design         { color: var(--acc);    background: var(--acc-bg);   border-color: var(--acc); }
.phase-construction   { color: var(--sil-b);  background: var(--sil-b-bg); border-color: var(--sil-b); }
.phase-commissioning  { color: var(--reset);  background: #f0eafb;          border-color: var(--reset); }
.phase-operation      { color: var(--ok);     background: var(--ok-bg);    border-color: var(--ok); }
.phase-closed         { color: var(--ink-3);  background: var(--paper-3);  border-color: var(--ink-3); }

/* 项目卡操作按钮组 */
.proj-actions {
  display: inline-flex;
  gap: var(--s-1);
  margin-left: auto;
  flex-shrink: 0;
}

/* 元数据 */
.meta { padding: 0 var(--s-4); }

/* 图纸列表 */
.diagrams {
  margin: 0 var(--s-4) var(--s-4);
  border-top: var(--rule-fine) solid var(--rule-2);
  padding-top: var(--s-3);
}
.diagram-head {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  margin-bottom: var(--s-2);
}
.dh-title {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.dh-count {
  font-weight: 700;
  color: var(--ink-1);
  padding: 0 var(--s-2);
  border: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
}
.diagram-head button { margin-left: auto; }

.diagrams ul {
  list-style: none;
  padding: 0;
  margin: 0;
  border: var(--rule-fine) solid var(--rule-2);
}
.diagrams li {
  border-bottom: var(--rule-fine) solid var(--rule-2);
}
.diagrams li:last-child { border-bottom: 0; }

.diagram-link {
  display: grid;
  grid-template-columns: 40px 110px 1fr auto auto auto;
  gap: var(--s-2);
  align-items: center;
  padding: var(--s-2) var(--s-3);
  color: var(--ink-1);
  text-decoration: none;
  font-size: var(--fs-sm);
  border-left: 4px solid transparent;
}
.diagram-link:hover {
  background: var(--paper-2);
  border-left-color: var(--acc);
  text-decoration: none;
}
.dl-key {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
}
.dl-code {
  font-weight: 600;
  color: var(--ink-1);
}
.dl-name { color: var(--ink-2); }
.dl-size {
  font-size: var(--fs-xs);
  color: var(--ink-3);
  padding: 1px 6px;
  background: var(--paper-3);
  border: var(--rule-fine) solid var(--rule-2);
}
</style>