import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
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
    <App />
  </StrictMode>,
)
