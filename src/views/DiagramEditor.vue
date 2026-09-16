<template>
  <div class="editor-page">
    <!-- 顶栏：ISO 7200 标题栏（diagram 字段） -->
    <header class="ed-head">
      <router-link to="/projects" class="back">← 项目</router-link>
      <div v-if="diagram" class="ed-title">
        <span class="ed-key">DWG · {{ diagram.code }}</span>
        <span class="ed-name">{{ diagram.name }}</span>
        <span class="ed-meta mono">· {{ diagram.sheetSize }} · REV {{ diagram.revision }}</span>
      </div>
      <div v-if="loadNote" class="ed-note mono">{{ loadNote }}</div>
      <div v-if="auth.isViewer" class="ed-note mono" title="只读角色不能保存修改">VIEWER · 只读预览</div>
      <button
        v-if="conflict"
        class="back"
        type="button"
        @click="reloadLatest"
        :disabled="saving"
      >载入最新版本</button>
      <div class="ed-actions">
        <div class="ed-status" :class="{ ok: saved, dirty: dirty, save: saving, error: saveError }">
          <span class="status-dot"></span>
          <span class="status-text mono">{{ statusLabel }}</span>
        </div>
        <button v-if="auth.canWrite" class="primary" @click="saveData" :disabled="!dirty || saving">保存到数据库</button>
      </div>
    </header>

    <div class="ed-body">
      <iframe
        v-if="diagram"
        ref="iframeEl"
        :src="editorSrc"
        @load="bindIframe"
      ></iframe>
      <div v-else class="placeholder">
        <div class="placeholder-mark">[ EMPTY ]</div>
        <div>联锁图编辑器加载失败，请确认 <code>public/editor.html</code> 是否存在。</div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute } from "vue-router";
import { RpcError } from "../api/rpc";
import { useStudioStore, type Diagram, type Instrument } from "../stores/studio";
import { useAuthStore } from "../stores/auth";

const store = useStudioStore();
const auth = useAuthStore();
const route = useRoute();

const diagram = ref<Diagram | null>(null);
const ready = ref(false);
const dirty = ref(false);
const saved = ref(true);
const saving = ref(false);
const saveError = ref(false);
/** 装载提示：库中快照不可用、已回退到内置示例图时，给用户一句明确说明 */
const loadNote = ref("");
/** B5 — 409 冲突态：他人已先保存，本地 version 过期 */
const conflict = ref(false);
const iframeEl = ref<HTMLIFrameElement | null>(null);
let messageHandler: ((ev: MessageEvent) => void) | null = null;
/** 当前 save-response 等不到的 Promise resolve —— saveData 用于 await */
let pendingSaveResolve: ((data: string | null) => void) | null = null;
/** save-response 超时兜底，避免 M0 意外断连时永久挂起 */
const SAVE_TIMEOUT_MS = 3000;

const diagramId = computed(() => Number(route.params.diagramId));

const editorSrc = computed(() => {
  if (!diagram.value) return "";
  // 注：M0 编辑器不读 URL hash（桥全走 postMessage），这里只用时间戳防缓存
  return `/editor.html?diagram=${diagram.value.id}&v=${Date.now()}`;
});

// LOADING → READY → DIRTY → SAVING → SAVED / ERROR 状态机文案
const statusLabel = computed(() => {
  if (!ready.value) return "LOADING";
  if (saving.value) return "SAVING…";
  if (saveError.value) return "ERROR";
  if (dirty.value) return "DIRTY";
  if (saved.value) return "SAVED";
  return "READY";
});

onMounted(async () => {
  // 立即注册消息监听（不等 iframe @load —— editor.html IIFE 同步发的
  // post('ready') 可能早于 load 事件，否则消息丢失）
  bindMessageListener();
  await loadDiagramIntoView();
});

// 路由参数变化（图 A → 图 B）：
// 同一个 route record 会复用组件实例，onMounted 不会再跑，
// 缺了这一步就会「换了 URL 但画布还是上一张图」。
watch(
  () => route.params.diagramId,
  (next, prev) => {
    if (next === prev) return;
    void loadDiagramIntoView();
  },
);

