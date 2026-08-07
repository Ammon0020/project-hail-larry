/** Extracted from FileViewer.tsx — see that file for the dispatch table. */
import { useEffect, useRef, useState } from 'react'
import { Loader2, Box } from 'lucide-react'
import { FallbackViewer } from './simple'

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

export function ModelViewer({ url, name }: { url: string; name: string }) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let renderer: import('three').WebGLRenderer | null = null
    let frameId = 0
    let cancelled = false
    let resizeObserver: ResizeObserver | null = null

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

        // Keep the renderer/camera in sync with the container (window resize,
        // pane drag, orientation change) — Three.js does not observe this.
        const ro = new ResizeObserver((entries) => {
          for (const entry of entries) {
            const w = entry.contentRect.width
            const h = entry.contentRect.height
            if (w === 0 || h === 0) return
            renderer3.setSize(w, h)
            camera.aspect = w / h
            camera.updateProjectionMatrix()
          }
        })
        ro.observe(container)
        resizeObserver = ro
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
      resizeObserver?.disconnect()
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

