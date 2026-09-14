import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// 在线版开发服务器：5173 跑 Vite，/api 反代到 axum 后端（默认 8080）。
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: "0.0.0.0",
    proxy: {
      "/api": {
        target: process.env.SIF_API_TARGET ?? "http://127.0.0.1:8080",
        changeOrigin: true,
      },
    },
    watch: { ignored: ["**/server/**"] },
  },
  build: {
    target: "es2021",
    outDir: "dist",
    minify: "esbuild",
    sourcemap: false,
    // pdfmake（~1.0MB）与中文字体子集（~2.1MB）都是**懒加载** chunk
    // （仅点「导出 PDF」时才拉），不参与首屏。故抬高告警阈值避免噪音。
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        // 让 pdfmake 与字体子集各自独立成 chunk，便于浏览器长缓存
        manualChunks(id) {
          if (id.includes("node_modules/pdfmake")) return "pdfmake";
          return undefined;
        },
      },
    },
  },
  // 同时支持后续 M0 编辑器以 iframe 嵌入；public/editor.html 不打包
  publicDir: "public",
});
