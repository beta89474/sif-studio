<!--
  SettingsView.vue — C6/D1 系统设置

  四个能力块：
    1. 组织成员与邀请（D1）：成员表全员可见；owner 可签发/吊销团队邀请链接、
       改角色、移除成员（最后一个 owner 受服务端保护）。
    2. 旧桌面库导入（组织 owner）：POST /api/admin/import-legacy，走会话 cookie；
       按业务唯一键幂等，结果按 8 类表展示 inserted/skipped。
    3. 整库备份/恢复（服务器操作员）：静态 X-Admin-Token（SIF_ADMIN_TOKEN）。
       备份为浏览器下载；恢复覆盖所有组织，需勾选知情确认；旧版本备份自动升级。
    4. 命令行用法：curl 示例与环境变量说明（自动化运维场景）。

  设计：沿用工业图纸卡片风（零圆角 / 1px 黑边 / 等宽数字）。
  管理员令牌只存在组件内存，不落 localStorage。
-->
<template>
  <div class="settings">
    <!-- 顶栏：ISO 7200 标题块 -->
    <header class="head card">
      <div class="head-text">
        <div class="head-eyebrow">SECTION 08 · SYSTEM SETTINGS</div>
        <h2 class="head-title">系统设置 · SETTINGS</h2>
        <div class="head-sub">
          旧桌面版数据迁入本组织；整库备份与恢复由服务器操作员持有令牌执行
        </div>
      </div>
      <div class="head-block">
        <div class="head-row">
          <span class="head-key">ROLE</span>
          <span class="head-val mono">{{ auth.user?.role ?? "—" }}</span>
        </div>
        <div class="head-row">
          <span class="head-key">ORG</span>
          <span class="head-val mono">{{ auth.user?.orgName ?? "—" }}</span>
        </div>
      </div>
    </header>

    <!-- 0. 账号安全（自助；所有角色可见） -->
    <section class="card">
      <div class="card-title">
        <span>账号安全 · ACCOUNT</span>
        <span class="tag" :class="`role-tag-${auth.user?.role ?? ''}`">{{ roleLabel(auth.user?.role ?? "") }}</span>
      </div>

      <div v-if="acctMsg" class="banner" :class="acctOk ? 'ok' : 'err'">{{ acctMsg }}</div>

      <!-- F2：被管理员重置密码后的强制改密警示（路由守卫已限制只能停留本页） -->
      <div v-if="auth.mustChangePassword" class="banner force-pw">
        管理员已为你重置了临时密码。出于账号安全，必须先在下方修改为自己的密码后，
        才能使用台账、图纸、旁路等其它功能。
      </div>

      <div class="acct-grid">
        <!-- 显示名 -->
        <div class="acct-block">
          <div class="acct-h">显示名 · DISPLAY NAME</div>
          <label class="field">
            <input
              v-model="profileName"
              type="text"
              maxlength="50"
              spellcheck="false"
              :disabled="acctBusy"
            />
          </label>
          <button class="btn primary" :disabled="acctBusy || !profileName.trim()" @click="onSaveProfile">
            保存显示名
          </button>
        </div>

        <!-- 改密码 -->
        <div class="acct-block">
          <div class="acct-h">修改密码 · CHANGE PASSWORD</div>
          <label class="field">
            <span class="lbl">原密码</span>
            <input v-model="oldPw" type="password" autocomplete="current-password" :disabled="acctBusy" />
          </label>
          <label class="field">
            <span class="lbl">新密码（至少 8 个字符）</span>
            <input v-model="newPw" type="password" autocomplete="new-password" :disabled="acctBusy" />
          </label>
          <button
            class="btn primary"
            :disabled="acctBusy || !oldPw || newPw.length < 8"
            @click="onChangePassword"
          >
            {{ acctBusy ? "处理中…" : "修改密码" }}
          </button>
          <p class="hint tight">修改成功后，本账号在其它设备上的会话将立即失效，需用新密码重新登录。</p>
        </div>
      </div>
    </section>

    <!-- 0.5 登录设备 / 会话管理 -->
    <section class="card">
      <div class="card-title">
        <span>登录设备 · SESSIONS</span>
        <button class="btn" :disabled="sessBusy || otherSessionCount === 0" @click="onLogoutOthers">
          {{ sessBusy ? "处理中…" : `退出其它设备（${otherSessionCount}）` }}
        </button>
      </div>
      <div v-if="sessMsg" class="banner" :class="sessOk ? 'ok' : 'err'">{{ sessMsg }}</div>
      <table class="report">
        <thead>
          <tr>
            <th>组织</th>
            <th>设备 / 浏览器</th>
            <th>IP</th>
            <th>最后访问</th>
            <th>状态</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(s, idx) in sessions" :key="idx" :class="{ 'sess-current': s.current }">
            <td>{{ s.orgName }}</td>
            <td class="mono sess-ua" :title="s.userAgent">{{ shortUa(s.userAgent) }}</td>
            <td class="mono">{{ s.ip || "—" }}</td>
            <td class="mono">{{ fmtTs(s.lastSeenAt) }}</td>
            <td>
              <span v-if="s.current" class="st-active">当前设备</span>
              <span v-else class="muted mono">已登录</span>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-if="!sessions.length" class="hint tight">会话列表加载中…</p>
    </section>

    <!-- 1. 组织成员 / 邀请 -->
    <section class="card">
      <div class="card-title">
        <span>组织成员 · MEMBERS</span>
        <span class="tag" :class="isOwner ? 'tag-ok' : 'tag-lock'">
          {{ isOwner ? "OWNER 可管理" : "只读" }}
        </span>
      </div>

      <div v-if="orgMsg" class="banner" :class="orgOk ? 'ok' : 'err'">{{ orgMsg }}</div>

      <!-- 成员表：所有成员可见；owner 可改角色 / 移除 -->
      <table class="report members">
        <thead>
          <tr>
            <th>邮箱</th>
            <th>显示名</th>
            <th>角色</th>
            <th>加入时间</th>
            <th v-if="isOwner" class="num">操作</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="m in members" :key="m.userId">
            <td class="mono">{{ m.email }}</td>
            <td>{{ m.displayName || "—" }}</td>
            <td>
              <select
                v-if="isOwner"
                :value="m.role"
                :disabled="orgBusy"
                class="role-select mono"
                @change="onRoleChange(m, ($event.target as HTMLSelectElement).value as OrgRole)"
              >
                <option value="owner">所有者</option>
                <option value="engineer">工程师</option>
                <option value="viewer">只读</option>
              </select>
              <span v-else class="mono">{{ roleLabel(m.role) }}</span>
            </td>
            <td class="mono">{{ fmtTs(m.createdAt) }}</td>
            <td v-if="isOwner" class="num member-ops">
              <button
                v-if="m.userId !== auth.user?.userId"
                class="btn-link"
                :disabled="orgBusy"
                @click="onResetPassword(m)"
              >
                重置密码
              </button>
              <button
                class="btn-link danger"
                :disabled="orgBusy"
                @click="onRemove(m)"
              >
                {{ m.userId === auth.user?.userId ? "退出组织" : "移除" }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>

      <template v-if="isOwner">
        <div class="divider"></div>

        <!-- 签发邀请 -->
        <p class="hint tight">
          邀请为<b>团队链接</b>：生成后在有效期内可被多人使用，对方打开链接注册即加入本组织；
          可随时吊销。新成员默认角色工程师，有效期默认 7 天（上限由部署配置决定，
          默认最长 30 天）。
        </p>
        <div class="row">
          <label class="inline-field">
            <span class="lbl">角色</span>
            <select v-model="newRole" class="role-select mono" :disabled="orgBusy">
              <option value="engineer">工程师（可编辑）</option>
              <option value="viewer">只读（仅查看）</option>
              <option value="owner">所有者（完全管理）</option>
            </select>
          </label>
          <label class="inline-field">
            <span class="lbl">有效期</span>
            <select v-model.number="newTtl" class="role-select mono" :disabled="orgBusy">
              <option :value="1">1 天</option>
              <option :value="7">7 天</option>
              <option :value="30">30 天</option>
            </select>
          </label>
          <button class="btn primary" :disabled="orgBusy" @click="onCreateInvite">
            {{ orgBusy ? "处理中…" : "生成邀请链接" }}
          </button>
        </div>

        <!-- 邀请列表 -->
        <table v-if="invites.length" class="report">
          <thead>
            <tr>
              <th>邀请链接（点击复制）</th>
              <th>角色</th>
              <th>有效期至</th>
              <th>状态</th>
              <th class="num">操作</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="inv in invites" :key="inv.token">
              <td>
                <button class="link-copy mono" :disabled="orgBusy" @click="copyInvite(inv.token)">
                  {{ shortToken(inv.token) }}
                </button>
                <span v-if="copiedToken === inv.token" class="copied-hint">已复制 ✓</span>
              </td>
              <td class="mono">{{ roleLabel(inv.role) }}</td>
              <td class="mono">{{ fmtTs(inv.expiresAt) }}</td>
              <td>
                <span v-if="inv.revokedAt" class="st-revoked">已吊销</span>
                <span v-else-if="isExpired(inv.expiresAt)" class="st-expired">已过期</span>
                <span v-else class="st-active">生效中</span>
              </td>
              <td class="num">
                <button
                  v-if="!inv.revokedAt && !isExpired(inv.expiresAt)"
                  class="btn-link danger"
                  :disabled="orgBusy"
                  @click="onRevoke(inv)"
                >
                  吊销
                </button>
                <span v-else>—</span>
              </td>
            </tr>
          </tbody>
        </table>
      </template>
    </section>

    <!-- 2. 旧桌面库导入（owner） -->
    <section class="card">
      <div class="card-title">
        <span>旧桌面库导入 · LEGACY IMPORT</span>
        <span class="tag" :class="isOwner ? 'tag-ok' : 'tag-lock'">
          {{ isOwner ? "OWNER" : "仅 OWNER" }}
        </span>
      </div>

      <template v-if="isOwner">
        <p class="hint">
          选择旧 Tauri 桌面版的 <code>studio.db</code>，其中的项目 / 仪表 / SIF /
          图纸 / 旁路 / 工单等数据会导入<b>当前组织</b>，ID 在导入时重新分配。
          导入可重复执行——已存在的业务数据自动跳过，不会产生重复行。
        </p>

        <div class="row">
          <input
            ref="legacyInput"
            type="file"
            accept=".db,.sqlite,.sqlite3,application/vnd.sqlite3"
            class="file-input"
            @change="onLegacyPick"
          />
          <button
            class="btn primary"
            :disabled="!legacyFile || legacyBusy"
            @click="doImportLegacy"
          >
            {{ legacyBusy ? "导入中…" : "开始导入" }}
          </button>
        </div>

        <div v-if="legacyMsg" class="banner" :class="legacyOk ? 'ok' : 'err'">
          {{ legacyMsg }}
        </div>

        <table v-if="legacyReport" class="report">
          <thead>
            <tr>
              <th>数据类别</th>
              <th class="num">新增 INSERTED</th>
              <th class="num">跳过 SKIPPED（已存在）</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="r in legacyRows" :key="r.key">
              <td>{{ r.label }}</td>
              <td class="num mono">{{ r.value.inserted }}</td>
              <td class="num mono">{{ r.value.skipped }}</td>
            </tr>
            <tr class="orphan" v-if="legacyReport.orphanRows > 0">
              <td>无归属项目被丢弃</td>
              <td class="num mono" colspan="2">{{ legacyReport.orphanRows }}</td>
            </tr>
          </tbody>
        </table>
      </template>

      <p v-else class="hint locked">
        旧桌面库导入是组织级操作，仅组织 <b>owner</b> 可执行；当前角色为
        <code>{{ auth.user?.role ?? "—" }}</code>。
      </p>
    </section>

    <!-- 3. 整库备份 / 恢复（服务器操作员） -->
    <section class="card">
      <div class="card-title">
        <span>整库备份与恢复 · BACKUP / RESTORE</span>
        <span class="tag tag-op">服务器操作员</span>
      </div>

      <p class="hint">
        备份覆盖<b>所有组织</b>（账号、会话、全部业务数据），是服务器操作员能力，
        需提供部署时通过环境变量 <code>SIF_ADMIN_TOKEN</code> 配置的管理员令牌。
        令牌仅保存在本页内存中，关闭或刷新页面即失效。
      </p>

      <label class="field">
        <span class="lbl">管理员令牌 · ADMIN TOKEN</span>
        <input
          v-model="adminToken"
          type="password"
          autocomplete="off"
          placeholder="SIF_ADMIN_TOKEN"
          spellcheck="false"
        />
      </label>

      <div v-if="backupMsg" class="banner" :class="backupOk ? 'ok' : 'err'">
        {{ backupMsg }}
      </div>

      <div class="row">
        <button
          class="btn primary"
          :disabled="!adminToken || backupBusy"
          @click="doBackup"
        >
          {{ backupBusy ? "生成中…" : "下载整库备份 (.db)" }}
        </button>
        <span class="inline-hint">VACUUM 一致性快照，建议恢复前先备份</span>
      </div>

      <div class="divider"></div>

      <div class="danger-box">
        <div class="danger-title">危险操作 · RESTORE 覆盖全部租户</div>
        <p class="hint tight">
          恢复会用备份文件<b>整体替换</b>当前服务器的数据库（所有组织、账号、
          会话）。旧版本备份会自动升级到当前服务版本；比当前服务更新的备份
          无法恢复。恢复后当前登录会话可能失效，需要重新登录。
        </p>
        <div class="row">
          <input
            ref="restoreInput"
            type="file"
            accept=".db,.sqlite,.sqlite3,application/vnd.sqlite3"
            class="file-input"
            @change="onRestorePick"
          />
        </div>
        <label class="ack">
          <input v-model="restoreAck" type="checkbox" />
          <span>我已知晓：恢复将<b>不可撤销地</b>覆盖所有组织的现有数据</span>
        </label>
        <div class="row">
          <button
            class="btn danger"
            :disabled="!adminToken || !restoreFile || !restoreAck || restoreBusy"
            @click="doRestore"
          >
            {{ restoreBusy ? "恢复中…" : "执行整库恢复" }}
          </button>
        </div>
        <div v-if="restoreMsg" class="banner" :class="restoreOk ? 'ok' : 'err'">
          {{ restoreMsg }}
        </div>
        <table v-if="restoreReport" class="report">
          <tbody>
            <tr>
              <td>复制表数</td>
              <td class="num mono">{{ restoreReport.tablesCopied }}</td>
            </tr>
            <tr>
              <td>迁移版本</td>
              <td class="num mono">v{{ restoreReport.migrationVersion }}</td>
            </tr>
            <tr v-for="(n, t) in restoreReport.rowsCopied" :key="t">
              <td class="mono">{{ t }}</td>
              <td class="num mono">{{ n }} 行</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>

    <!-- 4. 命令行用法 -->
    <section class="card">
      <div class="card-title"><span>命令行用法 · CLI</span></div>
      <p class="hint tight">无人值守备份 / 恢复可直接用 curl 调用：</p>
      <pre class="cli"># 下载备份
curl -fSsfOJ -H "x-admin-token: $SIF_ADMIN_TOKEN" \
  https://your-host/api/admin/backup

# 从备份恢复（multipart 字段名固定为 file）
curl -fSs -X POST \
  -H "x-admin-token: $SIF_ADMIN_TOKEN" \
  -F "file=@./sif-studio-backup-20260101-000000.db" \
  https://your-host/api/admin/restore

# 旧桌面库导入（用 owner 账号的会话 cookie）
curl -fSs -X POST \
  -H "Cookie: sif_session=&lt;token&gt;" \
  -F "file=@./studio.db" \
  https://your-host/api/admin/import-legacy</pre>
      <p class="hint tight">
        相关环境变量：<code>SIF_ADMIN_TOKEN</code>（操作员令牌，不设则备份/恢复端点
        404）、<code>SIF_COOKIE_SECURE=1</code>（HTTPS 下给会话 cookie 加 Secure）、
        <code>DATABASE_URL</code>（默认 <code>sqlite://data/studio.db?mode=rwc</code>）。
      </p>
    </section>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";
import { useStudioStore } from "../stores/studio";
import {
  downloadBackup,
  importLegacy,
  restoreBackup,
  type LegacyReport,
  type LegacyTableReport,
  type RestoreReport,
} from "../api/admin";
import {
  createInvite,
  inviteUrl,
  listInvites,
  listMembers,
  removeMember,
  resetMemberPassword,
  revokeInvite,
  roleLabel,
  setMemberRole,
  type Invite,
  type Member,
  type OrgRole,
} from "../api/org";
import { listSessions, logoutOtherSessions, type SessionInfo } from "../api/auth";

const auth = useAuthStore();
const studio = useStudioStore();
const router = useRouter();

const isOwner = computed(() => auth.user?.role === "owner");

// ---- E3 账号安全 ----------------------------------------------------------
const profileName = ref(auth.user?.displayName ?? "");
const oldPw = ref("");
const newPw = ref("");
const acctBusy = ref(false);
const acctMsg = ref("");
const acctOk = ref(false);

function setAcctMsg(ok: boolean, msg: string): void {
  acctOk.value = ok;
  acctMsg.value = msg;
}

async function onSaveProfile(): Promise<void> {
  const name = profileName.value.trim();
  if (!name || acctBusy.value) return;
  acctBusy.value = true;
  setAcctMsg(true, "");
  try {
    await auth.updateProfile(name);
    setAcctMsg(true, "显示名已更新。");
  } catch (e) {
    setAcctMsg(false, errText(e));
  } finally {
    acctBusy.value = false;
  }
}

async function onChangePassword(): Promise<void> {
  if (acctBusy.value || !oldPw.value || newPw.value.length < 8) return;
  acctBusy.value = true;
  setAcctMsg(true, "");
  try {
    await auth.changePassword(oldPw.value, newPw.value);
    oldPw.value = "";
    newPw.value = "";
    setAcctMsg(true, "密码已修改，其它设备已退出登录。");
    void refreshSessions().catch(() => {});
  } catch (e) {
    setAcctMsg(false, errText(e));
  } finally {
    acctBusy.value = false;
  }
}

// ---- E4 会话管理 ----------------------------------------------------------
const sessions = ref<SessionInfo[]>([]);
const sessBusy = ref(false);
const sessMsg = ref("");
const sessOk = ref(false);
const otherSessionCount = computed(() => sessions.value.filter((s) => !s.current).length);

async function refreshSessions(): Promise<void> {
  sessions.value = await listSessions();
}

function shortUa(ua: string): string {
  if (!ua) return "未知设备";
  // 精简展示：识别浏览器/系统关键字，避免一整段 UA 撑爆表格
  const os = /Windows/.test(ua)
    ? "Windows"
    : /Mac OS|Macintosh/.test(ua)
      ? "macOS"
      : /Android/.test(ua)
        ? "Android"
        : /iPhone|iPad|iOS/.test(ua)
          ? "iOS"
          : /Linux/.test(ua)
            ? "Linux"
            : "";
  const browser = /Edg\//.test(ua)
    ? "Edge"
    : /Chrome\//.test(ua)
      ? "Chrome"
      : /Firefox\//.test(ua)
        ? "Firefox"
        : /Safari\//.test(ua)
          ? "Safari"
          : "";
  return [browser, os].filter(Boolean).join(" · ") || ua.slice(0, 40);
}

async function onLogoutOthers(): Promise<void> {
  if (sessBusy.value || otherSessionCount.value === 0) return;
  if (!window.confirm("确定退出其它所有设备上的登录会话吗？")) return;
  sessBusy.value = true;
  sessMsg.value = "";
  try {
    const n = await logoutOtherSessions();
    await refreshSessions();
    sessOk.value = true;
    sessMsg.value = `已退出 ${n} 个其它设备会话。`;
  } catch (e) {
    sessOk.value = false;
    sessMsg.value = errText(e);
  } finally {
    sessBusy.value = false;
  }
}

// ---- E3 owner 重置成员密码 ------------------------------------------------
async function onResetPassword(m: Member): Promise<void> {
  const pw = window.prompt(
    `为 ${m.email} 设置临时密码（至少 8 个字符）。\n` +
      "重置后该成员在所有设备上立即下线，且首次登录时必须自行修改密码：",
  );
  if (pw === null) return;
  if (pw.length < 8) {
    setOrgMsg(false, "新密码至少 8 个字符，已取消本次重置。");
    return;
  }
  orgBusy.value = true;
  setOrgMsg(true, "");
  try {
    await resetMemberPassword(m.userId, pw);
    setOrgMsg(true, `已为 ${m.email} 设置临时密码，其全部会话已失效，首次登录须修改密码。`);
  } catch (e) {
    setOrgMsg(false, errText(e));
  } finally {
    orgBusy.value = false;
  }
}

// ---- 组织成员 / 邀请（D1）--------------------------------------------------
const members = ref<Member[]>([]);
const invites = ref<Invite[]>([]);
const orgBusy = ref(false);
const orgMsg = ref("");
const orgOk = ref(false);
const newRole = ref<OrgRole>("engineer");
const newTtl = ref<number>(7);
const copiedToken = ref("");

/** SQLite UTC 文本 → 本地时间字符串 */
function fmtTs(s: string): string {
  const t = Date.parse(s.includes("T") ? s : s.replace(" ", "T") + "Z");
  return Number.isNaN(t) ? s : new Date(t).toLocaleString();
}

function isExpired(expiresAt: string): boolean {
  const t = Date.parse(
    expiresAt.includes("T") ? expiresAt : expiresAt.replace(" ", "T") + "Z",
  );
  return !Number.isNaN(t) && t < Date.now();
}

function shortToken(t: string): string {
  return t.length > 14 ? `${t.slice(0, 8)}…${t.slice(-4)}` : t;
}

function setOrgMsg(ok: boolean, msg: string): void {
  orgOk.value = ok;
  orgMsg.value = msg;
}

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

async function refreshMembers(): Promise<void> {
  members.value = await listMembers();
}

async function refreshInvites(): Promise<void> {
  // GET /org/invites 仅 owner；非 owner 静默跳过
  if (!isOwner.value) return;
  invites.value = await listInvites();
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // 非安全上下文（HTTP）或无剪贴板权限 —— 退回 prompt 手工复制
    window.prompt("请复制邀请链接：", text);
    return false;
  }
}

