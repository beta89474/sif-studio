/**
 * 认证状态 store（阶段 B）
 *
 * - user === null 即未登录；ensureChecked() 幂等（共享同一 Promise），
 *   路由守卫与 main.ts 可并发调用。
 * - 登录/注册成功后若业务数据层尚未 bootstrap，则顺手拉一次
 *   （登出会 $reset 业务 store，再登录时重建）。
 * - 会话过期的被动感知：rpc.ts 收到 401 派发 window 事件 "sif:unauthorized"，
 *   App.vue 统一监听并跳登录页。
 */

import { defineStore } from "pinia";
import * as api from "../api/auth";
import type { MeInfo, RegisterInput } from "../api/auth";
import { useStudioStore } from "./studio";

/** ensureChecked 的共享 Promise（模块级，防并发双请求） */
let checkPromise: Promise<void> | null = null;

export const useAuthStore = defineStore("auth", {
  state: () => ({
    user: null as MeInfo | null,
    checked: false,
  }),

  getters: {
    /** 侧栏/审计 actor 展示：优先显示名，空则邮箱 */
    actorName(state): string {
      const u = state.user;
      if (!u) return "";
      return u.displayName || u.email;
    },

    /** viewer = 只读角色（E1：UI 隐藏全部写入口，后端另有强制 403） */
    isViewer(state): boolean {
      return state.user?.role === "viewer";
    },

    /** 是否有写权限（owner / engineer） */
    canWrite(): boolean {
      return !this.isViewer;
    },

    isOwner(state): boolean {
      return state.user?.role === "owner";
    },

    /** 是否存在可切换的其它组织 */
    hasMultipleOrgs(state): boolean {
      return (state.user?.orgs.length ?? 0) > 1;
    },

    /** F2：被管理员重置过密码，必须先自助改密 */
    mustChangePassword(state): boolean {
      return state.user?.mustChangePassword === true;
    },
  },

  actions: {
    /** 确保做过一次会话检查（GET /api/auth/me）；无论成败都落定 checked */
    ensureChecked(): Promise<void> {
      if (!checkPromise) {
        checkPromise = (async () => {
          try {
            this.user = await api.me();
          } catch {
            // 401（未登录）/ 网络 5xx —— 都先按未登录处理，进页面后再按需报错
            this.user = null;
          } finally {
            this.checked = true;
          }
        })();
      }
      return checkPromise;
    },

    async login(email: string, password: string): Promise<void> {
      this.user = await api.login(email, password);
      await this.ensureStudioData();
    },

    async register(input: RegisterInput): Promise<void> {
      this.user = await api.register(input);
      await this.ensureStudioData();
    },

    /** 登录成功后拉业务数据；错误已写入 studio.lastError（横幅可见） */
    async ensureStudioData(): Promise<void> {
      const studio = useStudioStore();
      if (!studio.ready) {
        try {
          await studio.bootstrap();
        } catch (e) {
          // eslint-disable-next-line no-console
          console.error("[sif-studio] bootstrap after login failed", e);
        }
      }
    },

    /** E3：更新显示名 */
    async updateProfile(displayName: string): Promise<void> {
      this.user = await api.updateProfile(displayName);
    },

    /** 重新拉取 /auth/me（接受跨组织邀请后刷新 orgs 列表等） */
    async refreshMe(): Promise<void> {
      this.user = await api.me();
    },

    /** F2：自助改密。成功后后端已踢掉其它会话并清除强制改密标记，
     *  这里重新拉 /me 让路由守卫即时放行。 */
    async changePassword(oldPassword: string, newPassword: string): Promise<void> {
      await api.changePassword(oldPassword, newPassword);
      await this.refreshMe();
    },

    /** E4：切换到自己所属的另一个组织。
     *  后端会轮换会话并返回新身份；业务数据全部按 org 隔离，必须重新 bootstrap。 */
    async switchOrg(orgId: number): Promise<void> {
      this.user = await api.switchOrg(orgId);
      const studio = useStudioStore();
      studio.stopOverduePolling();
      studio.$reset();
      await this.ensureStudioData();
    },

    async logout(): Promise<void> {
      try {
        await api.logout();
      } finally {
        this.user = null;
        checkPromise = null;
        // 清空业务状态（ready=false → 下次登录重新 bootstrap）
        const studio = useStudioStore();
        studio.stopOverduePolling();
        studio.$reset();
      }
    },
  },
});
