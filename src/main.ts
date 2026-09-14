import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { router } from "./router";
import "./styles/global.css";
import { useAuthStore } from "./stores/auth";
import { useStudioStore } from "./stores/studio";

function bootstrap() {
  const app = createApp(App);
  const pinia = createPinia();
  app.use(pinia);
  app.use(router);

  // 先挂载：即使认证检查 / 数据层初始化失败，
  // 用户也能看到完整界面（登录页 / 错误横幅），而不是一片白屏。
  app.mount("#app");

  // B7 — 路由守卫会先 ensureChecked（幂等共享 Promise）；
  // 这里在检查完成后按登录态拉业务数据。未登录时跳过，
  // 登录/注册成功后由 auth store 的 ensureStudioData() 触发。
  const auth = useAuthStore();
  void auth.ensureChecked().then(() => {
    if (!auth.user) return;
    useStudioStore()
      .bootstrap()
      .catch((err) => {
        // 错误已写入 store.lastError，界面顶部会显示错误横幅 + 重试按钮
        // eslint-disable-next-line no-console
        console.error("[sif-studio] bootstrap failed", err);
      });
  });
}

bootstrap();