async function copyInvite(token: string): Promise<void> {
  const auto = await copyText(inviteUrl(token));
  if (auto) {
    copiedToken.value = token;
    window.setTimeout(() => {
      if (copiedToken.value === token) copiedToken.value = "";
    }, 2000);
  }
}

async function onCreateInvite(): Promise<void> {
  if (orgBusy.value) return;
  orgBusy.value = true;
  setOrgMsg(true, "");
  try {
    const inv = await createInvite(newRole.value, newTtl.value);
    await refreshInvites();
    await copyInvite(inv.token);
    setOrgMsg(true, `邀请链接已生成${copiedToken.value === inv.token ? "并复制到剪贴板" : ""}，有效期 ${newTtl.value} 天。`);
  } catch (e) {
    setOrgMsg(false, errText(e));
  } finally {
    orgBusy.value = false;
  }
}

async function onRevoke(inv: Invite): Promise<void> {
  if (!window.confirm(`确定吊销该邀请链接（${shortToken(inv.token)}）吗？吊销后立即失效。`)) return;
  orgBusy.value = true;
  try {
    await revokeInvite(inv.token);
    await refreshInvites();
    setOrgMsg(true, "邀请已吊销。");
  } catch (e) {
    setOrgMsg(false, errText(e));
  } finally {
    orgBusy.value = false;
  }
}

