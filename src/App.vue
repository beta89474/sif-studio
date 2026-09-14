<template>
  <div class="app">
    <!-- 侧栏：工程图"图框边带" -->
    <aside class="sidebar">
      <div class="brand">
        <div class="logo">
          <span class="logo-sif">SIF</span>
          <span class="logo-studio">STUDIO</span>
        </div>
        <div class="brand-text">
          <div class="brand-title">SIF Studio</div>
          <div class="brand-sub">联锁工坊 · v0.1.0</div>
        </div>
      </div>

      <div class="brand-divider"></div>

      <nav class="nav">
        <div class="nav-label">VIEW</div>
        <router-link to="/" exact-active-class="active">
          <LayoutGrid :size="16" />
          <span>首页</span>
          <span class="nav-key">01</span>
        </router-link>
        <router-link to="/instruments" exact-active-class="active">
          <Database :size="16" />
          <span>仪表台账</span>
          <span class="nav-key">02</span>
        </router-link>
        <router-link to="/sifs" exact-active-class="active">
          <GitBranch :size="16" />
          <span>SIF 汇总</span>
          <span class="nav-key">03</span>
        </router-link>
        <router-link to="/projects" exact-active-class="active">
          <FolderOpen :size="16" />
          <span>项目</span>
          <span class="nav-key">04</span>
        </router-link>
        <router-link to="/bypass" exact-active-class="active">
          <ShieldAlert :size="16" />
          <span>旁路授权</span>
          <span class="nav-key">05</span>
        </router-link>
        <router-link to="/audit" exact-active-class="active">
          <ScrollText :size="16" />
          <span>审计中心</span>
          <span class="nav-key">06</span>
        </router-link>
        <router-link to="/settings" exact-active-class="active">
          <Settings :size="16" />
          <span>设置</span>
          <span class="nav-key">07</span>
        </router-link>
      </nav>

      <div class="bottom">
        <!-- E4：多组织切换（仅当用户属于 2 个及以上组织时出现） -->
        <div class="org-switch" v-if="auth.hasMultipleOrgs && auth.user">
          <div class="org-switch-label">ORG · 切换组织</div>
          <select
            class="org-select mono"
            :value="auth.user.orgId"
            :disabled="switchingOrg"
            @change="onSwitchOrg(($event.target as HTMLSelectElement).value)"
          >
            <option
              v-for="o in auth.user.orgs"
              :key="o.orgId"
              :value="o.orgId"
            >{{ o.orgName }}（{{ roleShort(o.role) }}）</option>
          </select>
          <div v-if="switchError" class="org-switch-err">{{ switchError }}</div>
        </div>
        <div class="status-row">
          <span class="status-dot" :class="{ ok: ready }"></span>
          <span class="status-text">{{ statusText }}</span>
          <span class="status-key">SYS</span>
        </div>
        <div class="meta" v-if="meta">
          <div class="meta-row">
            <span class="meta-key">APP</span>
            <span class="meta-val mono">v{{ meta.appVersion }}</span>
          </div>
          <div class="meta-row">
            <span class="meta-key">DB</span>
            <span class="meta-val mono">v{{ meta.dbVersion }}</span>
          </div>
          <!-- B7 — 登录用户 / 组织 / 退出 -->
          <div class="meta-row" v-if="auth.user">
            <span class="meta-key">USER</span>
            <span class="meta-val meta-user" :title="auth.user.email">
              {{ auth.actorName }}<span class="user-org"> · {{ auth.user.orgName }}</span>
              <span class="role-badge" :class="`role-${auth.user.role}`">{{ roleShort(auth.user.role) }}</span>
            </span>
            <button class="user-logout" type="button" @click="onLogout" :disabled="loggingOut">
              {{ loggingOut ? "…" : "退出" }}
            </button>
          </div>
          <div class="meta-row" v-else>
            <span class="meta-key">USER</span>
            <span class="meta-val mono">未登录</span>
          </div>
        </div>
      </div>
    </aside>

    <!-- 主区 -->
    <main class="main">
      <!-- 顶栏 = ISO 7200 标题栏（margin 让位给 BypassAlertBanner） -->
      <header class="topbar" :class="{ 'topbar-with-alert': hasBypassAlert }">
        <div class="tb-cell tb-title">
          <div class="tb-key">TITLE</div>
          <div class="tb-val">{{ currentTitle || "—" }}</div>
        </div>
        <div class="tb-cell tb-mid">
          <div class="tb-key">SECTION</div>
          <div class="tb-val mono">{{ routeSection }}</div>
        </div>
        <div class="tb-cell tb-right">
          <div class="tb-key">REV / STAMP</div>
          <div class="tb-val mono">{{ buildStamp }}</div>
        </div>
        <div class="tb-cell tb-num">
          <div class="tb-key">COUNT</div>
          <div class="tb-val mono">{{ totalInstruments }}/{{ totalSifs }}</div>
        </div>
      </header>

      <!-- M2.2 增强：全站旁路逾期告警横幅（IEC 61511-1 §11.5.2 合规缺口） -->
      <BypassAlertBanner />

      <section class="content">
        <!-- 数据层初始化失败时的错误横幅（避免白屏无可诊断信息） -->
        <div class="err-banner" v-if="store.lastError">
          <span class="err-kind">{{ store.lastError.kind }}</span>
          <span class="err-msg">{{ store.lastError.message }}</span>
          <button class="err-retry" type="button" @click="retry">重试 / RETRY</button>
        </div>

        <router-view />
      </section>

      <!-- 底栏：工程图页脚 -->
      <footer class="footer">
        <span class="f-key">SHEET</span>
        <span class="f-val mono">{{ pageCode }}</span>
        <span class="f-sep">|</span>
        <span class="f-key">PROJ</span>
        <span class="f-val mono">SIF-STUDIO</span>
        <span class="f-sep">|</span>
        <span class="f-key">DATE</span>
        <span class="f-val mono">{{ today }}</span>
        <span class="f-spacer"></span>
        <span class="f-key">REF</span>
        <span class="f-val mono">IEC 61511 / ISA 5.1 / GB/T 18135</span>
      </footer>
    </main>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import {
  LayoutGrid,
  Database,
  GitBranch,
  FolderOpen,
  ShieldAlert,
  ScrollText,
  Settings,
} from "lucide-vue-next";
import { useStudioStore } from "./stores/studio";
import { useAuthStore } from "./stores/auth";
import { acceptInvite, invitePreview, roleLabel } from "./api/org";
import BypassAlertBanner from "./components/BypassAlertBanner.vue";

