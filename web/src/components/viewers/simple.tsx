/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { FileX, Download } from 'lucide-react'

export function ImageViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex max-h-full flex-col items-center gap-3 overflow-auto p-6">
      <img
        src={url}
        alt={name}
        className="bg-checkerboard max-h-[calc(100vh-200px)] max-w-full rounded-lg border border-border shadow-lg"
      />
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

export function PdfViewer({ url }: { url: string }) {
  return (
    <iframe
      src={url}
      title="PDF preview"
      className="size-full border-0"
      // Firefox renders PDFs via PDF.js, which runs as scripted content inside
      // the iframe; sandbox="" would blank it out. The preview endpoint is the
      // daemon's own token-authed route serving a PDF, not arbitrary HTML.
      sandbox="allow-scripts allow-same-origin"
    />
  )
}

export function VideoViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex max-h-full flex-col items-center gap-3 p-6">
      <video
        controls
        className="max-h-[calc(100vh-200px)] max-w-full rounded-lg border border-border shadow-lg"
      >
        <source src={url} />
        Your browser does not support video playback.
      </video>
      <span className="text-xs text-muted-foreground">{name}</span>
    </div>
  )
}

export function AudioViewer({ url, name }: { url: string; name: string }) {
  return (
    <div className="flex flex-col items-center gap-4 p-8">
      <div className="flex flex-col items-center gap-2 text-muted-foreground">
        <div className="flex size-20 items-center justify-center rounded-full bg-muted">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="size-10">
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

export function FallbackViewer({ url, name, message }: { url: string; name: string; message?: string }) {
  return (
    <div className="flex flex-col items-center gap-3 p-6 text-muted-foreground">
      <FileX className="size-12" />
      <p className="text-sm font-medium">{message || 'Preview not available for this file type'}</p>
      <p className="text-xs text-muted-foreground/70">{name}</p>
      <a
        href={url}
        download={name}
        className="mt-2 flex items-center gap-1.5 text-xs font-medium text-primary transition hover:text-primary/80"
      >
        <Download className="size-3.5" /> Download file
      </a>
    </div>
  )
}