async function onRoleChange(m: Member, role: OrgRole): Promise<void> {
  if (role === m.role) return;
  orgBusy.value = true;
  setOrgMsg(true, "");
  try {
    await setMemberRole(m.userId, role);
    await refreshMembers();
    setOrgMsg(true, `已将 ${m.email} 的角色改为 ${roleLabel(role)}。`);
  } catch (e) {
    setOrgMsg(false, errText(e));
    // 失败（如最后一个 owner 保护）—— 还原下拉框
    await refreshMembers().catch(() => {});
  } finally {
    orgBusy.value = false;
  }
}

async function onRemove(m: Member): Promise<void> {
  const self = m.userId === auth.user?.userId;
  const tip = self
    ? "退出组织后当前会话立即失效，需要重新登录。确定退出吗？"
    : `确定将 ${m.email} 移出本组织吗？其会话将立即失效。`;
  if (!window.confirm(tip)) return;
  orgBusy.value = true;
  try {
    await removeMember(m.userId);
    if (self) {
      // 服务端已删会话：本地清状态并回登录页
      await auth.logout().catch(() => {});
      await router.push({ name: "login" });
      return;
    }
    await refreshMembers();
    setOrgMsg(true, `已移除成员 ${m.email}。`);
  } catch (e) {
    setOrgMsg(false, errText(e));
  } finally {
    orgBusy.value = false;
  }
}