const store = useStudioStore();
const auth = useAuthStore();
const route = useRoute();
const router = useRouter();

const loggingOut = ref(false);
const switchingOrg = ref(false);
const switchError = ref("");

function roleShort(role: string): string {
  if (role === "owner") return "OWNER";
  if (role === "engineer") return "ENG";
  if (role === "viewer") return "VIEWER";
  return role.toUpperCase();
}

async function onSwitchOrg(orgIdRaw: string) {
  const orgId = Number(orgIdRaw);
  if (!auth.user || orgId === auth.user.orgId || switchingOrg.value) return;
  switchingOrg.value = true;
  switchError.value = "";
  try {
    await auth.switchOrg(orgId);
    // 业务 store 已按新组织重置并重新 bootstrap（Pinia 响应式自动刷新当前页）；
    // 若正停留在带参数的编辑页，回首页避免展示旧组织上下文。
    if (route.name !== "home") await router.push({ name: "home" });
  } catch (e) {
    switchError.value = e instanceof Error ? e.message : "切换失败";
    // eslint-disable-next-line no-console
    console.error("[sif-studio] switch org failed", e);
  } finally {
    switchingOrg.value = false;
  }
}

/** E4：已登录用户打开邀请链接（#/register?invite=…）时的接受跨组织邀请流程。
 *  路由守卫会把该跳转改写为 /?invite=…，这里确认后调 /org/invite/accept。 */
