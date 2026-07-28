import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { ErrorBoundary } from '@/components/ErrorBoundary'
import { initTheme } from '@/lib/theme'

// Apply the persisted theme preference before render (defaults to dark per
// Blueprint Sec 17). Handles 'dark' | 'light' | 'system'. Capture the cleanup
// and dispose it on HMR so the media-query listener doesn't leak across reloads.
const cleanupTheme = initTheme()
if (import.meta.hot) {
  import.meta.hot.dispose(cleanupTheme)
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
)

// Register the offline shell service worker in production only. In dev the
// Vite dev server (HMR, proxied /api and /ws) must not be intercepted, so we
// skip registration there entirely. The SW (public/sw.js) caches the SPA
// shell + hashed bundles so a reload while the LAN daemon is unreachable
// still renders the last shell (login/pair UI); /api and /ws are never
// cached. autoUpdate behavior comes from skipWaiting()+clients.claim() in the
// SW itself, so no client-side update messaging is needed here.
if ('serviceWorker' in navigator && import.meta.env.PROD) {
  window.addEventListener('load', () => {
    navigator.serviceWorker
      .register('/sw.js')
      .catch((err) => console.warn('[sw] registration failed:', err))
  })
}
