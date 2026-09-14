<script setup lang="ts">
/**
 * BypassCard.vue — M2.2 嵌入式旁路卡
 *
 * 给 SifDashboard.vue / Home.vue 用：
 *   - 列出某 SIF 下所有未恢复（active / overdue）旁路
 *   - 显示状态徽章 + 剩余/逾期时间
 *   - 单击跳 /bypass 查看
 */
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useStudioStore } from "../stores/studio";
import type { BypassListItem } from "../stores/studio";

const props = defineProps<{
  /** 指定 SIF 下过滤；null = 全局活动旁路 */
  sifId?: number | null;
  /** 显示数量上限（默认 5） */
  max?: number;
}>();

const store = useStudioStore();
const router = useRouter();

const items = computed<BypassListItem[]>(() => {
  let list: BypassListItem[];
  if (props.sifId !== undefined && props.sifId !== null) {
    list = store.bypasses.filter(
      (b) => b.sifId === props.sifId && (b.status === "active" || b.status === "overdue"),
    );
  } else {
    list = store.activeBypasses.concat(store.overdueBypasses);
  }
  return list.slice(0, props.max ?? 5);
});

const hasAny = computed(() => items.value.length > 0);

function formatHours(h: number): string {
  if (h >= 24) return `${(h / 24).toFixed(1)} 天`;
  if (h >= 0) return `${h.toFixed(1)} h`;
  const d = Math.floor(-h / 24);
  const r = -h - d * 24;
  return d > 0 ? `逾期 ${d}d ${r.toFixed(0)}h` : `逾期 ${r.toFixed(1)}h`;
}

function goLedger() {
  router.push("/bypass");
}
</script>

<template>
  <div v-if="hasAny" class="bypass-card">
    <div class="head">
      <span class="head-key">旁路 · BYPASS</span>
      <span class="head-count mono">{{ items.length }}</span>
      <span class="head-action">
        <a href="#" @click.prevent="goLedger">查看全部 →</a>
      </span>
    </div>
    <div class="rows">
      <div
        v-for="b in items"
        :key="b.id"
        class="row"
        :class="b.status"
      >
        <span class="status-pill" :class="b.status">
          {{ b.status === 'active' ? '活动' : '逾期' }}
        </span>
        <span class="sif mono">{{ b.sifCode }}</span>
        <span class="hours mono">{{ formatHours(b.hoursToRestore) }}</span>
        <span class="by">{{ b.bypassedBy }}</span>
        <span class="reason" :title="b.reason">{{ b.reason }}</span>
      </div>
    </div>
  </div>
  <div v-else class="bypass-card empty-card">
    <div class="head">
      <span class="head-key">旁路 · BYPASS</span>
      <span class="head-count mono">0</span>
    </div>
    <div class="empty-msg">无活动旁路</div>
  </div>
</template>

<style scoped>
.bypass-card {
  border: var(--rule-mid) solid var(--rule);
  background: var(--paper);
  padding: 0;
}
.head {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-3);
  border-bottom: var(--rule-fine) solid var(--rule);
  background: var(--paper-3);
}
.head-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.head-count {
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
  font-variant-numeric: tabular-nums;
}
.head-action {
  margin-left: auto;
  font-size: var(--fs-xs);
}

.rows {
  display: flex;
  flex-direction: column;
}
.row {
  display: grid;
  grid-template-columns: 56px 90px 100px 90px 1fr;
  gap: var(--s-2);
  align-items: center;
  padding: var(--s-2) var(--s-3);
  border-bottom: var(--rule-fine) solid var(--rule-3);
  font-size: var(--fs-xs);
}
.row:last-child { border-bottom: 0; }
.row.overdue { background: var(--err-bg); }
.status-pill {
  display: inline-block;
  padding: 1px 6px;
  font-family: var(--font-mono);
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.04em;
  border: var(--rule-fine) solid currentColor;
  text-transform: uppercase;
  text-align: center;
}
.status-pill.active  { color: var(--warn); background: var(--warn-bg); border-color: var(--warn); }
.status-pill.overdue { color: var(--err);  background: var(--err-bg);  border-color: var(--err); }
.sif { font-weight: 700; color: var(--ink-1); }
.hours { font-weight: 700; color: var(--ink-2); font-variant-numeric: tabular-nums; }
.row.overdue .hours { color: var(--err); }
.by { color: var(--ink-2); }
.reason {
  color: var(--ink-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.empty-card .empty-msg {
  padding: var(--s-3);
  color: var(--ink-4);
  font-size: var(--fs-xs);
  text-align: center;
  font-family: var(--font-mono);
  letter-spacing: 0.04em;
}
</style>