const inviteBusy = ref(false);
async function maybeAcceptInvite(tokenRaw: unknown): Promise<void> {
  if (typeof tokenRaw !== "string" || !tokenRaw || !auth.user || inviteBusy.value) return;
  inviteBusy.value = true;
  try {
    const inv = await invitePreview(tokenRaw);
    const ok = window.confirm(
      `组织「${inv.orgName}」邀请你以角色「${roleLabel(inv.role)}」加入本系统。` +
        "接受后可在左侧栏底部切换组织，是否接受？",
    );
    if (!ok) return;
    const res = await acceptInvite(tokenRaw);
    await auth.refreshMe();
    window.alert(`已加入组织「${res.orgName}」，可在左侧栏底部切换组织。`);
  } catch (e) {
    window.alert(`接受邀请失败：${e instanceof Error ? e.message : String(e)}`);
  } finally {
    inviteBusy.value = false;
    const query = { ...route.query };
    delete query.invite;
    await router.replace({ query });
  }
}
watch(
  () => [route.query.invite, auth.user] as const,
  ([token]) => {
    void maybeAcceptInvite(token);
  },
);

async function onLogout() {
  if (loggingOut.value) return;
  loggingOut.value = true;
  try {
    await auth.logout();
    await router.push({ name: "login" });
  } finally {
    loggingOut.value = false;
  }
}

// B7 — 会话过期被动感知：任意 RPC 收到 401 → 清登录态并回登录页
function onUnauthorized() {
  auth.user = null;
  if (route.name !== "login") {
    void router.push({
      name: "login",
      query: route.fullPath === "/" ? {} : { redirect: route.fullPath },
    });
  }
}
onMounted(() => window.addEventListener("sif:unauthorized", onUnauthorized));
onBeforeUnmount(() => window.removeEventListener("sif:unauthorized", onUnauthorized));

const ready = computed(() => store.ready);
const meta = computed(() => store.meta);
const totalInstruments = computed(() => store.instruments.length);
const totalSifs = computed(() => store.sifSummary.length);
const statusText = computed(() => {
  if (store.lastError) return "ERR";
  if (!store.ready) return "INIT";
  return "READY";
});

const currentTitle = computed(() => (route.meta.title as string) ?? "");
const routeSection = computed(() => {
  const p = route.path;
  if (p === "/") return "01 / OVERVIEW";
  if (p.startsWith("/instruments")) return "02 / INSTRUMENT REGISTRY";
  if (p.startsWith("/sifs")) return "03 / SIF SUMMARY";
  if (p.startsWith("/projects")) return "04 / PROJECT INDEX";
  if (p.startsWith("/diagram")) return "05 / LOGIC DIAGRAM";
  if (p.startsWith("/bypass")) return "06 / BYPASS REGISTER";
  if (p.startsWith("/audit")) return "07 / AUDIT EXPORT";
  if (p.startsWith("/settings")) return "08 / SYSTEM SETTINGS";
  return p.toUpperCase();
});

const buildStamp = computed(() => "REV A · " + new Date().toISOString().slice(0, 10));
const today = new Date().toISOString().slice(0, 10);
// M2.2 增强：横幅 fixed 时需要给顶栏让位（避免遮挡）
const hasBypassAlert = computed(() => store.overdueBypassStatus.count > 0);
const pageCode = computed(() => {
  const map: Record<string, string> = {
    "/": "DWG-001",
    "/instruments": "DWG-002",
    "/sifs": "DWG-003",
    "/projects": "DWG-004",
    "/bypass": "DWG-006",
    "/audit": "DWG-007",
    "/settings": "DWG-008",
  };
  if (route.path.startsWith("/diagram")) return "DWG-005";
  return map[route.path] ?? "DWG-000";
});

// 数据层 bootstrap 由 main.ts 在挂载后统一触发，此处不重复调用（避免并发双请求）
async function retry() {
  store.clearError();
  try {
    await store.bootstrap();
  } catch (e) {
    // 错误会重新写入 store.lastError，横幅继续显示
    // eslint-disable-next-line no-console
    console.error("[sif-studio] retry failed", e);
  }
}
</script>

<style scoped>
/* ============================================================================
 * 主框架布局
 * ========================================================================== */
.app {
  display: grid;
  grid-template-columns: 232px 1fr;
  height: 100vh;
  background: var(--paper-2);
}

/* ============================================================================
 * 侧栏
 * ========================================================================== */