onMounted(async () => {
  try {
    await Promise.all([refreshMembers(), refreshInvites(), refreshSessions()]);
  } catch (e) {
    setOrgMsg(false, `加载成员信息失败：${errText(e)}`);
  }
});

// ---- legacy 导入 ----------------------------------------------------------
const legacyInput = ref<HTMLInputElement | null>(null);
const legacyFile = ref<File | null>(null);
const legacyBusy = ref(false);
const legacyMsg = ref("");
const legacyOk = ref(false);
const legacyReport = ref<LegacyReport | null>(null);

function onLegacyPick(): void {
  legacyReport.value = null;
  legacyMsg.value = "";
  legacyFile.value = legacyInput.value?.files?.[0] ?? null;
}

async function doImportLegacy(): Promise<void> {
  if (!legacyFile.value) return;
  legacyBusy.value = true;
  legacyMsg.value = "";
  try {
    const report = await importLegacy(legacyFile.value);
    legacyReport.value = report;
    const totalInserted = legacyRows.value.reduce((s, r) => s + r.value.inserted, 0);
    legacyMsg.value =
      `导入完成：共新增 ${totalInserted} 条，重复跳过 ` +
      `${legacyRows.value.reduce((s, r) => s + r.value.skipped, 0)} 条。` +
      (report.orphanRows > 0 ? ` 另有 ${report.orphanRows} 行因无归属项目被丢弃。` : "");
    legacyOk.value = true;
    // 刷新前端业务缓存，让新导入的数据立即出现在台账/项目页
    studio.$reset();
    await studio.bootstrap();
  } catch (e) {
    legacyOk.value = false;
    legacyMsg.value = e instanceof Error ? e.message : String(e);
  } finally {
    legacyBusy.value = false;
  }
}

