/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useRef, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { FallbackViewer } from './simple'

export function EpubViewer({ url }: { url: string }) {
  const viewerRef = useRef<HTMLDivElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    let rendition: import('epubjs').Rendition | null = null

    async function init() {
      if (cancelled || !viewerRef.current) return
      const ePub = (await import('epubjs')).default
      const book = ePub(url)
      rendition = book.renderTo(viewerRef.current, { width: '100%', height: '100%' })
      await rendition.display()
      if (!cancelled) setLoading(false)
    }

    init().catch((err) => {
      if (!cancelled) {
        setError(err instanceof Error ? err.message : String(err))
        setLoading(false)
      }
    })
    return () => {
      cancelled = true
      if (rendition) rendition.destroy()
    }
  }, [url])

  if (error) {
    return <FallbackViewer url={url} name="" message={`Failed to render EPUB: ${error}`} />
  }
  return (
    <div className="w-full h-full relative bg-white">
      {loading && (
        <div className="absolute inset-0 flex items-center justify-center">
          <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
        </div>
      )}
      <div ref={viewerRef} className="w-full h-full" />
    </div>
  )
}

