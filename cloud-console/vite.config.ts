import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// The console is served by supertask-cloud-server under /admin/, so the build must
// emit absolute /admin/* asset URLs. Dev keeps the same prefix and proxies the API,
// which keeps the browser same-origin and therefore free of CORS.
const apiTarget = process.env.SUPERTASK_API_TARGET ?? "http://127.0.0.1:8787";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "/admin/",
  build: {
    outDir: "dist",
    // 沙箱的安全删除会拦截 vite 清空 dist 的 trash 操作；不清空，避免假失败（index.html 按哈希引用资源）。
    emptyOutDir: false,
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    // 显式 IPv4 loopback：host=false 时 vite 按 localhost 解析，Windows 上落到 ::1，
    // 文档与 start-cloud.ps1 用的 127.0.0.1:1430 会连不上。不要改成 true（会暴露到局域网）。
    host: "127.0.0.1",
    proxy: {
      "/admin/api": { target: apiTarget, changeOrigin: false },
    },
  },
});