const legacyRows = computed<
  { key: string; label: string; value: LegacyTableReport }[]
>(() => {
  const r = legacyReport.value;
  if (!r) return [];
  return [
    { key: "projects", label: "项目", value: r.projects },
    { key: "instruments", label: "仪表", value: r.instruments },
    { key: "sifs", label: "SIF 回路", value: r.sifs },
    { key: "diagrams", label: "联锁图纸", value: r.diagrams },
    { key: "links", label: "回路-仪表关联", value: r.links },
    { key: "bypasses", label: "旁路记录", value: r.bypasses },
    { key: "tickets", label: "检维修工单", value: r.tickets },
    { key: "auditLogs", label: "审计日志", value: r.auditLogs },
  ];
});

// ---- 备份 -----------------------------------------------------------------
const adminToken = ref("");
const backupBusy = ref(false);
const backupMsg = ref("");
const backupOk = ref(false);

async function doBackup(): Promise<void> {
  backupBusy.value = true;
  backupMsg.value = "";
  try {
    await downloadBackup(adminToken.value);
    backupOk.value = true;
    backupMsg.value = "备份已生成并开始下载。";
  } catch (e) {
    backupOk.value = false;
    backupMsg.value = formatAdminError(e);
  } finally {
    backupBusy.value = false;
  }
}

