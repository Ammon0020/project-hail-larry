import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { initTheme } from '@/lib/theme'

// Apply the persisted theme preference before render (defaults to dark per
// Blueprint Sec 17). Handles 'dark' | 'light' | 'system'.
initTheme()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
