import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],

  // Vite 开发服务器配置（Tauri 固定端口）
  // 注意：勿使用 1420 等可能落入 Windows Hyper-V/WSL 动态保留端口段（如 1387-1486）的端口，
  // 否则绑定报 EACCES: permission denied；改端口时须同步修改 tauri.conf.json 的 devUrl
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // 忽略 Rust 后端目录，避免无谓重启
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
