import { createRouter, createWebHashHistory, type RouteRecordRaw } from "vue-router";
import Home from "./views/Home.vue";
import Instruments from "./views/Instruments.vue";
import SifDashboard from "./views/SifDashboard.vue";
import ProjectsView from "./views/ProjectsView.vue";
import DiagramEditor from "./views/DiagramEditor.vue";
import ImportWizard from "./views/ImportWizard.vue";
import BypassLedger from "./views/BypassLedger.vue";
import AuditCenter from "./views/AuditCenter.vue";  // M2.5 — 审计包导出
import LoginView from "./views/LoginView.vue";      // B7 — 认证
import RegisterView from "./views/RegisterView.vue";
import SettingsView from "./views/SettingsView.vue"; // C6 — 系统设置
import { useAuthStore } from "./stores/auth";

const routes: RouteRecordRaw[] = [
  // 公开页（未登录可访问；已登录访问会被送回首页）
  { path: "/login", name: "login", component: LoginView, meta: { title: "登录", public: true } },
  { path: "/register", name: "register", component: RegisterView, meta: { title: "注册新组织", public: true } },

  { path: "/", name: "home", component: Home, meta: { title: "工作台首页" } },
  { path: "/instruments", name: "instruments", component: Instruments, meta: { title: "仪表台账" } },
  { path: "/import", name: "import", component: ImportWizard, meta: { title: "仪表导入", requiresWrite: true } },
  { path: "/sifs", name: "sifs", component: SifDashboard, meta: { title: "SIF 汇总" } },
  { path: "/projects", name: "projects", component: ProjectsView, meta: { title: "项目" } },
  { path: "/bypass", name: "bypass", component: BypassLedger, meta: { title: "旁路授权台账" } },
  // M2.5 — 审计中心（导出 audit_log 为 CSV）
  { path: "/audit", name: "audit", component: AuditCenter, meta: { title: "审计中心" } },
  // C6 — 系统设置（legacy 导入 / 整库备份恢复）
  { path: "/settings", name: "settings", component: SettingsView, meta: { title: "系统设置" } },
  {
    path: "/diagram/:diagramId(\\d+)",
    name: "diagram",
    component: DiagramEditor,
    props: true,
    meta: { title: "联锁图编辑" },
  },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

// ---------------------------------------------------------------------------
// B7 — 全局路由守卫
//
// 首次导航时做一次会话检查（ensureChecked 幂等，与 main.ts 共享同一 Promise）：
//   - 公开页：已登录则回首页（避免登录后再看登录页）
//   - 业务页：未登录 → 跳登录页并携带 redirect，登录后回到原页
// ---------------------------------------------------------------------------
router.beforeEach(async (to) => {
  const auth = useAuthStore();
  await auth.ensureChecked();

  if (to.meta.public) {
    // 已登录用户打开邀请链接：带 invite 参数回首页，由 App.vue 走接受邀请流程
    if (auth.user) {
      return to.query.invite ? { path: "/", query: to.query } : { path: "/" };
    }
    return true;
  }
  if (!auth.user) {
    return {
      name: "login",
      query: to.fullPath === "/" ? {} : { redirect: to.fullPath },
    };
  }
  // E1：只读角色不能进入写操作页面（服务端对每条写命令另有强制 403）
  if (to.meta.requiresWrite && auth.isViewer) {
    return { path: "/" };
  }
  // F2：管理员重置过密码 → 除设置页外一律拦截，先完成自助改密
  if (auth.mustChangePassword && to.name !== "settings") {
    return { name: "settings" };
  }
  return true;
});
