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
import { useEffect, useState } from 'react'
import { Loader2, Code } from 'lucide-react'
import { api, previewFileUrl } from '@/lib/api'
import { cn } from '@/lib/utils'
import type { Tab } from '@/types'
import { ImageViewer, PdfViewer, VideoViewer, AudioViewer, FallbackViewer } from './viewers/simple'
import { TiffViewer } from './viewers/TiffViewer'
import { HeicViewer } from './viewers/HeicViewer'
import { DocxViewer } from './viewers/DocxViewer'
import { XlsxViewer } from './viewers/XlsxViewer'
import { EpubViewer } from './viewers/EpubViewer'
import { CsvViewer } from './viewers/CsvViewer'
import { HtmlViewer } from './viewers/HtmlViewer'
import { ModelViewer } from './viewers/ModelViewer'

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
      {url && kind === 'svg' && <ImageViewer url={url} name={tab.name} />}
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