// ---- 恢复 -----------------------------------------------------------------
const restoreInput = ref<HTMLInputElement | null>(null);
const restoreFile = ref<File | null>(null);
const restoreAck = ref(false);
const restoreBusy = ref(false);
const restoreMsg = ref("");
const restoreOk = ref(false);
const restoreReport = ref<RestoreReport | null>(null);

function onRestorePick(): void {
  restoreReport.value = null;
  restoreMsg.value = "";
  restoreFile.value = restoreInput.value?.files?.[0] ?? null;
  // 换文件后要求重新勾选确认
  restoreAck.value = false;
}

async function doRestore(): Promise<void> {
  if (!restoreFile.value || !restoreAck.value) return;
  restoreBusy.value = true;
  restoreMsg.value = "";
  try {
    const report = await restoreBackup(adminToken.value, restoreFile.value);
    restoreReport.value = report;
    restoreOk.value = true;
    restoreMsg.value =
      "整库恢复完成。所有组织数据已被备份内容替换，如页面异常请退出后重新登录。";
  } catch (e) {
    restoreOk.value = false;
    restoreMsg.value = formatAdminError(e);
  } finally {
    restoreBusy.value = false;
  }
}

function formatAdminError(e: unknown): string {
  if (e instanceof Error) {
    const status = (e as { status?: number }).status;
    if (status === 404) return "端点不存在（404）：服务器未配置 SIF_ADMIN_TOKEN。";
    if (status === 401) return "管理员令牌无效（401），请检查后重试。";
    return e.message;
  }
  return String(e);
}
</script>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: var(--s-4);
}