/** 装载当前 diagramId 对应的图。
 *  先把 diagram 置空 → iframe 随 v-if 卸载，旧编辑器的消息通道随之断开，
 *  避免上一张图的 save-response / dirty 落到新图的状态机上。 */
async function loadDiagramIntoView() {
  ready.value = false;
  saved.value = true;
  dirty.value = false;
  saveError.value = false;
  conflict.value = false;
  loadNote.value = "";
  pendingSaveResolve = null;
  diagram.value = null;
  const d = await store.loadDiagram(diagramId.value);
  diagram.value = d;
}

/** B5 — 409 后丢弃本地副本，重新拉取最新图（iframe 重建，画布回到库中版本） */
async function reloadLatest() {
  if (saving.value) return;
  await loadDiagramIntoView();
}

onBeforeUnmount(() => {
  if (messageHandler) {
    window.removeEventListener("message", messageHandler);
    messageHandler = null;
  }
  // 保存中用户切走 → 让悬挂 Promise 落空，避免 resolve 后还改一个已卸的 ref
  if (pendingSaveResolve) {
    pendingSaveResolve(null);
    pendingSaveResolve = null;
  }
});

// 注册消息监听（与 iframe load 时序解耦 —— editor.html 的
// post('ready') 在 IIFE 末尾同步发出，可能早于 @load 事件触发）
const bindMessageListener = () => {
  if (messageHandler) {
    window.removeEventListener("message", messageHandler);
  }
  messageHandler = (ev: MessageEvent) => {
    const data = ev.data as { source?: string; type?: string; payload?: unknown };
    if (!data || data.source !== "sif-editor") return;
    if (!iframeEl.value) return;

    if (data.type === "ready") {
      // 编辑器初始化完成 → 把最新 diagram 发过去
      // 顺序：先发 load-diagram（同步），再异步拉项目仪表并发 inject-instruments
      // 仪表台账是图选择起因/最终元件的事实来源，没有它 picker 就退化成内置 demo。
      iframeEl.value.contentWindow?.postMessage(
        {
          source: "sif-studio",
          type: "load-diagram",
          payload: { id: diagram.value?.id, data: diagram.value?.data },
        },
        "*",
      );
      void pushProjectInstrumentsToIframe();
    } else if (data.type === "loaded") {
      // 编辑器装载完成。applied=true → 库里的快照已生效（干净态）；
      // applied=false → 库中没有可用快照，编辑器保留了内置示例图作为起点。
      //   这份示例还没落库，所以必须标成 DIRTY 并给出说明，让用户明确保存。
      ready.value = true;
      const p = data.payload as
        | { applied?: boolean; reason?: string; blocks?: number }
        | undefined;
      if (p?.applied === true) {
        saved.value = true;
        dirty.value = false;
        loadNote.value = "";
      } else {
        saved.value = false;
        dirty.value = true;
        loadNote.value =
          p?.reason === "invalid-snapshot"
            ? "库中快照格式不可用，已载入示例图 · 尚未保存"
            : p?.reason === "legacy-empty"
              ? "这张图的存档是空的（修复前的旧数据），已载入示例图 · 尚未保存"
              : "尚无保存内容，已载入示例图 · 尚未保存";
      }
    } else if (data.type === "dirty") {
      // 编辑器内任意修改 → DIRTY
      dirty.value = true;
      saved.value = false;
    } else if (data.type === "save-response") {
      // 编辑器响应 request-save → 把最新 JSON 写回 diagram 并 resolve 等数据的 Promise
      const payload = data.payload as { data?: string };
      const fresh = payload?.data ?? null;
      if (fresh && diagram.value) {
        diagram.value.data = fresh;
      }
      if (pendingSaveResolve) {
        const r = pendingSaveResolve;
        pendingSaveResolve = null;
        r(fresh);
      }
    }
  };
  window.addEventListener("message", messageHandler);
};

// @load 回调（仅用于 HMR / 路由切换时的二次绑定）
function bindIframe() {
  bindMessageListener();
}

