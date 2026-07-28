/**
 * FileViewer — renders binary/non-text files that CodeMirror cannot edit.
 *
 * Dispatches to specialized viewers based on file extension:
 *  - Images (png, jpg, gif, webp, bmp, ico, avif) → <img>
 *  - TIFF (tiff, tif) → UTIF canvas rendering
 *  - HEIC (heic, heif) → heic2any PNG conversion
 *  - SVG → <img>
 *  - PDF → <iframe> (browser-native rendering)
 *  - Video (mp4, webm, ogv, mov, mkv) → <video controls>
 *  - Audio (mp3, wav, ogg, flac, m4a, aac, opus) → <audio controls>
 *  - DOCX → mammoth.js (converts to HTML in-browser)
 *  - XLSX → SheetJS table preview
 *  - EPUB → epub.js reader
 *  - CSV → table preview
 *  - HTML (html, htm) → sandboxed <iframe>
 *  - 3D models (stl, 3mf, obj, gltf, glb, ply, dae, wrl, vrml) → Three.js orbit viewer
 *  - Fallback → "preview not available" + download link
 *
 * All binary content is served from GET /preview/{id}/{path}, which
 * streams raw bytes with a proper Content-Type (unlike the JSON-wrapped
 * /file endpoint). Browser media tags cannot set Authorization headers,
 * so a one-time preview-session ticket bootstraps an HttpOnly cookie
 * instead of putting the device secret in the URL query string.
 */
import { useEffect, useRef, useState } from 'react'
import DOMPurify from 'dompurify'
import { FileX, Download, Loader2, Box, Code } from 'lucide-react'
import { api, previewFileUrl } from '@/lib/api'
import { cn } from '@/lib/utils'
import { TrustPrompt } from '@/components/preview/TrustPrompt'
import type { Tab } from '@/types'

// ---------------------------------------------------------------------------
// Extension classification
// ---------------------------------------------------------------------------

const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'avif', 'tiff', 'tif', 'heic', 'heif']
const SVG_EXTS = ['svg']
const PDF_EXTS = ['pdf']
const VIDEO_EXTS = ['mp4', 'webm', 'ogv', 'mov', 'mkv']
const AUDIO_EXTS = ['mp3', 'wav', 'oga', 'ogg', 'flac', 'm4a', 'aac', 'opus']
const DOCX_EXTS = ['docx']
const XLSX_EXTS = ['xlsx']
const EPUB_EXTS = ['epub']
const CSV_EXTS = ['csv']
const HTML_EXTS = ['html', 'htm']
const MODEL_EXTS = ['stl', '3mf', 'obj', 'gltf', 'glb', 'ply', 'dae', 'wrl', 'vrml']

const PREVIEW_AUTH_ROUTE_UNAVAILABLE =
  'Preview authorization needs a server restart to finish updating.'

type ViewerKind =
  | 'image'
  | 'svg'
  | 'pdf'
  | 'video'
  | 'audio'
  | 'docx'
  | 'xlsx'
  | 'epub'
  | 'csv'
  | 'html'
  | 'model'
  | 'fallback'

/** Resolves the viewer kind from a file name's extension. */
function viewerKind(name: string): ViewerKind {
  const ext = name.split('.').pop()?.toLowerCase() || ''
  if (IMAGE_EXTS.includes(ext)) return 'image'
  if (SVG_EXTS.includes(ext)) return 'svg'
  if (PDF_EXTS.includes(ext)) return 'pdf'
  if (VIDEO_EXTS.includes(ext)) return 'video'
  if (AUDIO_EXTS.includes(ext)) return 'audio'
  if (DOCX_EXTS.includes(ext)) return 'docx'
  if (XLSX_EXTS.includes(ext)) return 'xlsx'
  if (EPUB_EXTS.includes(ext)) return 'epub'
  if (CSV_EXTS.includes(ext)) return 'csv'
  if (HTML_EXTS.includes(ext)) return 'html'
  if (MODEL_EXTS.includes(ext)) return 'model'
  return 'fallback'
}

// ---------------------------------------------------------------------------
// Main dispatcher
// ---------------------------------------------------------------------------

