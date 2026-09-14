<template>
  <div class="home">
    <!-- 横幅：工程图标题块 -->
    <section class="card hero">
      <div class="hero-grid">
        <div class="hero-text">
          <div class="hero-eyebrow">SECTION 01 · OVERVIEW</div>
          <h2 class="hero-title">SIF Studio</h2>
          <div class="hero-subtitle">联锁工坊 · 面向中小石化自动化工程师的桌面文档工具</div>
          <ul class="hero-points">
            <li>联锁逻辑图绘制（与 M0 编辑器 iframe 嵌入）</li>
            <li>真实仪表台账 · SIF 跨图汇总 · SIL 验算</li>
            <li>服务记录、旁路授权、审计资料导出</li>
          </ul>
          <div class="hero-actions">
            <router-link to="/instruments" class="btn primary">
              {{ auth.isViewer ? "查看仪表台账" : "导入仪表台账" }}
            </router-link>
            <router-link to="/sifs" class="btn">查看 SIF 汇总</router-link>
            <router-link v-if="activeBypassCount > 0" to="/bypass" class="btn">
              旁路台账 ({{ activeBypassCount }})
            </router-link>
            <router-link to="/projects" class="btn">打开项目</router-link>
          </div>
        </div>
        <div class="hero-block">
          <div class="hero-block-row">
            <span class="hb-key">DOC</span>
            <span class="hb-val mono">SIF-STUDIO / OVERVIEW</span>
          </div>
          <div class="hero-block-row">
            <span class="hb-key">REV</span>
            <span class="hb-val mono">A · 2026-09-13</span>
          </div>
          <div class="hero-block-row">
            <span class="hb-key">SCALE</span>
            <span class="hb-val mono">NTS</span>
          </div>
          <div class="hero-block-row">
            <span class="hb-key">SHEET</span>
            <span class="hb-val mono">01 / 01</span>
          </div>
        </div>
      </div>
    </section>

    <!-- 统计 -->
    <section class="stats">
      <div class="panel stat-card">
        <div class="panel-header">仪表主数据 · INSTRUMENT REGISTRY</div>
        <div class="panel-value">{{ store.instruments.length }}</div>
        <div class="panel-foot">
          检测 {{ detectors.length }} · 最终 {{ finals.length }} · 旁路 {{ auxes.length }}
        </div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">项目 · PROJECT INDEX</div>
        <div class="panel-value">{{ store.projects.length }}</div>
        <div class="panel-foot">{{ activeProjects }} 个运行 / 设计中</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">SIF 安全仪表功能 · SIF SUMMARY</div>
        <div class="panel-value">{{ store.sifSummary.length }}</div>
        <div class="panel-foot">跨图汇总 · {{ totalLinks }} 个仪表关联</div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">活动旁路 · ACTIVE BYPASS</div>
        <div class="panel-value" :class="{ alert: bypassAlert }">{{ activeBypassCount }}</div>
        <div class="panel-foot">
          <span v-if="overdueBypassCount > 0" class="overdue-mark">
            {{ overdueBypassCount }} 个逾期 · 最久 {{ formatHours(store.overdueBypassStatus.oldestOverdueHours) }}
          </span>
          <span v-else>限时在用 · IEC 61511 §11.5.2</span>
        </div>
      </div>
      <div class="panel stat-card">
        <div class="panel-header">待办 · TODO</div>
        <div class="panel-value">{{ todos }}</div>
        <div class="panel-foot">SIL 未验算 / 位号未填</div>
      </div>
    </section>

    <!-- SIL 验算 + 最近 SIF -->
    <section class="grid">
      <div class="card">
        <h3 class="card-title">SIL 验算进度 · VERIFICATION</h3>
        <div class="sil-bar">
          <div
            v-for="s in silBreakdown"
            :key="s.label"
            class="sil-seg"
            :class="`sil-${s.label.toLowerCase()}`"
            :style="{ flex: s.count || 0.1 }"
            :title="`SIL ${s.label}: ${s.count}`"
          ></div>
        </div>
        <div class="sil-legend">
          <span v-for="s in silBreakdown" :key="s.label" class="sil-legend-row">
            <span class="sil-swatch" :class="`sil-${s.label.toLowerCase()}`"></span>
            <span class="sil-legend-key">SIL {{ s.label }}</span>
            <span class="sil-legend-val mono">{{ s.count }}</span>
          </span>
        </div>
      </div>

      <div class="card">
        <h3 class="card-title">最近 SIF · RECENT</h3>
        <table v-if="store.sifSummary.length">
          <thead>
            <tr>
              <th>编号</th>
              <th>名称</th>
              <th>SIL</th>
              <th class="num">图</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="s in store.sifSummary.slice(0, 6)" :key="s.id">
              <td class="mono">{{ s.code }}</td>
              <td>{{ s.name }}</td>
              <td><span class="tag" :class="`sil-${s.silVerified.toLowerCase()}`">SIL {{ s.silVerified }}</span></td>
              <td class="num">{{ s.diagramCount }}</td>
            </tr>
          </tbody>
        </table>
        <div v-else class="empty">
          还没 SIF · 去
          <router-link to="/sifs">SIF 汇总</router-link>
          创建第一条
        </div>
      </div>
    </section>

    <!-- 遵循标准 -->
    <section class="card standards">
      <h3 class="card-title">遵循标准 · APPLICABLE STANDARDS</h3>
      <dl class="kv">
        <dt>IEC 61511-1:2016</dt>
        <dd>过程工业功能安全 / SIS 设计、SIL 验算、HFT/PFDavg/CCF、旁路限时+报警</dd>
        <dt>GB/T 21109.1-2007</dt>
        <dd>等同采用 IEC 61511；国内合规</dd>
        <dt>ANSI/ISA 5.1-2024</dt>
        <dd>仪表符号、识别、连线、颜色编码（仪表 bubble、信号线）</dd>
        <dt>GB/T 50770-2013</dt>
        <dd>石油化工安全仪表系统设计规范（中文版 ISA 84）</dd>
        <dt>GB/T 14691-1993 / GB/T 18135-2008</dt>
        <dd>技术制图字体、电气工程 CAD 制图规则（字体、线宽、字高、字符宽高比）</dd>
        <dt>ISO 3098 / ISO 7200 / ISO 5457</dt>
        <dd>技术制图字体、标题栏、图纸幅面（A0-A4）</dd>
      </dl>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useStudioStore } from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const store = useStudioStore();