/**
 * M2.9 — 把当前 diagram 所属项目的仪表台账注入 M0 编辑器。
 *  - 异步：必须先 await refreshInstrumentsByProject(pid)，否则 store.instruments 是上次的脏数据
 *  - 容错：拉取失败 → 静默（编辑器会保留内置示例，不会让画图页崩）
 *  - 二次发送：同一个 iframe 收第二次 inject-instruments 直接覆盖 INSTRUMENT_DB + renderInst()，
 *    路由切换走 loadDiagramIntoView 重建 iframe，所以这个函数只对「当前 iframe」调用一次
 */
async function pushProjectInstrumentsToIframe() {
  if (!iframeEl.value || !iframeEl.value.contentWindow) return;
  const d = diagram.value;
  if (!d || !d.projectId) {
    // diagram 没归属项目（理论上不会发生，create_diagram 已 require）→ 用空列表明确发
    iframeEl.value.contentWindow.postMessage(
      {
        source: "sif-studio",
        type: "inject-instruments",
        payload: { projectId: null, items: [], groups: [] },
      },
      "*",
    );
    return;
  }
  try {
    // 刷新 store（覆盖该项目分片；其他项目的旧数据保留在 store.instruments 里）
    await store.refreshInstrumentsByProject(d.projectId);
    const list = store.instrumentsByProject(d.projectId);
    // ★ Vue 的 reactive proxy 含不可序列化的内部 slot，postMessage 的 structured clone
    //   会抛「could not be cloned」。深拷一层纯 JS 对象出去 —— 编辑器收的就是
    //   JSON.parse 往返后的纯净数据，对得上。
    const plainList = JSON.parse(JSON.stringify(list));
    const groups = buildInstrumentGroups(plainList);
    iframeEl.value.contentWindow.postMessage(
      {
        source: "sif-studio",
        type: "inject-instruments",
        payload: { projectId: d.projectId, items: plainList, groups },
      },
      "*",
    );
  } catch (e: any) {
    // 静默：失败也要发一份空集，让编辑器至少能继续画（picker 退化成内置 demo）
    if (import.meta.env.DEV) {
      console.error("pushProjectInstrumentsToIframe 失败：", e);
    }
    iframeEl.value.contentWindow.postMessage(
      {
        source: "sif-studio",
        type: "inject-instruments",
        payload: { projectId: d.projectId, items: [], groups: [] },
      },
      "*",
    );
  }
}

/**
 * M2.9 — 把 store 里的 Instrument[] 构造成 M0 editor.html 期望的分组格式。
 *  - 四种角色全部进 picker：detector / final / logic / aux
 *  - 按 kind 简单分桶，桶内按 tag 升序
 *  - 把 SQL Instrument 字段映射到 M0 块类型（type 字段）：
 *      detector: 变送器/测量 → ai、开关/火焰/位置 → di、按钮 → hs、系统命令 → comm
 *      final: 阀类 → xv、电机 → motor、电磁阀 → sov、报警 → alarm、其余 → do
 *      logic: 表决器 → vote、与门 → and、或门 → or、非门 → not、锁存 → rs、计时 → ton
 *      aux: 旁路 → bypass、允许 → perm、首出 → firstout、复位 → reset
 *  - M0 picker 只在「检测元件」列（col=0）展开；其余列走通用块，
 *    这里把四种角色都投出去，让用户能在搜索框找到；列选择器不展开但单点照样能 addFromDb。
 */
interface PickerItem {
  id: number | null;
  tag: string;
  desc: string;
  type: string;
  contact: string;
  loc: string;
  unit: string;
}