export function FileViewer({ tab, active, onToggleViewMode, trusted }: { tab: Tab; active: boolean; onToggleViewMode?: (id: string) => void; trusted?: boolean | null }) {
  const kind = viewerKind(tab.name)
  const ext = tab.name.split('.').pop()?.toLowerCase() || ''
  const workspaceId = tab.workspaceId ?? ''
  const [previewToken, setPreviewToken] = useState<string>()
  const [sessionError, setSessionError] = useState<string>()
  const [sessionVersion, setSessionVersion] = useState(0)
  const url = previewToken ? previewFileUrl(workspaceId, tab.path, previewToken) : ''

  useEffect(() => {
    if (!workspaceId) return
    let cancelled = false
    void api.createPreviewSession(workspaceId)
      .then(({ token }) => { if (!cancelled) setPreviewToken(token) })
      .catch((error: unknown) => {
        if (!cancelled) {
          setSessionError(
            error instanceof Error && error.message === 'Method Not Allowed'
              ? PREVIEW_AUTH_ROUTE_UNAVAILABLE
              : error instanceof Error
                ? error.message
                : 'Unable to authorize preview',
          )
        }
      })
    return () => { cancelled = true }
  }, [workspaceId, sessionVersion])

  return (
    <div className={cn('absolute inset-0 items-center justify-center bg-editor', active ? 'flex' : 'hidden')}>
      {sessionError ? (
        <div className="flex flex-col items-center gap-3 px-6 text-center text-sm text-destructive">
          <p>Preview authorization failed: {sessionError}</p>
          <button
            type="button"
            onClick={() => {
              setSessionError(undefined)
              setPreviewToken(undefined)
              setSessionVersion((v) => v + 1)
            }}
            className="rounded px-2 py-1 font-medium text-foreground hover:text-primary"
          >
            Retry preview
          </button>
        </div>
      ) : !url ? (
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      ) : null}
      {url && kind === 'image' && (
        ext === 'tiff' || ext === 'tif'
          ? <TiffViewer url={url} name={tab.name} />
          : ext === 'heic' || ext === 'heif'
            ? <HeicViewer url={url} name={tab.name} />
            : <ImageViewer url={url} name={tab.name} />
      )}
      {url && kind === 'svg' && <SvgViewer url={url} name={tab.name} />}
      {url && kind === 'pdf' && <PdfViewer url={url} />}
      {url && kind === 'video' && <VideoViewer url={url} name={tab.name} />}
      {url && kind === 'audio' && <AudioViewer url={url} name={tab.name} />}
      {url && kind === 'docx' && <DocxViewer url={url} name={tab.name} />}
      {url && kind === 'xlsx' && <XlsxViewer url={url} name={tab.name} />}
      {url && kind === 'epub' && <EpubViewer url={url} />}
      {url && kind === 'csv' && <CsvViewer url={url} name={tab.name} />}
      {url && kind === 'html' && <HtmlViewer url={url} workspaceId={workspaceId} trusted={trusted} />}
      {url && kind === 'model' && <ModelViewer url={url} name={tab.name} />}
      {url && kind === 'fallback' && <FallbackViewer url={url} name={tab.name} />}
      {/* View Raw button — shown for text-preview files (SVG, CSV, HTML, OBJ)
          that are in preview mode, so the user can switch back to CodeMirror. */}
      {tab.previewable && !tab.isBinary && onToggleViewMode && (
        <button
          type="button"
          onClick={() => onToggleViewMode(tab.id)}
          className="absolute top-2 right-2 z-10 flex items-center gap-1.5 px-2.5 py-1 text-xs font-medium rounded bg-secondary/90 hover:bg-secondary text-secondary-foreground backdrop-blur-sm transition shadow-sm"
        >
          <Code className="w-3.5 h-3.5" /> View Raw
        </button>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------
// Individual viewers
// ---------------------------------------------------------------------------

function ImageViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex flex-col items-center gap-3 p-6 max-h-full overflow-auto">
      <img
        src={url}
        alt={name}
        className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg bg-checkerboard"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

function SvgViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex flex-col items-center gap-3 p-6 max-h-full overflow-auto">
      <img
        src={url}
        alt={name}
        className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg bg-checkerboard"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

function TiffViewer({ url, name }: { url: string; name: string }) {
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

function HeicViewer({ url, name }: { url: string; name: string }) {
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
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="flex flex-col items-center gap-3 p-6 max-h-full overflow-auto">
      <img
        src={pngUrl}
        alt={name}
        className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg bg-checkerboard"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

function PdfViewer({ url }: { url: string }) {
  return (
    <iframe
      src={url}
      title="PDF preview"
      className="w-full h-full border-0"
      sandbox=""
    />
  )
}

function VideoViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex flex-col items-center gap-3 p-6 max-h-full">
      <video
        controls
        className="max-w-full max-h-[calc(100vh-200px)] rounded-lg border border-border shadow-lg"
      >
        <source src={url} />
        Your browser does not support video playback.
      </video>
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

function AudioViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex flex-col items-center gap-4 p-8">
      <div className="flex flex-col items-center gap-2 text-muted-foreground">
        <div className="w-20 h-20 rounded-full bg-muted flex items-center justify-center">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="w-10 h-10">
            <path d="M9 18V5l12-2v13" />
            <circle cx="6" cy="18" r="3" />
            <circle cx="18" cy="16" r="3" />
          </svg>
        </div>
        <span className="text-sm font-medium">{name}</span>
      </div>
      <audio controls className="w-full max-w-md">
        <source src={url} />
        Your browser does not support audio playback.
      </audio>
    </div>
  )
}

// ---------------------------------------------------------------------------
// DOCX viewer — mammoth.js converts .docx to HTML in-browser
// ---------------------------------------------------------------------------

function DocxViewer({ url, name }: { url: string; name: string }) {
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

function XlsxViewer({ url, name }: { url: string; name: string }) {
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
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="w-full h-full flex flex-col bg-white text-black">
      {sheets.length > 1 && (
        <div className="flex gap-1 p-2 border-b border-gray-200 bg-gray-50 overflow-x-auto">
          {sheets.map((sheet, index) => (
            <button
              key={sheet.name}
              onClick={() => setActiveSheet(index)}
              className={cn(
                'px-3 py-1 text-xs rounded whitespace-nowrap',
                index === activeSheet
                  ? 'bg-white border border-gray-300 font-medium shadow-sm'
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

function EpubViewer({ url }: { url: string }) {
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

function CsvViewer({ url, name }: { url: string; name: string }) {
  const [rows, setRows] = useState<string[][]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    fetch(url)
      .then((res) => res.text())
      .then((text) => {
        if (cancelled) return
        setRows(parseCsv(text))
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
    return <FallbackViewer url={url} name={name} message={`Failed to render CSV: ${error}`} />
  }
  if (loading) {
    return (
      <div className="flex items-center justify-center">
        <Loader2 className="w-8 h-8 animate-spin text-muted-foreground" />
      </div>
    )
  }
  return (
    <div className="w-full h-full overflow-auto bg-white text-black">
      <table className="border-collapse text-xs">
        <tbody>
          {rows.map((row, rowIndex) => (
            <tr key={rowIndex} className={rowIndex === 0 ? 'font-bold bg-gray-100 sticky top-0' : ''}>
              {row.map((cell, cellIndex) => (
                <td key={cellIndex} className="border border-gray-300 px-2 py-1 whitespace-nowrap">
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function parseCsv(text: string): string[][] {
  const rows: string[][] = []
  let row: string[] = []
  let field = ''
  let inQuotes = false
  for (let index = 0; index < text.length; index++) {
    const character = text[index]
    if (inQuotes) {
      if (character === '"') {
        if (text[index + 1] === '"') {
          field += '"'
          index++
        } else {
          inQuotes = false
        }
      } else {
        field += character
      }
    } else if (character === '"') {
      inQuotes = true
    } else if (character === ',') {
      row.push(field)
      field = ''
    } else if (character === '\n') {
      row.push(field)
      rows.push(row)
      row = []
      field = ''
    } else if (character !== '\r') {
      field += character
    }
  }
  if (field || row.length) {
    row.push(field)
    rows.push(row)
  }
  return rows
}

function HtmlViewer({ url, workspaceId, trusted }: { url: string; workspaceId: string; trusted?: boolean | null }) {
  // Local override once the user answers the trust prompt — avoids waiting for
  // a parent re-render before showing the iframe.
  const [resolvedTrust, setResolvedTrust] = useState<boolean | null | undefined>(undefined)
  const effectiveTrust = resolvedTrust ?? trusted
  const trustUnknown = effectiveTrust == null

  if (trustUnknown) {
    return (
      <TrustPrompt
        workspaceId={workspaceId}
        onResolve={setResolvedTrust}
        className="w-full h-full text-destructive"
      />
    )
  }
  return (
    <iframe
      src={url}
      title="HTML preview"
      className="w-full h-full border-0 bg-white"
      sandbox=""
    />
  )
}

// ---------------------------------------------------------------------------
// STL viewer — Three.js + STLLoader with orbit controls
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 3D model viewer — Three.js with format-specific loaders
//
// STL and PLY loaders return BufferGeometry (wrapped in a Mesh with a default
// material). 3MF, OBJ, GLTF, Collada, and VRML loaders return Object3D/Group (already
// materialized, added directly to the scene). The scene setup — renderer,
// camera, lighting, auto-framing, orbit controls, animation loop, cleanup —
// is shared across all formats.
// ---------------------------------------------------------------------------

/** Loads a 3D file from url and returns an Object3D ready to add to the scene. */
async function loadModel(
  THREE: typeof import('three'),
  ext: string,
  url: string,
): Promise<import('three').Object3D> {
  switch (ext) {
    case 'stl': {
      const { STLLoader } = await import('three/examples/jsm/loaders/STLLoader.js')
      const geometry = await new Promise<import('three').BufferGeometry>((resolve, reject) =>
        new STLLoader().load(url, resolve, undefined, reject),
      )
      geometry.computeVertexNormals()
      return new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        color: 0x8b9bb4, metalness: 0.1, roughness: 0.5,
      }))
    }
    case 'ply': {
      const { PLYLoader } = await import('three/examples/jsm/loaders/PLYLoader.js')
      const geometry = await new Promise<import('three').BufferGeometry>((resolve, reject) =>
        new PLYLoader().load(url, resolve, undefined, reject),
      )
      geometry.computeVertexNormals()
      return new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({
        color: 0x8b9bb4, metalness: 0.1, roughness: 0.5,
      }))
    }
    case '3mf': {
      const { ThreeMFLoader } = await import('three/examples/jsm/loaders/3MFLoader.js')
      return new Promise<import('three').Object3D>((resolve, reject) =>
        new ThreeMFLoader().load(url, resolve, undefined, reject),
      )
    }
    case 'obj': {
      const { OBJLoader } = await import('three/examples/jsm/loaders/OBJLoader.js')
      return new Promise<import('three').Object3D>((resolve, reject) =>
        new OBJLoader().load(url, resolve, undefined, reject),
      )
    }
    case 'gltf':
    case 'glb': {
      const { GLTFLoader } = await import('three/examples/jsm/loaders/GLTFLoader.js')
      const gltf = await new Promise<import('three/examples/jsm/loaders/GLTFLoader.js').GLTF>(
        (resolve, reject) => new GLTFLoader().load(url, resolve, undefined, reject),
      )
      return gltf.scene
    }
    case 'dae': {
      const { ColladaLoader } = await import('three/examples/jsm/loaders/ColladaLoader.js')
      const collada = await new Promise<import('three/examples/jsm/loaders/ColladaLoader.js').Collada>(
        (resolve, reject) => new ColladaLoader().load(
          url,
          (result) => result ? resolve(result) : reject(new Error('Failed to load Collada model')),
          undefined,
          reject,
        ),
      )
      return collada.scene
    }
    case 'wrl':
    case 'vrml': {
      const { VRMLLoader } = await import('three/examples/jsm/loaders/VRMLLoader.js')
      return new Promise<import('three').Object3D>((resolve, reject) =>
        new VRMLLoader().load(url, resolve, undefined, reject),
      )
    }
    default:
      throw new Error(`Unsupported 3D format: .${ext}`)
  }
}

function ModelViewer({ url, name }: { url: string; name: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let renderer: import('three').WebGLRenderer | null = null
    let frameId = 0
    let cancelled = false

    async function init() {
      try {
        const THREE = await import('three')
        const { OrbitControls } = await import('three/examples/jsm/controls/OrbitControls.js')
        if (cancelled || !containerRef.current) return

        const ext = name.split('.').pop()?.toLowerCase() || ''
        const object = await loadModel(THREE, ext, url)
        if (cancelled || !containerRef.current) return

        const container = containerRef.current
        const width = container.clientWidth
        const height = container.clientHeight

        // Scene setup with neutral lighting so surface details are visible.
        const scene = new THREE.Scene()
        scene.background = new THREE.Color(0x1e1e2e)

        const camera = new THREE.PerspectiveCamera(50, width / height, 0.1, 10000)
        const renderer3 = new THREE.WebGLRenderer({ antialias: true })
        renderer3.setSize(width, height)
        renderer3.setPixelRatio(window.devicePixelRatio)
        container.appendChild(renderer3.domElement)
        renderer = renderer3

        scene.add(object)

        // Auto-frame: compute bounding box, center the model, and position the
        // camera at a distance that fits the bounding sphere.
        const box = new THREE.Box3().setFromObject(object)
        const size = box.getSize(new THREE.Vector3())
        const center = box.getCenter(new THREE.Vector3())
        const maxDim = Math.max(size.x, size.y, size.z)
        const fov = camera.fov * (Math.PI / 180)
        const distance = (maxDim / 2) / Math.tan(fov / 2) * 1.8
        camera.position.set(center.x + distance * 0.5, center.y + distance * 0.4, center.z + distance)
        camera.lookAt(center)

        // Lights — key, fill, and back for depth, plus ambient for base level.
        const keyLight = new THREE.DirectionalLight(0xffffff, 1.2)
        keyLight.position.set(1, 1, 2)
        scene.add(keyLight)
        const fillLight = new THREE.DirectionalLight(0xffffff, 0.4)
        fillLight.position.set(-1, -0.5, 1)
        scene.add(fillLight)
        const backLight = new THREE.DirectionalLight(0xffffff, 0.3)
        backLight.position.set(0, 1, -2)
        scene.add(backLight)
        scene.add(new THREE.AmbientLight(0xffffff, 0.3))

        // Orbit controls let the user rotate, zoom, and pan the model.
        const controls = new OrbitControls(camera, renderer3.domElement)
        controls.target.copy(center)
        controls.enableDamping = true
        controls.dampingFactor = 0.08
        controls.update()

        const animate = () => {
          if (cancelled) return
          frameId = requestAnimationFrame(animate)
          controls.update()
          renderer3.render(scene, camera)
        }
        animate()
        setLoading(false)
      } catch (err) {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : 'Failed to load 3D model')
          setLoading(false)
        }
      }
    }
    init()

    // Cleanup: cancel animation frame, dispose renderer, remove canvas.
    return () => {
      cancelled = true
      cancelAnimationFrame(frameId)
      if (renderer) {
        renderer.dispose()
        if (renderer.domElement.parentNode) {
          renderer.domElement.parentNode.removeChild(renderer.domElement)
        }
      }
    }
  }, [url, name])

  if (error) {
    return <FallbackViewer url={url} name={name} message={`Failed to render 3D model: ${error}`} />
  }
  return (
    <div className="w-full h-full relative">
      {loading && (
        <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 text-muted-foreground bg-editor">
          <Loader2 className="w-8 h-8 animate-spin" />
          <p className="text-sm">Loading 3D model…</p>
        </div>
      )}
      <div ref={containerRef} className="w-full h-full" />
      <div className="absolute bottom-2 left-3 text-xs text-muted-foreground/60 flex items-center gap-1.5 pointer-events-none">
        <Box className="w-3 h-3" /> {name} — drag to rotate, scroll to zoom
      </div>
    </div>
  )
}

// ---------------------------------------------------------------------------
// Fallback — unsupported binary file with a download link
// ---------------------------------------------------------------------------

function FallbackViewer({ url, name, message }: { url: string; name: string; message?: string }) {
  return (
    <div className="flex flex-col items-center gap-3 text-muted-foreground p-6">
      <FileX className="w-12 h-12" />
      <p className="text-sm font-medium">{message || 'Preview not available for this file type'}</p>
      <p className="text-xs text-muted-foreground/70">{name}</p>
      <a
        href={url}
        download={name}
        className="mt-2 flex items-center gap-1.5 text-xs font-medium text-primary hover:text-primary/80 transition"
      >
        <Download className="w-3.5 h-3.5" /> Download file
      </a>
    </div>
  )
}