const auth = useAuthStore();

const detectors = computed(() => store.detectors);
const finals = computed(() => store.finals);
const auxes = computed(() => store.auxes);
const activeProjects = computed(
  () => store.projects.filter((p) => p.phase !== "closed").length,
);
const totalLinks = computed(() =>
  store.sifSummary.reduce(
    (a, s) => a + s.detectorCount + s.finalCount + s.logicCount + s.auxCount,
    0,
  ),
);
const todos = computed(
  () =>
    store.sifSummary.filter((s) => s.silVerified === "NA").length +
    store.instruments.filter((i) => i.tag.startsWith("NEW-")).length,
);
// M2.2 — 旁路统计
const activeBypassCount = computed(() => store.activeBypasses.length);
const overdueBypassCount = computed(() => store.overdueBypasses.length);
const bypassAlert = computed(() => overdueBypassCount.value > 0);

// M2.2 增强：把"X.X h"格式化成"d d h.h h"
function formatHours(h: number | null): string {
  if (h == null) return "—";
  if (h < 24) return h.toFixed(1) + " h";
  const d = Math.floor(h / 24);
  const rem = h - d * 24;
  return d + " d " + rem.toFixed(1) + " h";
}

interface SilBucket {
  label: "NA" | "A" | "B" | "C" | "D";
  count: number;
}
const silBreakdown = computed<SilBucket[]>(() => {
  const labels = ["NA", "A", "B", "C", "D"] as const;
  return labels.map((l) => ({
    label: l,
    count: store.sifSummary.filter((s) => s.silVerified === l).length,
  }));
});
</script>

<style scoped>
.home { display: flex; flex-direction: column; gap: var(--s-4); }