function buildInstrumentGroups(list: Instrument[]): { g: string; list: PickerItem[] }[] {
  // 四种角色全部进 picker
  const filtered = list || [];
  const buckets = new Map<string, PickerItem[]>();
  filtered
    .slice()
    .sort((a, b) => a.tag.localeCompare(b.tag))
    .forEach((ins) => {
      const t = mapInstrumentToType(ins);
      if (!t) return; // 未知角色/类型跳过
      const label = kindGroupLabel(ins.kind, t);
      const arr = buckets.get(label) ?? [];
      arr.push({
        // id 透传给 M0 —— M2.9 #3 的强外键（block.instrumentId）靠它回写
        id: ins.id,
        tag: ins.tag,
        desc: ins.service || ins.notes || "",
        type: t,
        contact: contactFor(ins),
        loc: ins.role === "logic" ? "机柜室" : ins.role === "aux" ? "机柜室" : "现场",
        unit: ins.unit || "",
      });
      buckets.set(label, arr);
    });
  return Array.from(buckets.entries()).map(([g, l]) => ({ g, list: l }));
}

function mapInstrumentToType(ins: Instrument): string | null {
  const k = String(ins.kind || "").toLowerCase();
  const tag = String(ins.tag || "").toUpperCase();
  if (ins.role === "detector") {
    if (/变送器|transmitter|测量|传感器|sensor/.test(k)) return "ai";
    if (/热电偶|热电阻|rtd|tc/.test(k)) return "ai";
    if (/开关|switch|变送/.test(k)) return "di";
    if (/按钮|手操|hand|按键/.test(k)) return "hs";
    if (/系统|esd|急停|命令/.test(k)) return "comm";
    // tag 前缀兜底（覆盖 kind 命名不规范的情况）
    if (/^(PT|TT|FT|LT|AT|VT|JT|QT|TE|TC|RTD)/.test(tag)) return "ai";
    if (/^(PS|TS|LS|FS|BS|ZS|GS|XS|YS|PSLL|PSHH|TSLL|TSHH|LSLL|LSHH|FSLL|FSHH)/.test(tag)) return "di";
    if (/^HS/.test(tag)) return "hs";
    if (/^ESD/.test(tag)) return "comm";
    return "di"; // 检测元件兜底
  }
  if (ins.role === "final") {
    if (/切断阀|闸阀|球阀|阀|valve/.test(k)) return "xv";
    if (/电机|马达|motor|泵|pump/.test(k)) return "motor";
    if (/电磁阀|solenoid/.test(k)) return "sov";
    if (/报警|喇叭|声光|alarm/.test(k)) return "alarm";
    if (/模拟输出|analog/.test(k)) return "ao";
    // tag 前缀兜底
    if (/^XV|^SDV|^FV|^HV|^TV/.test(tag)) return "xv";
    if (/^M-|^K-|^P-/.test(tag)) return "motor";
    if (/^SOV/.test(tag)) return "sov";
    if (/^UA|^XY|^HA/.test(tag)) return "alarm";
    return "do"; // 最终元件兜底
  }
  if (ins.role === "logic") {
    // 逻辑求解器：继电器/表决/比较/逻辑组合/时序/锁存
    if (/表决|投票|voting|m.{1,2}n/.test(k)) return "vote";
    if (/比较|compare|cmp/.test(k)) return "cmp";
    if (/与门|and/.test(k)) return "and";
    if (/或门|or/.test(k)) return "or";
    if (/非门|not/.test(k)) return "not";
    if (/异或|xor/.test(k)) return "xor";
    if (/与非|nand/.test(k)) return "nand";
    if (/或非|nor/.test(k)) return "nor";
    if (/允许|permit|perm/.test(k)) return "perm";
    if (/锁存|latch|保持|rs锁存|sr锁存/.test(k)) return "rs";
    if (/延时|定时|timer|ton|tof/.test(k)) return "ton";
    if (/计数|count|counter/.test(k)) return "ctu";
    if (/首出|first.?out/.test(k)) return "firstout";
    // tag 前缀兜底
    if (/^AND/i.test(tag)) return "and";
    if (/^OR/i.test(tag)) return "or";
    if (/^NOT/i.test(tag)) return "not";
    if (/^XOR/i.test(tag)) return "xor";
    if (/^RS[-_]?/i.test(tag)) return "rs";
    if (/^TON[-_]?/i.test(tag)) return "ton";
    if (/^CTU[-_]?/i.test(tag)) return "ctu";
    if (/^FO[-_]?/i.test(tag)) return "firstout";
    return "and"; // 逻辑元件兜底为与门
  }
  if (ins.role === "aux") {
    // 旁路/允许设备：维护旁路、允许条件、首出记录、手动复位
    if (/旁路|旁通|bypass/.test(k)) return "bypass";
    if (/允许|permit|perm/.test(k)) return "perm";
    if (/首出|first.?out/.test(k)) return "firstout";
    if (/复位|reset/.test(k)) return "reset";
    // tag 前缀兜底
    if (/^BYP/i.test(tag)) return "bypass";
    if (/^PERM/i.test(tag)) return "perm";
    if (/^FO[-_]?/i.test(tag)) return "firstout";
    if (/^RST[-_]?/i.test(tag)) return "reset";
    if (/^HS[-_]?R/i.test(tag)) return "reset";
    return "bypass"; // 旁路设备兜底
  }
  return null;
}

