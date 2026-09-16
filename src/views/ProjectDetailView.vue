<template>
  <div class="pd">
    <!-- 项目头：ISO 7200 风格标题栏 -->
    <header v-if="project" class="pd-head card">
      <div class="pd-id">
        <div class="pd-code-key">PROJECT · CODE</div>
        <div class="pd-code mono">{{ project.code }}</div>
      </div>
      <div class="pd-title">
        <h3 class="pd-name">{{ project.name }}</h3>
        <span class="phase" :class="`phase-${project.phase}`">{{ phaseCN(project.phase) }}</span>
        <span class="pd-actions">
          <button class="sm" @click="openHistory(project.id)">历史</button>
        </span>
      </div>
      <dl class="pd-meta">
        <div class="pd-meta-item">
          <dt>业主 · CLIENT</dt>
          <dd>{{ project.client || "—" }}</dd>
        </div>
        <div class="pd-meta-item">
          <dt>位置 · LOC</dt>
          <dd>{{ project.location || "—" }}</dd>
        </div>
        <div class="pd-meta-item">
          <dt>开始 · START</dt>
          <dd class="mono">{{ format(project.startedAt) || "—" }}</dd>
        </div>
        <div class="pd-meta-item">
          <dt>阶段 · PHASE</dt>
          <dd>{{ phaseCN(project.phase) }}</dd>
        </div>
      </dl>
      <p v-if="project.notes" class="pd-notes">{{ project.notes }}</p>
    </header>
    <div v-else class="empty">加载项目中…</div>

    <!-- Tab 切换（查询参数保活，可深链 / 刷新） -->
    <nav class="pd-tabs">
      <button
        v-for="t in TABS"
        :key="t.key"
        class="pd-tab"
        :class="{ active: tab === t.key }"
        @click="setTab(t.key)"
      >
        <span class="pd-tab-key">{{ t.no }}</span>
        <span>{{ t.label }}</span>
      </button>
    </nav>

    <!-- Tab 内容：单项目锁定模式 -->
    <InstrumentsPanel v-if="tab === 'instruments'" :project-id="projectId" />
    <AlarmsPanel v-else-if="tab === 'alarms'" :project-id="projectId" />
    <DiagramsPanel v-else-if="tab === 'diagrams'" :project-id="projectId" />
    <SifPanel v-else-if="tab === 'sifs'" :project-id="projectId" />
    <ImportPanel v-else-if="tab === 'import'" :project-id="projectId" />

    <!-- 修改历史抽屉（M2.4） -->
    <EntityHistoryDrawer entity-type="project" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { onBeforeRouteLeave, useRoute, useRouter } from "vue-router";
import { useStudioStore } from "../stores/studio";
import EntityHistoryDrawer from "../components/EntityHistoryDrawer.vue";
import InstrumentsPanel from "../components/InstrumentsPanel.vue";
import AlarmsPanel from "../components/AlarmsPanel.vue";
import DiagramsPanel from "../components/DiagramsPanel.vue";
import SifPanel from "../components/SifPanel.vue";
import ImportPanel from "../components/ImportPanel.vue";

/**
 * ProjectDetailView — 项目详情页（项目中心化导航）
 *
 * #/projects/:id?tab=instruments|alarms|diagrams|sifs|import
 * 各 Tab 面板均以锁定模式挂载（projectId 必传），内容天然只属于该项目。
 */
const props = defineProps<{ projectId: number }>();

const TABS = [
  { key: "instruments", no: "01", label: "仪表台账" },
  { key: "alarms", no: "02", label: "报警台账" },
  { key: "diagrams", no: "03", label: "联锁逻辑图" },
  { key: "sifs", no: "04", label: "SIF 汇总" },
  { key: "import", no: "05", label: "数据导入" },
] as const;
type TabKey = (typeof TABS)[number]["key"];

const store = useStudioStore();
const route = useRoute();
const router = useRouter();

const project = computed(
  () => store.projects.find((p) => p.id === props.projectId) ?? null,
);

// 项目不存在 → 回项目列表（如深链到已删项目、或项目被删除后仍停留在本页）
const checked = ref(false);
onMounted(async () => {
  if (!store.projects.length) {
    await store.refreshProjects();
  }
  checked.value = true;
  if (!project.value) {
    router.replace({ name: "projects" });
  }
});
watch(project, (p) => {
  if (checked.value && !p) router.replace({ name: "projects" });
});

// Tab 状态 ↔ 查询参数（非法/缺省值回落到 instruments）
const tab = computed<TabKey>(() => {
  const t = route.query.tab;
  return TABS.some((x) => x.key === t) ? (t as TabKey) : "instruments";
});
function setTab(key: TabKey) {
  router.replace({ query: { ...route.query, tab: key === "instruments" ? undefined : key } });
}

// 进入导入 Tab → 预置目标项目（锁定模式导入不再手选）
watch(
  tab,
  (t) => {
    if (t === "import") store.setImportTargetProjectId(props.projectId);
  },
  { immediate: true },
);
// 离开详情页（切 Tab 不触发，路由级钩子只在真正离开时调用）→ 清掉，防泄漏到全局 /import
onBeforeRouteLeave(() => {
  store.setImportTargetProjectId(null);
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
function openHistory(id: number) {
  store.openEntityHistory("project", id);
}
</script>

<style scoped>
.pd { display: flex; flex-direction: column; gap: var(--s-3); }

/* ─── 项目头 ─── */
.pd-head {
  display: grid;
  grid-template-columns: 120px 1fr;
  gap: var(--s-3) var(--s-4);
  padding: var(--s-3) var(--s-4);
  background: var(--paper-3);
  border-left: var(--rule-mid) solid var(--ink-1);
}
.pd-id { display: flex; flex-direction: column; gap: 2px; }
.pd-code-key {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.pd-code {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
}
.pd-title {
  display: flex;
  align-items: center;
  gap: var(--s-3);
}
.pd-name {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
  margin: 0;
  flex: 1;
}
.pd-actions {
  display: inline-flex;
  gap: var(--s-1);
  flex-shrink: 0;
}

/* 阶段标签（与项目列表一致） */
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

/* 元数据条（跨两列铺满） */
.pd-meta {
  grid-column: 1 / -1;
  display: flex;
  gap: var(--s-5);
  margin: 0;
  padding: 0;
}
.pd-meta-item dt {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-3);
  letter-spacing: 0.14em;
  text-transform: uppercase;
}
.pd-meta-item dd {
  margin: 2px 0 0;
  font-size: var(--fs-sm);
  color: var(--ink-1);
}
.pd-notes {
  grid-column: 1 / -1;
  margin: 0;
  font-size: var(--fs-sm);
  color: var(--ink-2);
}

/* ─── Tab 条（工程图目录页风格） ─── */
.pd-tabs {
  display: flex;
  gap: 0;
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  overflow-x: auto;
}
.pd-tab {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-4);
  border-right: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  color: var(--ink-2);
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-sm);
  cursor: pointer;
  white-space: nowrap;
}
.pd-tab:hover { background: var(--paper-2); color: var(--ink-1); }
.pd-tab.active {
  background: var(--ink-1);
  color: var(--paper);
}
.pd-tab-key {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.14em;
  opacity: 0.65;
}
</style>
