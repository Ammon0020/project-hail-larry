import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    // The single-bundle SPA (React + CodeMirror + lucide) is ~800 kB, which is
    // fine for a LAN-served app. Raise the warning limit so the build does not
    // emit a stderr warning — build.ps1 runs with ErrorActionPreference=Stop and
    // would otherwise treat that warning as a fatal error.
    chunkSizeWarningLimit: 2000,
  },
  server: {
    // Proxy API and WebSocket routes to the Rust daemon during dev.
    // In production, the Rust daemon serves the frontend directly via
    // build.rs embed (see build.rs).
    // strictPort: fail loudly if 5173 is taken instead of silently moving to
    // 5174+, which would leave the browser on a stale Vite from a previous
    // dev.sh run and cause perpetual "Reconnecting" (proxy → dead daemon).
    strictPort: true,
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://localhost:7337',
        changeOrigin: true,
      },
      '/ws': {
        target: 'ws://localhost:7337',
        ws: true,
      },
      '/health': {
        target: 'http://localhost:7337',
        changeOrigin: true,
      },
    },
  },
})