.sidebar {
  background: var(--ink-1);
  color: var(--paper);
  display: flex;
  flex-direction: column;
  border-right: var(--rule-bold) solid var(--ink-1);
}

.brand {
  display: flex;
  align-items: center;
  padding: var(--s-4) var(--s-4) var(--s-3);
  gap: var(--s-3);
}

.logo {
  width: 44px;
  height: 44px;
  border: var(--rule-mid) solid var(--paper);
  display: grid;
  place-items: center;
  text-align: center;
  line-height: 1;
  background: var(--ink-1);
  font-family: var(--font-title);
  letter-spacing: 0;
}
.logo-sif {
  display: block;
  font-size: 16px;
  font-weight: 900;
  color: var(--paper);
}
.logo-studio {
  display: block;
  font-size: 7px;
  font-weight: 700;
  color: var(--ink-3);
  margin-top: 1px;
  letter-spacing: 0.08em;
}

.brand-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.brand-title {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--paper);
  letter-spacing: 0.02em;
}
.brand-sub {
  font-size: var(--fs-micro);
  color: var(--ink-4);
  font-family: var(--font-mono);
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.brand-divider {
  height: 0;
  border-top: var(--rule-fine) solid var(--ink-3);
  margin: 0 var(--s-4) var(--s-3);
}

/* ----- nav ----- */
.nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  padding: 0;
}
.nav-label {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-4);
  padding: 0 var(--s-4) var(--s-2);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.nav a {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  padding: var(--s-3) var(--s-4);
  color: var(--ink-3);
  font-family: var(--font-roman), var(--font-cn);
  font-size: var(--fs-sm);
  font-weight: 500;
  border-left: 4px solid transparent;
  border-bottom: var(--rule-fine) solid #1f2937;
  text-decoration: none;
  letter-spacing: 0.02em;
  transition: none;
}
.nav a:hover {
  background: #1a2128;
  color: var(--paper);
  text-decoration: none;
}
.nav a.active {
  background: var(--paper-3);
  color: var(--ink-1);
  border-left-color: var(--acc);
  font-weight: 700;
}
.nav .nav-key {
  margin-left: auto;
  font-family: var(--font-mono);
  font-size: var(--fs-micro);
  color: var(--ink-4);
  font-weight: 500;
}
.nav a.active .nav-key { color: var(--ink-1); }

/* ----- bottom status ----- */
.bottom {
  margin-top: auto;
  padding: var(--s-3) var(--s-4);
  border-top: var(--rule-fine) solid var(--ink-3);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
}
.status-row {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  margin-bottom: var(--s-2);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--ink-3);
}
.status-row .status-text { color: var(--paper); font-weight: 700; }
.status-row .status-key {
  margin-left: auto;
  font-size: 9px;
  color: var(--ink-4);
  letter-spacing: 0.16em;
}
.status-dot {
  width: 8px; height: 8px;
  border-radius: 0;
  background: var(--warn);
  border: 1px solid var(--warn);
}
.status-dot.ok {
  background: var(--ok);
  border-color: var(--ok);
}
.meta { display: flex; flex-direction: column; gap: 2px; }
.meta-row {
  display: flex;
  gap: var(--s-2);
  align-items: baseline;
}
.meta-row .meta-key {
  color: var(--ink-4);
  font-size: 9px;
  letter-spacing: 0.16em;
  min-width: 30px;
}
.meta-row .meta-val { color: var(--ink-3); }
/* B7 — 侧栏用户区 */
.meta-row .meta-val.meta-user {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--paper);
  font-size: var(--fs-xs);
}
.meta-user .user-org {
  color: var(--ink-4);
  font-size: 9px;
}
.user-logout {
  flex: none;
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.12em;
  padding: 2px 8px;
  cursor: pointer;
  background: transparent;
  color: var(--ink-3);
  border: var(--rule-fine) solid var(--ink-3);
  border-radius: 0;
}
.user-logout:hover:not(:disabled) {
  background: var(--paper-3);
  color: var(--ink-1);
  border-color: var(--paper-3);
}
.user-logout:disabled { opacity: 0.5; cursor: default; }

