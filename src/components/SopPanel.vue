<template>
  <div class="sop">
    <!-- 统计 -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">规程数 · SOP</div>
        <div class="panel-value">{{ store.proofTestSops.length }}</div>
        <div class="panel-foot">成文检验规程</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">被引用 · IN USE</div>
        <div class="panel-value">{{ usedCount }}</div>
        <div class="panel-foot">至少 1 条检验记录引用</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">引用次数 · REFERENCES</div>
        <div class="panel-value">{{ totalRefs }}</div>
        <div class="panel-foot">检验记录关联总数</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">标准 · CLAUSE</div>
        <div class="panel-value">§16.2.2</div>
        <div class="panel-foot">IEC 61511-1</div>
      </div>
    </section>

    <!-- 过滤 -->
    <section class="head card">
      <input class="search" v-model="query" placeholder="按编号 / 标题 / 文档号搜索…" />
      <button v-if="auth.canWrite" class="primary" @click="openCreate">
        <Plus :size="14" /> 新建规程
      </button>
    </section>

    <!-- 列表 -->
    <table v-if="filtered.length">
      <thead>
        <tr>
          <th style="width: 32px"></th>
          <th style="width: 130px">编号 · CODE</th>
          <th>名称 · TITLE</th>
          <th style="width: 80px">版本 · VER</th>
          <th style="width: 140px">文档号 · DOC REF</th>
          <th style="width: 160px">适用范围 · SCOPE</th>
          <th class="num" style="width: 80px">引用 · USE</th>
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
            <td>{{ s.title }}</td>
            <td class="mono">{{ s.version }}</td>
            <td class="mono">{{ s.docRef || "—" }}</td>
            <td>{{ s.scope || "—" }}</td>
            <td class="num">
              <span class="tag" :class="s.usageCount > 0 ? 'vs-verified' : 'vs-pending'">{{ s.usageCount }}</span>
            </td>
            <td class="row-actions">
              <button v-if="auth.canWrite" class="sm" @click="openEdit(s)">编辑</button>
              <button v-if="auth.canWrite" class="sm danger" @click="onDelete(s)">删除</button>
            </td>
          </tr>
          <tr v-if="expanded === s.id" class="detail-row">
            <td colspan="8">
              <div class="detail">
                <div class="detail-block full">
                  <h4>成文规程 · PROCEDURE（IEC 61511-1 §16.2.2）</h4>
                  <dl class="kv">
                    <dt>检验方法与步骤</dt>
                    <dd class="full method">{{ s.testMethod || "—" }}</dd>
                    <dt>通过判据</dt>
                    <dd class="full">{{ s.passCriteria || "—" }}</dd>
                    <dt>备注 · NOTES</dt>
                    <dd class="full">{{ s.notes || "—" }}</dd>
                  </dl>
                </div>
              </div>
            </td>
          </tr>
        </template>
      </tbody>
    </table>
    <div v-else class="empty">
      还没有检验规程。按 IEC 61511-1 §16.2.2，功能测试应依据成文规程执行（步骤 / 通过判据 / 文档编号）。
      点击"新建规程"创建第一条（如 SOP-MECH-007 关断阀全行程测试）。
    </div>

    <!-- 新建 / 编辑弹窗 -->
    <div v-if="creating" class="modal-mask" @click.self="closeEditor">
      <div class="modal">
        <div class="modal-header">
          <span>{{ editing !== null ? "编辑规程 · EDIT" : "新建规程 · NEW" }}</span>
          <span class="ref">IEC 61511-1 §16.2.2</span>
        </div>
        <div class="modal-body">
          <div class="form">
            <label>编号 · CODE <input v-model="form.code" placeholder="SOP-MECH-007" /></label>
            <label>版本 · VERSION <input v-model="form.version" placeholder="v1.0" /></label>
            <label class="full">名称 · TITLE <input v-model="form.title" placeholder="气动关断阀全行程测试规程" /></label>
            <label>文档号 · DOC REF <input v-model="form.docRef" placeholder="QMS 文档编号" /></label>
            <label>适用范围 · SCOPE <input v-model="form.scope" placeholder="气动关断阀 / 变送器" /></label>
            <label class="full">检验方法与步骤 · METHOD
              <textarea v-model="form.testMethod" rows="5" placeholder="1. 隔离工艺&#10;2. 全行程动作&#10;3. 记录动作时间"></textarea>
            </label>
            <label class="full">通过判据 · PASS CRITERIA
              <textarea v-model="form.passCriteria" rows="2" placeholder="行程到位且动作时间 ≤ 5s"></textarea>
            </label>
            <label class="full">备注 · NOTES <textarea v-model="form.notes" rows="2"></textarea></label>
          </div>
        </div>
        <div class="modal-footer">
          <span class="ref">FORM-SOP-01</span>
          <button @click="closeEditor">取消</button>
          <button class="primary" :disabled="!form.code.trim() || !form.title.trim() || !form.version?.trim()" @click="saveNow">保存</button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import { ChevronRight, ChevronDown, Plus } from "lucide-vue-next";
import { useStudioStore, type ProofTestSop, type ProofTestSopInput } from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const store = useStudioStore();
const auth = useAuthStore();

onMounted(async () => {
  await store.refreshProofTestSops();
});

const query = ref("");
const expanded = ref<number | null>(null);

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase();
  return store.proofTestSops.filter((s) => {
    if (!q) return true;
    return (
      s.code.toLowerCase().includes(q) ||
      s.title.toLowerCase().includes(q) ||
      s.docRef.toLowerCase().includes(q)
    );
  });
});

