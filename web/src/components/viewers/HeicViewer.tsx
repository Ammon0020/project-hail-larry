/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { FallbackViewer } from './simple'

export function HeicViewer({ url, name }: { url: string; name: string }) {
  const [pngUrl, setPngUrl] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    let objectUrl: string | null = null
    fetch(url)
      .then((res) => res.blob())
      .then(async (blob) => {
        if (cancelled) return
        const heic2any = (await import('heic2any')).default
        const pngBlob = await heic2any({ blob, toType: 'image/png' })
        if (cancelled) return
        objectUrl = URL.createObjectURL(pngBlob as Blob)
        setPngUrl(objectUrl)
        setLoading(false)
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err))
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [url])

  if (error) {
    return <FallbackViewer url={url} name={name} message={`Failed to render HEIC: ${error}`} />
  }
  if (loading || !pngUrl) {
    return (
      <div className="flex items-center justify-center">
        <Loader2 className="size-8 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="flex max-h-full flex-col items-center gap-3 overflow-auto p-6">
      <img
        src={pngUrl}
        alt={name}
        className="bg-checkerboard max-h-[calc(100vh-200px)] max-w-full rounded-lg border border-border shadow-lg"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