/* E4 — 组织切换器 / 角色徽标 */
.org-switch {
  margin-bottom: var(--s-3);
  padding-bottom: var(--s-3);
  border-bottom: var(--rule-fine) solid var(--ink-3);
}
.org-switch-label {
  font-size: 9px;
  color: var(--ink-4);
  letter-spacing: 0.16em;
  margin-bottom: 4px;
}
.org-select {
  width: 100%;
  background: var(--ink-1);
  color: var(--paper);
  border: var(--rule-fine) solid var(--ink-3);
  border-radius: 0;
  padding: 4px 6px;
  font-size: var(--fs-xs);
  cursor: pointer;
}
.org-select:disabled { opacity: 0.6; cursor: default; }
.org-switch-err {
  margin-top: 4px;
  font-size: 9px;
  color: var(--warn);
  line-height: 1.4;
}
.role-badge {
  display: inline-block;
  margin-left: 4px;
  padding: 0 4px;
  font-size: 8px;
  letter-spacing: 0.08em;
  border: var(--rule-fine) solid currentColor;
  vertical-align: middle;
}
.role-badge.role-owner { color: var(--acc); }
.role-badge.role-engineer { color: var(--ok); }
.role-badge.role-viewer { color: var(--ink-4); }
.meta-path .meta-val {
  font-size: 9px;
  word-break: break-all;
  color: var(--ink-4);
  letter-spacing: 0.02em;
}

/* ============================================================================
 * 主区
 * ========================================================================== */
.main {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* ----- ISO 7200 标题栏 ----- */
.topbar {
  display: grid;
  grid-template-columns: 2fr 1.6fr 1.4fr 1fr;
  border-bottom: var(--rule-bold) solid var(--ink-1);
  background: var(--paper);
}
/* M2.2 增强：让位给 fixed 顶部横幅 */
.topbar.topbar-with-alert { margin-top: 56px; }
.tb-cell {
  padding: var(--s-2) var(--s-4);
  border-right: var(--rule-fine) solid var(--rule-2);
  display: flex;
  flex-direction: column;
  gap: 2px;
  justify-content: center;
}
.tb-cell:last-child { border-right: 0; }
.tb-key {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-4);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.tb-val {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  color: var(--ink-1);
  font-weight: 700;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  line-height: var(--lh-tight);
}
.tb-cell.tb-mid .tb-val,
.tb-cell.tb-right .tb-val,
.tb-cell.tb-num .tb-val {
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  font-weight: 600;
  text-transform: none;
  letter-spacing: 0.02em;
  color: var(--ink-2);
}

.content {
  flex: 1;
  overflow: auto;
  padding: var(--s-5);
  background: var(--paper-2);
}

/* ----- 错误横幅（数据层初始化失败） ----- */
.err-banner {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  margin-bottom: var(--s-4);
  padding: var(--s-3) var(--s-4);
  background: var(--err-bg);
  border: var(--rule-mid) solid var(--err);
  border-left-width: 6px;
  color: var(--err);
}
.err-kind {
  flex: none;
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  padding: 1px 6px;
  border: var(--rule-fine) solid var(--err);
  background: var(--paper);
}
.err-msg {
  flex: 1;
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--ink-1);
  word-break: break-all;
  line-height: 1.45;
}
.err-retry {
  flex: none;
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  padding: 4px 12px;
  cursor: pointer;
  background: var(--err);
  color: var(--paper);
  border: var(--rule-fine) solid var(--err);
  border-radius: 0;
}
.err-retry:hover {
  background: var(--paper);
  color: var(--err);
}

/* ----- 底栏 ----- */
.footer {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  padding: var(--s-2) var(--s-4);
  border-top: var(--rule-fine) solid var(--rule-2);
  background: var(--paper);
  font-size: var(--fs-xs);
}
.f-key {
  font-family: var(--font-mono);
  font-size: 9px;
  color: var(--ink-4);
  letter-spacing: 0.16em;
  text-transform: uppercase;
}
.f-val {
  color: var(--ink-2);
  font-weight: 500;
}
.f-sep {
  color: var(--rule-2);
  margin: 0 var(--s-1);
}
.f-spacer { flex: 1; }
</style>