/* ---------- 横幅 ---------- */
.hero {
  border: var(--rule-bold) solid var(--ink-1);
  background: var(--paper);
  padding: 0;
}
.hero-grid {
  display: grid;
  grid-template-columns: 1.8fr 1fr;
  gap: 0;
}
.hero-text {
  padding: var(--s-5);
  border-right: var(--rule-fine) solid var(--rule-2);
}
.hero-eyebrow {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  margin-bottom: var(--s-2);
  text-transform: uppercase;
}
.hero-title {
  font-family: var(--font-title);
  font-size: 28px;
  font-weight: 900;
  letter-spacing: 0.04em;
  color: var(--ink-1);
  margin: 0 0 var(--s-1);
}
.hero-subtitle {
  font-size: var(--fs-md);
  color: var(--ink-2);
  margin-bottom: var(--s-3);
}
.hero-points {
  list-style: none;
  padding: 0;
  margin: 0 0 var(--s-4);
  font-size: var(--fs-sm);
  color: var(--ink-2);
}
.hero-points li {
  padding: var(--s-1) 0 var(--s-1) var(--s-3);
  position: relative;
  border-bottom: var(--rule-hair) solid var(--rule-3);
}
.hero-points li:last-child { border-bottom: 0; }
.hero-points li::before {
  content: "";
  position: absolute;
  left: 0; top: 50%;
  transform: translateY(-50%);
  width: var(--s-2);
  height: 2px;
  background: var(--ink-1);
}
.hero-actions {
  display: flex;
  gap: var(--s-2);
  flex-wrap: wrap;
}
.btn {
  display: inline-block;
  padding: 6px 14px;
  border: var(--rule-mid) solid var(--rule);
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-sm);
  font-weight: 600;
  color: var(--ink-1);
  background: var(--paper);
  text-decoration: none;
  letter-spacing: 0.02em;
}
.btn:hover { background: var(--paper-2); text-decoration: none; border-width: var(--rule-bold); padding: calc(6px - 0.5px) calc(14px - 0.5px); }
.btn.primary {
  background: var(--ink-1);
  color: var(--paper);
  border-color: var(--ink-1);
}
.btn.primary:hover { background: #000; color: var(--paper); }

/* 标题栏方块（右侧 ISO 7200 字段） */
.hero-block {
  padding: var(--s-5);
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
  background: var(--paper-3);
}
.hero-block-row {
  display: grid;
  grid-template-columns: 60px 1fr;
  gap: var(--s-2);
  align-items: baseline;
  padding: var(--s-1) 0;
  border-bottom: var(--rule-hair) solid var(--rule-2);
}
.hero-block-row:last-child { border-bottom: 0; }
.hb-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.hb-val { font-size: var(--fs-sm); color: var(--ink-1); font-weight: 500; }

/* ---------- 统计 ---------- */
.stats {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: var(--s-3);
}
.stat-card { padding: var(--s-3) var(--s-4); }
.stat-card .panel-value.alert {
  color: var(--err);
}
.stat-card .overdue-mark {
  color: var(--err);
  font-weight: 700;
  letter-spacing: 0.02em;
}

/* ---------- SIL 段块 ---------- */
.sil-bar {
  display: flex;
  height: 18px;
  border: var(--rule-fine) solid var(--rule);
  overflow: hidden;
  margin: var(--s-2) 0 var(--s-3);
}
.sil-seg { transition: none; }
.sil-seg.sil-na { background: var(--sil-na); }
.sil-seg.sil-a  { background: var(--sil-a); }
.sil-seg.sil-b  { background: var(--sil-b); }
.sil-seg.sil-c  { background: var(--sil-c); }
.sil-seg.sil-d  { background: var(--sil-d); }
.sil-legend {
  display: grid;
  grid-template-columns: repeat(5, 1fr);
  gap: var(--s-2);
}
.sil-legend-row {
  display: grid;
  grid-template-columns: 12px 1fr auto;
  gap: var(--s-2);
  align-items: center;
  padding: var(--s-1) var(--s-2);
  border: var(--rule-fine) solid var(--rule-2);
  font-size: var(--fs-xs);
}
.sil-swatch {
  width: 12px;
  height: 12px;
  border: var(--rule-fine) solid var(--ink-1);
}
.sil-swatch.sil-na { background: var(--sil-na); }
.sil-swatch.sil-a  { background: var(--sil-a); }
.sil-swatch.sil-b  { background: var(--sil-b); }
.sil-swatch.sil-c  { background: var(--sil-c); }
.sil-swatch.sil-d  { background: var(--sil-d); }
.sil-legend-key { color: var(--ink-2); font-weight: 600; }
.sil-legend-val { font-weight: 700; color: var(--ink-1); }

.grid {
  display: grid;
  grid-template-columns: 1.2fr 1fr;
  gap: var(--s-3);
}
</style>