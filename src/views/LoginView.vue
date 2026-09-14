<template>
  <div class="auth-page">
    <form class="auth-card" @submit.prevent="submit">
      <div class="auth-mark mono">SIF STUDIO · 联锁工坊</div>
      <h1 class="auth-title">登录</h1>

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
        <span class="field-key mono">PASSWORD</span>
        <input
          v-model="password"
          type="password"
          required
          autocomplete="current-password"
          placeholder="••••••••"
          :disabled="busy"
        />
      </label>

      <div class="auth-err mono" v-if="error">{{ error }}</div>

      <button class="auth-submit" type="submit" :disabled="busy">
        {{ busy ? "登录中…" : "登 录" }}
      </button>

      <div class="auth-alt">
        还没有账号？
        <router-link :to="{ name: 'register', query: route.query }">注册新组织</router-link>
      </div>
    </form>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useAuthStore } from "../stores/auth";

const auth = useAuthStore();
const route = useRoute();
const router = useRouter();

const email = ref("");
const password = ref("");
const busy = ref(false);
const error = ref("");

/** 只接受站内路径，防 open redirect */
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
    await auth.login(email.value, password.value);
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
  width: 380px;
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
  margin: 0 0 var(--s-4);
  padding-bottom: var(--s-2);
  border-bottom: var(--rule-mid) solid var(--ink-1);
  letter-spacing: 0.04em;
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
