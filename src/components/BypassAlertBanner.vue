<!--
  BypassAlertBanner.vue —— 全站旁路逾期告警横幅（M2.2 增强）

  设计原则：
  - 全站最高优先级警告（IEC 61511-1 §11.5.2 合规缺口不能静默）
  - count = 0 → 不渲染（避免噪音）
  - 整条横幅可点击 → 跳 BypassLedger 页面
  - 视觉：琥珀色 1px 黑边底色（与工业图纸告警块对齐）
  - 不引入 Modal / Toast 库 —— 用一个 fixed 顶部条
  - 数字用 tabular-nums 等宽，避免抖动
-->
<script setup lang="ts">
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useStudioStore } from "../stores/studio";
import { AlertTriangle, X } from "lucide-vue-next";

const store = useStudioStore();
const router = useRouter();

const show = computed(() => store.overdueBypassStatus.count > 0);
const count = computed(() => store.overdueBypassStatus.count);
const oldestHours = computed(() => store.overdueBypassStatus.oldestOverdueHours);

function formatHours(h: number | null): string {
  if (h == null) return "—";
  if (h < 24) return h.toFixed(1) + " h";
  const d = Math.floor(h / 24);
  const rem = h - d * 24;
  return d + " d " + rem.toFixed(1) + " h";
}

const severityLabel = computed(() => {
  const h = oldestHours.value ?? 0;
  if (h >= 24) return "严重 · CRITICAL";
  if (h >= 8) return "警告 · WARNING";
  return "关注 · ADVISORY";
});

const severityClass = computed(() => {
  const h = oldestHours.value ?? 0;
  if (h >= 24) return "sev-critical";
  if (h >= 8) return "sev-warning";
  return "sev-advisory";
});

function goLedger() {
  router.push("/bypass");
}

function dismiss() {
  // 临时关闭到下次轮询（60s）—— 合规上只是 UI 隐藏，后端状态不变
  // 真正的"已确认"应在 BypassLedger 里恢复/记录
  store.overdueBypassStatus = { count: 0, oldestOverdueHours: null };
}
</script>

<template>
  <Transition name="banner-fade">
    <div v-if="show" class="bypass-alert" :class="severityClass" role="alert" aria-live="assertive">
      <div class="banner-grid">
        <div class="banner-icon">
          <AlertTriangle :size="20" />
        </div>
        <div class="banner-body">
          <div class="banner-head">
            <span class="banner-title">{{ severityLabel }}</span>
            <span class="banner-sep">·</span>
            <span class="banner-count">{{ count }} 条旁路已逾期</span>
          </div>
          <div class="banner-detail">
            最久逾期 <span class="tabular">{{ formatHours(oldestHours) }}</span>
            <span class="muted"> · IEC 61511-1 §11.5.2 合规缺口</span>
          </div>
        </div>
        <div class="banner-actions">
          <button type="button" class="btn primary" @click="goLedger">
            前往 BypassLedger
          </button>
          <button
            type="button"
            class="btn icon-only"
            aria-label="临时关闭（60s 后再次提醒）"
            title="临时关闭（60s 后再次提醒）"
            @click="dismiss"
          >
            <X :size="16" />
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
/* ---- 工业图纸告警块：琥珀底 + 黑边 + 硬角 ---- */
.bypass-alert {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  z-index: 1000;
  border-bottom: 1.5px solid var(--ink-1, #0a0a0a);
  background: var(--warn-fill, #fef3c7);
  color: var(--ink-1, #0a0a0a);
  font-family: var(--ff-sans, "ISOCPEUR", "Arial Narrow", "Microsoft YaHei", sans-serif);
  user-select: none;
  cursor: pointer;
}
/* 严重（≥24h）= 实色警告 + 红边 */
.sev-critical {
  background: var(--fin-fill, #fee2e2);
  border-bottom-color: var(--err-stroke, #b91c1c);
  color: var(--err-stroke, #b91c1c);
}
.sev-critical .banner-icon { color: var(--err-stroke, #b91c1c); }

/* 警告（≥8h）= 琥珀 */
.sev-warning {
  background: var(--warn-fill, #fef3c7);
  border-bottom-color: var(--warn-stroke, #b45309);
  color: var(--warn-stroke, #b45309);
}
.sev-warning .banner-icon { color: var(--warn-stroke, #b45309); }

/* 关注（<8h）= 淡琥珀 */
.sev-advisory {
  background: #fffbeb;
  border-bottom-color: var(--ink-3, #525252);
  color: var(--ink-2, #262626);
}

.banner-grid {
  display: grid;
  grid-template-columns: 32px 1fr auto;
  align-items: center;
  gap: var(--s-3, 12px);
  padding: var(--s-2, 8px) var(--s-4, 16px);
  max-width: 1600px;
  margin: 0 auto;
}

.banner-icon {
  display: flex;
  align-items: center;
  justify-content: center;
}

.banner-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.banner-head {
  display: flex;
  align-items: baseline;
  gap: 6px;
  flex-wrap: wrap;
}
.banner-title {
  font-family: var(--ff-mono, "Consolas", "MONOTXT", monospace);
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.05em;
  text-transform: uppercase;
}
.banner-sep {
  color: var(--ink-4, #737373);
}
.banner-count {
  font-family: var(--ff-mono, monospace);
  font-size: 13px;
  font-weight: 600;
}
.banner-detail {
  font-size: 12px;
  color: var(--ink-3, #525252);
}
.tabular {
  font-family: var(--ff-mono, monospace);
  font-variant-numeric: tabular-nums;
  font-weight: 600;
  color: inherit;
}
.muted { color: var(--ink-4, #737373); }

.banner-actions {
  display: flex;
  align-items: center;
  gap: var(--s-2, 8px);
}

.icon-only {
  padding: 4px;
  min-width: 24px;
  height: 24px;
  background: transparent;
  border: 1px solid currentColor;
  color: inherit;
  cursor: pointer;
  border-radius: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
}
.icon-only:hover {
  background: rgba(0, 0, 0, 0.08);
}

/* ---- 入场动画：fade + 下滑 4px ---- */
.banner-fade-enter-active,
.banner-fade-leave-active {
  transition: opacity 180ms ease, transform 180ms ease;
}
.banner-fade-enter-from,
.banner-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

/* 横幅 fixed 会盖住 App 顶栏；main 加 padding-top 抵消 */
:global(.has-bypass-alert .app-header) {
  margin-top: 56px;
}
</style>
