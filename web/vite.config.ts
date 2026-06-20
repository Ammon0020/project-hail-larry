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
  server: {
    // Proxy API and WebSocket routes to the Go backend during dev.
    // In production, the Go server serves the frontend directly via go:embed.
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