.card {
  background: var(--paper);
  border: var(--rule-fine) solid var(--rule);
  padding: var(--s-5);
}

/* 标题块（与 AuditCenter 同款） */
.head {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: var(--s-5);
}
.head-eyebrow {
  font-family: var(--font-mono);
  font-size: var(--fs-micro);
  letter-spacing: 0.14em;
  color: var(--ink-3);
}
.head-title {
  margin: var(--s-2) 0;
  font-family: var(--font-title);
  font-size: var(--fs-xl);
  color: var(--ink-1);
}
.head-sub {
  font-size: var(--fs-sm);
  color: var(--ink-3);
}
.head-block {
  min-width: 200px;
  border: var(--rule-hair) solid var(--rule-2);
}
.head-row {
  display: flex;
  justify-content: space-between;
  padding: var(--s-2) var(--s-3);
  border-bottom: var(--rule-hair) solid var(--rule-3);
  font-size: var(--fs-xs);
}
.head-row:last-child {
  border-bottom: 0;
}
.head-key {
  font-family: var(--font-mono);
  color: var(--ink-4);
  letter-spacing: 0.1em;
}
.head-val {
  color: var(--ink-1);
}

.card-title {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  font-family: var(--font-title);
  font-size: var(--fs-md);
  padding-bottom: var(--s-3);
  margin-bottom: var(--s-4);
  border-bottom: var(--rule-fine) solid var(--rule);
}

.tag {
  font-family: var(--font-mono);
  font-size: var(--fs-micro);
  letter-spacing: 0.1em;
  padding: 2px 8px;
  border: var(--rule-hair) solid var(--rule);
}
.tag-ok {
  background: var(--ok-bg);
  color: var(--ok);
  border-color: var(--ok);
}
.tag-lock {
  background: var(--paper-3);
  color: var(--ink-3);
}
.tag-op {
  background: var(--warn-bg);
  color: var(--warn);
  border-color: var(--warn);
}

.hint {
  font-size: var(--fs-sm);
  line-height: var(--lh-normal);
  color: var(--ink-2);
  margin: 0 0 var(--s-4);
}
.hint.tight {
  margin-bottom: var(--s-3);
}
.hint.locked {
  color: var(--ink-3);
  margin-bottom: 0;
}
.hint code,
.field .lbl + input,
.cli {
  font-family: var(--font-mono);
}
code {
  background: var(--paper-3);
  padding: 0 4px;
  font-size: 0.92em;
}

.row {
  display: flex;
  align-items: center;
  gap: var(--s-3);
  margin-bottom: var(--s-3);
  flex-wrap: wrap;
}

.inline-hint {
  font-size: var(--fs-xs);
  color: var(--ink-4);
}

.file-input {
  font-size: var(--fs-xs);
  font-family: var(--font-mono);
  border: var(--rule-hair) solid var(--rule-2);
  padding: var(--s-2);
  background: var(--paper);
  flex: 1 1 320px;
  max-width: 520px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--s-2);
  margin-bottom: var(--s-4);
  max-width: 420px;
}
.lbl {
  font-family: var(--font-mono);
  font-size: var(--fs-micro);
  letter-spacing: 0.1em;
  color: var(--ink-3);
}
.field input {
  border: var(--rule-fine) solid var(--rule);
  padding: var(--s-2) var(--s-3);
  font-size: var(--fs-sm);
  outline: none;
}
.field input:focus {
  border-color: var(--acc);
}

.btn {
  font-family: var(--font-title);
  font-size: var(--fs-sm);
  padding: var(--s-2) var(--s-5);
  border: var(--rule-fine) solid var(--rule);
  background: var(--paper);
  color: var(--ink-1);
  cursor: pointer;
}
.btn:disabled {
  color: var(--ink-4);
  border-color: var(--rule-2);
  cursor: not-allowed;
}
.btn.primary:not(:disabled) {
  background: var(--acc);
  border-color: var(--acc);
  color: #fff;
}
.btn.danger:not(:disabled) {
  background: var(--fin);
  border-color: var(--fin);
  color: #fff;
}

