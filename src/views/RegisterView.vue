<template>
  <div class="auth-page">
    <form class="auth-card" @submit.prevent="submit">
      <div class="auth-mark mono">SIF STUDIO · 联锁工坊</div>
      <h1 class="auth-title">{{ invite ? "加入组织" : "注册新组织" }}</h1>

      <!-- 邀请链接：加入指定组织 -->
      <div v-if="invite" class="invite-banner">
        <div class="invite-title">邀请链接 · INVITE</div>
        <div>
          你将加入组织 <b>{{ invite.orgName }}</b>，角色为
          <b>{{ roleText(invite.role) }}</b>。
        </div>
        <div class="invite-exp">有效期至 {{ fmtTime(invite.expiresAt) }}</div>
      </div>
      <div v-else-if="inviteToken && inviteChecked" class="auth-err mono">
        邀请链接无效或已过期，请向组织管理员重新索取链接后再注册。
      </div>

      <p class="auth-hint" v-if="!invite">
        第一个注册的用户将接管本数据库的默认组织（承接既有台账数据）；
        之后的注册会创建全新组织，注册人即组织 Owner。
      </p>

      <label class="field">
        <span class="field-key mono">EMAIL</span>
        <input
          v-model.trim="email"
          type="email"
          required
          autocomplete="username"
          placeholder="you@company.com"
          :disabled="busy"
        />
      </label>
      <label class="field">
        <span class="field-key mono">PASSWORD（至少 8 位）</span>
        <input
          v-model="password"
          type="password"
          required
          minlength="8"
          autocomplete="new-password"
          placeholder="••••••••"
          :disabled="busy"
        />
      </label>
      <label class="field">
        <span class="field-key mono">DISPLAY NAME（可选）</span>
        <input
          v-model.trim="displayName"
          type="text"
          autocomplete="nickname"
          placeholder="如：张工"
          :disabled="busy"
        />
      </label>
      <label class="field" v-if="!invite">
        <span class="field-key mono">ORG NAME（组织名，可选）</span>
        <input
          v-model.trim="orgName"
          type="text"
          placeholder="如：化一车间仪表组"
          :disabled="busy"
        />
      </label>

      <div class="auth-err mono" v-if="error">{{ error }}</div>

      <button class="auth-submit" type="submit" :disabled="busy">
        {{ busy ? "注册中…" : "注 册" }}
      </button>

      <div class="auth-alt">
        已有账号？
        <router-link :to="{ name: 'login', query: route.query }">直接登录</router-link>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";
import { invitePreview, roleLabel, type InvitePreview } from "../api/org";

const auth = useAuthStore();
const route = useRoute();
const router = useRouter();

const email = ref("");
const password = ref("");
const displayName = ref("");
const orgName = ref("");
const busy = ref(false);
const error = ref("");

// ---- 邀请链接（?invite=<token>）-------------------------------------------
const inviteToken = ref("");
const inviteChecked = ref(false);
const invite = ref<InvitePreview | null>(null);

function roleText(role: string): string {
  return roleLabel(role);
}

/** SQLite UTC 文本 → 本地可读时间；解析失败原样返回 */
function fmtTime(s: string): string {
  // 后端给的是 "YYYY-MM-DD HH:MM:SS"（UTC），补 Z 后交给 toLocaleString
  const t = Date.parse(s.includes("T") ? s : s.replace(" ", "T") + "Z");
  return Number.isNaN(t) ? s : new Date(t).toLocaleString();
}

onMounted(async () => {
  const q = route.query.invite;
  if (typeof q !== "string" || !q.trim()) return;
  inviteToken.value = q.trim();
  try {
    invite.value = await invitePreview(inviteToken.value);
  } catch {
    invite.value = null;
  } finally {
    inviteChecked.value = true;
  }
});

function safeRedirect(): string {
  const r = route.query.redirect;
  if (typeof r === "string" && r.startsWith("/") && !r.startsWith("//")) return r;
  return "/";
}

async function submit() {
  if (busy.value) return;
  busy.value = true;
  error.value = "";
  try {
    await auth.register({
      email: email.value,
      password: password.value,
      displayName: displayName.value,
      orgName: orgName.value,
      // 仅在预览有效的情况下携带邀请
      inviteToken: invite.value ? inviteToken.value : undefined,
    });
    await router.push(safeRedirect());
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    busy.value = false;
  }
}
</script>

<style scoped>
.auth-page {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 70vh;
}

.auth-card {
  width: 420px;
  max-width: 100%;
  background: var(--paper);
  border: var(--rule-bold) solid var(--ink-1);
  padding: var(--s-5);
}

.auth-mark {
  font-size: var(--fs-micro);
  letter-spacing: 0.16em;
  color: var(--ink-4);
  text-transform: uppercase;
  margin-bottom: var(--s-2);
}

.auth-title {
  font-family: var(--font-title);
  font-size: var(--fs-md);
  font-weight: 700;
  color: var(--ink-1);
  margin: 0 0 var(--s-2);
  padding-bottom: var(--s-2);
  border-bottom: var(--rule-mid) solid var(--ink-1);
  letter-spacing: 0.04em;
}

.auth-hint {
  font-size: var(--fs-xs);
  color: var(--ink-2);
  line-height: 1.6;
  margin: 0 0 var(--s-4);
  padding: var(--s-2) var(--s-3);
  background: var(--paper-3);
  border-left: 4px solid var(--acc);
}

.invite-banner {
  font-size: var(--fs-xs);
  line-height: 1.7;
  color: var(--ink-1);
  background: var(--ok-bg, var(--paper-3));
  border: var(--rule-fine) solid var(--acc);
  border-left: 4px solid var(--acc);
  padding: var(--s-3);
  margin: 0 0 var(--s-4);
}
.invite-title {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0.16em;
  color: var(--ink-4);
  margin-bottom: 4px;
}
.invite-exp {
  margin-top: 4px;
  color: var(--ink-3);
  font-family: var(--font-mono);
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: var(--s-3);
}
.field-key {
  font-size: 9px;
  letter-spacing: 0.16em;
  color: var(--ink-4);
  text-transform: uppercase;
}
.field input {
  border: var(--rule-mid) solid var(--rule-2);
  border-radius: 0;
  padding: 8px 10px;
  font-family: var(--font-mono);
  font-size: var(--fs-sm);
  background: var(--paper-2);
  color: var(--ink-1);
}
.field input:focus {
  outline: 2px solid var(--acc);
  outline-offset: -2px;
}
.field input:disabled {
  opacity: 0.6;
}

.auth-err {
  padding: var(--s-2) var(--s-3);
  margin-bottom: var(--s-3);
  background: var(--err-bg);
  border: var(--rule-fine) solid var(--err);
  color: var(--err);
  font-size: var(--fs-xs);
  word-break: break-all;
}

.auth-submit {
  width: 100%;
  padding: 10px;
  background: var(--ink-1);
  color: var(--paper);
  border: var(--rule-mid) solid var(--ink-1);
  border-radius: 0;
  font-family: var(--font-title);
  font-size: var(--fs-sm);
  font-weight: 700;
  letter-spacing: 0.2em;
  cursor: pointer;
}
.auth-submit:hover:not(:disabled) {
  background: var(--paper-3);
  color: var(--ink-1);
}
.auth-submit:disabled {
  opacity: 0.6;
  cursor: default;
}

.auth-alt {
  margin-top: var(--s-3);
  font-size: var(--fs-xs);
  color: var(--ink-2);
}
.auth-alt a {
  color: var(--ink-1);
  font-weight: 700;
}
</style>
