/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useState } from 'react'
import DOMPurify from 'dompurify'
import { Loader2 } from 'lucide-react'
import { cn } from '@/lib/utils'
import { FallbackViewer } from './simple'

export function XlsxViewer({ url, name }: { url: string; name: string }) {
  const [sheets, setSheets] = useState<{ name: string; html: string }[]>([])
  const [activeSheet, setActiveSheet] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    fetch(url)
      .then((res) => res.arrayBuffer())
      .then(async (buf) => {
        if (cancelled) return
        const XLSX = await import('xlsx')
        const wb = XLSX.read(buf, { type: 'array' })
        const sheetData = wb.SheetNames.map((sheetName) => {
          const sheet = wb.Sheets[sheetName]
          // Sanitize before injecting: SheetJS escapes cell text by default,
          // but a library bug could allow script injection.
          const html = DOMPurify.sanitize(XLSX.utils.sheet_to_html(sheet, { editable: false }), {
            USE_PROFILES: { html: true },
          })
          return { name: sheetName, html }
        })
        if (!cancelled) {
          setSheets(sheetData)
          setLoading(false)
        }
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
    return <FallbackViewer url={url} name={name} message={`Failed to render XLSX: ${error}`} />
  }
  if (loading) {
    return (
      <div className="flex items-center justify-center">
        <Loader2 className="size-8 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="flex size-full flex-col bg-white text-black">
      {sheets.length > 1 && (
        <div className="flex gap-1 overflow-x-auto border-b border-gray-200 bg-gray-50 p-2">
          {sheets.map((sheet, index) => (
            <button
              key={sheet.name}
              onClick={() => setActiveSheet(index)}
              className={cn(
                'rounded px-3 py-1 text-xs whitespace-nowrap',
                index === activeSheet
                  ? 'border border-gray-300 bg-white font-medium shadow-sm'
                  : 'text-gray-600 hover:bg-gray-100',
              )}
            >
              {sheet.name}
            </button>
          ))}
        </div>
      )}
      <div
        className="flex-1 overflow-auto"
        dangerouslySetInnerHTML={{ __html: sheets[activeSheet]?.html || '' }}
      />
    </div>
  )
}