.banner {
  font-size: var(--fs-sm);
  border: var(--rule-fine) solid;
  padding: var(--s-2) var(--s-3);
  margin-bottom: var(--s-3);
  white-space: pre-wrap;
}
.banner.ok {
  background: var(--ok-bg);
  border-color: var(--ok);
  color: var(--ok);
}
.banner.err {
  background: var(--err-bg);
  border-color: var(--err);
  color: var(--err);
}

.report {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--fs-xs);
  margin-top: var(--s-3);
}
.report th,
.report td {
  border: var(--rule-hair) solid var(--rule-2);
  padding: var(--s-2) var(--s-3);
  text-align: left;
}
.report th {
  font-family: var(--font-mono);
  font-weight: 600;
  background: var(--paper-2);
  letter-spacing: 0.06em;
}
.report .num {
  text-align: right;
  width: 220px;
}
.report .orphan td {
  background: var(--warn-bg);
  color: var(--warn);
}

.divider {
  border-top: var(--rule-hair) solid var(--rule-2);
  margin: var(--s-4) 0;
}

.danger-box {
  border: var(--rule-mid) solid var(--fin);
  background: var(--fin-bg);
  padding: var(--s-4);
}
.danger-title {
  font-family: var(--font-title);
  color: var(--fin);
  font-size: var(--fs-sm);
  letter-spacing: 0.08em;
  margin-bottom: var(--s-3);
}
.ack {
  display: flex;
  align-items: flex-start;
  gap: var(--s-2);
  font-size: var(--fs-xs);
  color: var(--ink-2);
  margin: var(--s-2) 0 var(--s-4);
}
.ack input {
  margin-top: 2px;
}

.cli {
  background: var(--ink-1);
  color: #d8e2ec;
  font-size: var(--fs-xs);
  line-height: 1.7;
  padding: var(--s-4);
  overflow-x: auto;
  white-space: pre;
  margin: 0 0 var(--s-3);
}

/* D1 —— 成员 / 邀请 */
.report.members td,
.report.members th {
  vertical-align: middle;
}
.role-select {
  border: var(--rule-hair) solid var(--rule-2);
  background: var(--paper);
  color: var(--ink-1);
  font-size: var(--fs-xs);
  padding: 3px 6px;
  border-radius: 0;
}
.role-select:disabled {
  opacity: 0.6;
}
.inline-field {
  display: flex;
  align-items: center;
  gap: var(--s-2);
  font-size: var(--fs-xs);
}
.inline-field .lbl {
  margin: 0;
  white-space: nowrap;
}
.btn-link {
  background: none;
  border: 0;
  padding: 0;
  font-family: var(--font-title);
  font-size: var(--fs-xs);
  color: var(--ink-1);
  text-decoration: underline;
  cursor: pointer;
}
.btn-link.danger:not(:disabled) {
  color: var(--fin);
}
.btn-link:disabled {
  color: var(--ink-4);
  cursor: not-allowed;
  text-decoration: none;
}
.link-copy {
  background: none;
  border: 0;
  padding: 0;
  font-size: var(--fs-xs);
  color: var(--ink-1);
  cursor: pointer;
  text-decoration: underline dotted;
}
.link-copy:disabled {
  color: var(--ink-4);
  cursor: default;
}
.copied-hint {
  margin-left: var(--s-2);
  font-size: var(--fs-xs);
  color: var(--ok);
}
.st-active {
  color: var(--ok);
  font-size: var(--fs-xs);
}
.st-revoked,
.st-expired {
  color: var(--ink-4);
  font-size: var(--fs-xs);
}

/* E3/E4 — 账号安全 / 会话 */
.acct-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--s-4);
}
.acct-block {
  border: var(--rule-fine) solid var(--ink-3);
  padding: var(--s-3);
}
.acct-h {
  font-size: var(--fs-xs);
  letter-spacing: 0.12em;
  color: var(--ink-4);
  margin-bottom: var(--s-2);
}
.acct-block .btn { margin-top: var(--s-2); }
.banner.force-pw {
  font-weight: 700;
  color: var(--ink-1);
  background: var(--paper-3);
  border: var(--rule-mid) solid var(--acc);
  border-left-width: 4px;
}
.role-tag-owner { color: var(--acc); border-color: var(--acc); }
.role-tag-engineer { color: var(--ok); border-color: var(--ok); }
.role-tag-viewer { color: var(--ink-4); }
.sess-current td { background: var(--paper-3); }
.sess-ua { max-width: 220px; }
.muted { color: var(--ink-4); }
.member-ops { white-space: nowrap; }
.member-ops .btn-link { margin-left: var(--s-2); }
.member-ops .btn-link:first-child { margin-left: 0; }
</style>
