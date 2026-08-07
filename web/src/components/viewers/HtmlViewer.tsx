/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useState } from 'react'
import { TrustPrompt } from '@/components/preview/TrustPrompt'

export function HtmlViewer({ url, workspaceId, trusted }: { url: string; workspaceId: string; trusted?: boolean | null }) {
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
  if (effectiveTrust === false) {
    return (
      <div className="flex flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">
        <p>Preview blocked — mark as trusted to view.</p>
      </div>
    )
  }
  // Trusted: allow scripts but keep the opaque origin (no allow-same-origin)
  // so workspace HTML/JS cannot reach the IDE's storage or authed APIs.
  return (
    <iframe
      src={url}
      title="HTML preview"
      className="w-full h-full border-0 bg-white"
      sandbox="allow-scripts"
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
