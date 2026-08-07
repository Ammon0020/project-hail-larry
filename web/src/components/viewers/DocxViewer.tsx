/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useState } from 'react'
import DOMPurify from 'dompurify'
import { Loader2 } from 'lucide-react'
import { FallbackViewer } from './simple'

export function DocxViewer({ url, name }: { url: string; name: string }) {
  const [state, setState] = useState<{ html: string | null; error: string | null }>({ html: null, error: null })

  useEffect(() => {
    let cancelled = false
    // Fetch the .docx as an ArrayBuffer, then convert to HTML with mammoth.
    // mammoth is loaded dynamically so it doesn't bloat the main bundle —
    // only users who open a .docx pay the ~150KB import cost.
    fetch(url)
      .then((res) => {
        if (!res.ok) throw new Error(`HTTP ${res.status}`)
        return res.arrayBuffer()
      })
      .then((buf) => import('mammoth').then((m) => m.convertToHtml({ arrayBuffer: buf })))
      .then((result) => {
        // Sanitize before injecting: mammoth escapes text by default, but
        // a library bug or unescaped code path could allow script injection.
        if (!cancelled)
          setState({ html: DOMPurify.sanitize(result.value, { USE_PROFILES: { html: true } }), error: null })
      })
      .catch((err) => {
        if (!cancelled) setState({ html: null, error: err instanceof Error ? err.message : String(err) })
      })
    return () => { cancelled = true }
  }, [url])

  const { html, error } = state

  if (error) {
    return (
      <FallbackViewer url={url} name={name} message={`Failed to render DOCX: ${error}`} />
    )
  }
  if (html === null) {
    return (
      <div className="flex flex-col items-center gap-3 text-muted-foreground">
        <Loader2 className="w-8 h-8 animate-spin" />
        <p className="text-sm">Converting {name}…</p>
      </div>
    )
  }
  return (
    <div className="w-full h-full overflow-auto bg-white text-black dark:bg-white dark:text-black">
      {/* prose-docx: a scoped wrapper so the DOCX HTML gets readable margins
          without polluting the app's Tailwind prose styles. */}
      <div
        className="prose-docx max-w-3xl mx-auto p-8"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  )
}

