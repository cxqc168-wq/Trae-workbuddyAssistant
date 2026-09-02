import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    // Rust 构建产物目录不应被 vite 监视，否则编译期锁定的 .exe 会触发 EBUSY 导致 dev 崩溃
    watch: {
      ignored: ['**/src-tauri/target/**'],
    },
  },
  build: {
    target: 'esnext',
    sourcemap: false,
  },
});