function kindGroupLabel(kind: string, type: string): string {
  // 优先用 SQL kind（用户已分类），否则用 type 兜底标签
  const k = String(kind || "").trim();
  if (k) return k;
  const fallback: Record<string, string> = {
    ai: "变送器 / AI",
    di: "开关 / DI",
    hs: "手操按钮",
    comm: "系统命令",
    alarm: "声光报警",
    xv: "切断阀",
    motor: "电机",
    sov: "电磁阀",
    do: "数字输出",
    ao: "模拟输出",
    // 逻辑求解器
    vote: "表决器 MooN",
    cmp: "比较器",
    and: "与门 AND",
    or: "或门 OR",
    not: "非门 NOT",
    xor: "异或 XOR",
    nand: "与非 NAND",
    nor: "或非 NOR",
    perm: "允许条件",
    rs: "RS 锁存",
    ton: "延时通",
    ctu: "计数器",
    firstout: "首出记录",
    // 旁路/复位
    bypass: "维护旁路",
    reset: "手动复位",
  };
  return fallback[type] || type;
}

function contactFor(ins: Instrument): string {
  // SQL 没有 contact 字段 —— 用 silTarget 暗示（NA/高 SIL → NC，低 SIL/手动 → NO）
  // 实装时如果用户提需求，再加字段。当前以 NO 兜底不影响画图。
  const s = String(ins.silTarget || "").toUpperCase();
  if (s === "NA" || s === "A") return "NC"; // 高安全 → 默认常闭
  return "NO";
}

async function saveData() {
  if (!diagram.value || !iframeEl.value || saving.value) return;
  // E1：viewer 只读（按钮已隐藏，此处为纵深防御）
  if (!auth.canWrite) return;

  // 已在等回包就跳过（防双击）—— 状态机的 saving=true 已经是显式围栏
  saving.value = true;
  saveError.value = false;

  // 1. 等编辑器回 save-response —— Promise-based（不靠 100ms 猜）
  //    超时 3s 后兜底返 null，避免主线程阻塞或 M0 异常时按钮永久 loading
  const savePromise = new Promise<string | null>((resolve) => {
    pendingSaveResolve = resolve;
  });
  const timeoutPromise = new Promise<string | null>((resolve) =>
    setTimeout(() => resolve(null), SAVE_TIMEOUT_MS),
  );

  iframeEl.value.contentWindow?.postMessage(
    { source: "sif-studio", type: "request-save" },
    "*",
  );

  const fresh = await Promise.race([savePromise, timeoutPromise]);
  // 兜底超时后清掉悬挂 resolver（M0 在延后时刻再回包也不会二次改 diagram）
  if (pendingSaveResolve) {
    pendingSaveResolve = null;
  }

  if (!diagram.value) {
    // 用户在 3s 内跳走 → 放弃
    saving.value = false;
    return;
  }

  if (fresh === null) {
    // M0 没回包：保留 DIRTY 让人重试；标 ERROR 提示
    saving.value = false;
    saveError.value = true;
    console.error("save-response 超时：editor.html 未在 3s 内回 payload.data");
    return;
  }

  // 2. payload.data 已写入 diagram.value.data（save-response handler 内做），
  //    写库。带当前 version 做乐观锁；409 = 他人已先保存。
  try {
    const res = await store.saveDiagramData(
      diagram.value.id,
      fresh,
      diagram.value.version,
    );
    // 用服务端返回的新版本号更新本地副本，下次保存继续 CAS
    diagram.value.version = res.version;
    saved.value = true;
    dirty.value = false;
    saveError.value = false;
    conflict.value = false;
    loadNote.value = ""; // 已落库 —— 撤掉"尚未保存"的提示
  } catch (e) {
    saveError.value = true;
    dirty.value = true;
    saved.value = false;
    if (e instanceof RpcError && e.kind === "conflict") {
      // 预期内的多用户冲突：内联提示 + 提供重载，不冒泡到全局错误横幅
      conflict.value = true;
      loadNote.value = "图已被他人修改，本地内容未丢失";
      console.warn("saveDiagramData 409 冲突：本地版本过期");
      return;
    }
    console.error("saveDiagramData 失败：", e);
    // 让上层 lastError 看到 —— store 内部已记录；这里再 console 一次方便排查
    throw e;
  } finally {
    saving.value = false;
  }
}
</script>

