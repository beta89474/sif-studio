<script setup lang="ts">
/**
 * DiagramsPanel — 项目联锁图面板（从 ProjectsView 卡片图纸区块迁入）
 *
 * 仅在项目详情页使用：projectId 必填，图纸列表与新建图弹窗都锁定该项目。
 */
import { ref, watch } from "vue";
import { useRouter } from "vue-router";
import { useStudioStore, type DiagramSummary } from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const props = defineProps<{ projectId: number }>();

const store = useStudioStore();
const auth = useAuthStore();
const router = useRouter();

const list = ref<DiagramSummary[]>([]);
async function load() {
  list.value = await store.listDiagrams(props.projectId);
}
// 切换项目（理论少见）或首次挂载都重新加载
watch(() => props.projectId, load, { immediate: true });

const creating = ref<number | null>(null);
const form = ref({
  code: "",
  name: "",
  sheetSize: "A1",
  revision: "A0",
});
function openNew() {
  form.value = { code: "", name: "", sheetSize: "A1", revision: "A0" };
  creating.value = props.projectId;
}
async function save() {
  if (!creating.value || !form.value.code || !form.value.name) return;
  const d = await store.createDiagram({
    projectId: props.projectId,
    code: form.value.code,
    name: form.value.name,
    sheetSize: form.value.sheetSize,
    revision: form.value.revision,
  });
  creating.value = null;
  await load();
  router.push(`/diagram/${d.id}`);
}
</script>

<template>
  <div class="dg-panel">
    <section class="diagrams">
      <header class="diagram-head">
        <span class="dh-title">联锁图 · DIAGRAMS</span>
        <span class="dh-count mono">{{ list.length }}</span>
        <button v-if="auth.canWrite" class="sm" @click="openNew">+ 新建图</button>
      </header>
      <ul v-if="list.length">
        <li v-for="d in list" :key="d.id">
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

    <!-- 新建 diagram 弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="creating = null">
      <div class="modal">
        <div class="modal-header">
          <span>新建联锁图 · NEW DIAGRAM</span>
          <span class="ref">PROJECT-{{ creating }}</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label>图纸编号 · DWG NO <input v-model="form.code" placeholder="DGM-002" /></label>
            <label class="full">图名 · TITLE <input v-model="form.name" placeholder="…" /></label>
            <label>图幅 · SHEET
              <select v-model="form.sheetSize">
                <option value="A0">A0 · 1189×841</option>
                <option value="A1">A1 · 841×594</option>
                <option value="A2">A2 · 594×420</option>
                <option value="A3">A3 · 420×297</option>
                <option value="A4">A4 · 297×210</option>
              </select>
            </label>
            <label>版次 · REV <input v-model="form.revision" /></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">ISO 5457</span>
          <button @click="creating = null">取消</button>
          <button class="primary" :disabled="!form.code || !form.name" @click="save">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.diagrams {
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
