/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useRef, useState } from 'react'
import { Loader2 } from 'lucide-react'
import { FallbackViewer } from './simple'

export function TiffViewer({ url, name }: { url: string; name: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    fetch(url)
      .then((res) => res.arrayBuffer())
      .then(async (buf) => {
        if (cancelled) return
        const UTIF = await import('utif')
        const ifds = UTIF.decode(buf)
        UTIF.decodeImage(buf, ifds[0])
        const rgba = UTIF.toRGBA8(ifds[0])
        const canvas = canvasRef.current
        if (!canvas || cancelled) return
        canvas.width = ifds[0].width
        canvas.height = ifds[0].height
        const ctx = canvas.getContext('2d')!
        const imageData = ctx.createImageData(canvas.width, canvas.height)
        imageData.data.set(rgba)
        ctx.putImageData(imageData, 0, 0)
        setLoading(false)
      })
      .catch((err) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err))
          setLoading(false)
        }
      })
    return () => { cancelled = true }
  }, [url])

  if (error) {
    return <FallbackViewer url={url} name={name} message={`Failed to render TIFF: ${error}`} />
  }
  return (
    <div className="flex flex-col items-center gap-3 p-6 max-h-full overflow-auto">
      {loading && <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />}
      <canvas
        ref={canvasRef}
        className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg bg-checkerboard"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