<style scoped>
.editor-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  /* 嵌入 iframe：取消 App content 的 padding */
  margin: calc(-1 * var(--s-5));
}

/* 标题栏 */
.ed-head {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  padding: var(--s-2) var(--s-4);
  border-bottom: var(--rule-bold) solid var(--ink-1);
  background: var(--paper);
}
.back {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ink-2);
  text-decoration: none;
  padding: 4px 8px;
  border: var(--rule-fine) solid var(--rule-2);
}
.back:hover {
  color: var(--ink-1);
  border-color: var(--ink-1);
  text-decoration: none;
}

.ed-title {
  display: flex;
  align-items: baseline;
  gap: var(--s-2);
  flex: 1;
  border-left: var(--rule-fine) solid var(--rule-2);
  border-right: var(--rule-fine) solid var(--rule-2);
  padding: 0 var(--s-3);
}
.ed-key {
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--ink-3);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.ed-name {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
  letter-spacing: 0.02em;
}
.ed-meta {
  font-size: var(--fs-xs);
  color: var(--ink-3);
  letter-spacing: 0.02em;
}

.ed-actions {
  display: flex;
  align-items: center;
  gap: var(--s-3);
}
.ed-note {
  font-size: var(--fs-micro);
  color: var(--warn);
  letter-spacing: 0.02em;
  white-space: nowrap;
}
.ed-status {
  display: flex;
  align-items: center;
  gap: var(--s-1);
  padding: 2px 8px;
  border: var(--rule-fine) solid currentColor;
  font-size: var(--fs-micro);
  letter-spacing: 0.06em;
}
.ed-status .status-dot {
  width: 8px;
  height: 8px;
  background: currentColor;
}
.ed-status .status-text { font-weight: 700; }
.ed-status.ok    { color: var(--ok);   background: var(--ok-bg); }
.ed-status.dirty { color: var(--warn); background: var(--warn-bg); }
.ed-status.save  { color: var(--acc);  background: var(--acc-bg); }
.ed-status.error { color: var(--err);  background: var(--err-bg); }

.ed-body {
  flex: 1;
  background: var(--paper-3);
  border-bottom: var(--rule-fine) solid var(--rule-2);
}
iframe {
  width: 100%;
  height: 100%;
  border: 0;
  background: var(--paper);
  display: block;
}
.placeholder {
  display: grid;
  place-items: center;
  align-content: center;
  gap: var(--s-2);
  color: var(--ink-3);
  height: 100%;
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  text-align: center;
}
.placeholder-mark {
  font-size: var(--fs-xl);
  font-weight: 700;
  color: var(--ink-2);
  letter-spacing: 0.1em;
  padding: var(--s-2) var(--s-4);
  border: var(--rule-mid) solid var(--ink-1);
}
.placeholder code {
  font-family: var(--font-mono);
  background: var(--paper-3);
  padding: 1px 6px;
  border: var(--rule-fine) solid var(--rule-2);
}
</style>