const usedCount = computed(() => store.proofTestSops.filter((s) => s.usageCount > 0).length);
const totalRefs = computed(() => store.proofTestSops.reduce((a, s) => a + s.usageCount, 0));

function toggle(id: number) {
  expanded.value = expanded.value === id ? null : id;
}

// ---- 编辑弹窗 ----
const creating = ref(false);
const editing = ref<number | null>(null);
const form = reactive<ProofTestSopInput>({
  code: "",
  title: "",
  version: "v1.0",
  docRef: "",
  scope: "",
  testMethod: "",
  passCriteria: "",
  notes: "",
});

function resetForm() {
  form.code = "";
  form.title = "";
  form.version = "v1.0";
  form.docRef = "";
  form.scope = "";
  form.testMethod = "";
  form.passCriteria = "";
  form.notes = "";
}

function openCreate() {
  editing.value = null;
  resetForm();
  creating.value = true;
}

function openEdit(s: ProofTestSop) {
  editing.value = s.id;
  form.code = s.code;
  form.title = s.title;
  form.version = s.version;
  form.docRef = s.docRef;
  form.scope = s.scope;
  form.testMethod = s.testMethod;
  form.passCriteria = s.passCriteria;
  form.notes = s.notes;
  creating.value = true;
}

function closeEditor() {
  creating.value = false;
  editing.value = null;
}

async function saveNow() {
  if (editing.value !== null) {
    await store.updateProofTestSop(editing.value, { ...form });
  } else {
    await store.createProofTestSop({ ...form });
  }
  closeEditor();
}

async function onDelete(s: ProofTestSop) {
  if (s.usageCount > 0) {
    const ok = window.confirm(
      `规程 ${s.code} 正被 ${s.usageCount} 条检验记录引用。\n删除将解除这些记录的规程关联（检验历史保留）。确认删除？`,
    );
    if (!ok) return;
    await store.deleteProofTestSop(s.id, true);
  } else {
    if (!window.confirm(`确认删除规程 ${s.code}？`)) return;
    await store.deleteProofTestSop(s.id, false);
  }
}
</script>

<style scoped>
.sop { display: flex; flex-direction: column; gap: var(--s-3); }

.stats {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: var(--s-2);
}

.head {
  display: flex;
  gap: var(--s-2);
  align-items: center;
  padding: var(--s-2);
}
.search { flex: 1; min-width: 200px; }

table { width: 100%; border-collapse: collapse; }
th, td {
  text-align: left;
  padding: 6px 10px;
  border-bottom: var(--rule-fine) solid var(--rule);
  font-size: var(--fs-sm);
  vertical-align: middle;
}
th {
  background: var(--paper-2);
  font-weight: 600;
  color: var(--ink-2);
  white-space: nowrap;
}
tr.active { background: var(--paper-2); }
.num { text-align: right; }
.mono { font-family: var(--mono); }
.muted { color: var(--ink-3); }
.row-actions { display: flex; gap: 4px; }

.detail-row td { background: var(--paper-1); padding: 0; }
.detail { padding: var(--s-2) var(--s-3); display: flex; flex-direction: column; gap: var(--s-2); }
.detail-block h4 {
  margin: 0 0 var(--s-1);
  font-size: var(--fs-sm);
  letter-spacing: 0.05em;
  color: var(--ink-2);
}
dl.kv {
  display: grid;
  grid-template-columns: 130px 1fr;
  gap: 4px 12px;
  margin: 0;
}
dl.kv dt { color: var(--ink-3); font-size: var(--fs-sm); }
dl.kv dd { margin: 0; grid-column: 2; }
dl.kv dd.full { grid-column: 2; }
dl.kv .method { white-space: pre-wrap; }

.empty {
  padding: var(--s-4);
  text-align: center;
  color: var(--ink-3);
  border: var(--rule-fine) dashed var(--rule);
}

/* modal */
.modal-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.35);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.modal {
  background: var(--paper);
  width: min(680px, 92vw);
  max-height: 88vh;
  display: flex;
  flex-direction: column;
  border: var(--rule-fine) solid var(--rule);
}
.modal-header {
  display: flex;
  justify-content: space-between;
  padding: var(--s-2) var(--s-3);
  border-bottom: var(--rule-fine) solid var(--rule);
  font-weight: 600;
}
.modal-body { padding: var(--s-3); overflow-y: auto; }
.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  border-top: var(--rule-fine) solid var(--rule);
}
.ref { color: var(--ink-3); font-family: var(--mono); font-size: var(--fs-xs); margin-right: auto; }
.form {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--s-2);
}
.form label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: var(--fs-sm);
  color: var(--ink-2);
}
.form label.full { grid-column: 1 / -1; }
.form input, .form select, .form textarea {
  padding: 6px 8px;
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  font: inherit;
}
.form textarea { resize: vertical; font-family: var(--mono); font-size: var(--fs-sm); }

button.primary { background: var(--ink); color: var(--paper); border: none; padding: 6px 14px; cursor: pointer; }
button.primary:disabled { opacity: 0.4; cursor: not-allowed; }
button.danger { color: var(--danger); }
.sm { padding: 2px 8px; font-size: var(--fs-sm); }
.icon { background: none; border: none; cursor: pointer; padding: 2px; color: var(--ink-2); }
</style>
