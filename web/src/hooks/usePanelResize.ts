import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type PointerEvent as ReactPointerEvent,
  type SetStateAction,
} from 'react'
import { safeStorage } from '@/lib/safeStorage'

interface PanelResizeConfig {
  initialWidth: number
  minWidth: number
  maxWidth: number
  storageKey: string
}

interface UsePanelResizeOptions {
  left: PanelResizeConfig
  right: PanelResizeConfig
}

interface UsePanelResizeResult {
  leftWidth: number
  rightWidth: number
  setLeftWidth: Dispatch<SetStateAction<number>>
  setRightWidth: Dispatch<SetStateAction<number>>
  startLeftDrag: (event: ReactPointerEvent) => void
  startRightDrag: (event: ReactPointerEvent) => void
  hideLeftPanel: () => void
  showLeftPanel: () => void
  toggleLeftPanel: () => void
  hideRightPanel: () => void
  showRightPanel: () => void
  toggleRightPanel: () => void
}

function readStoredWidth(config: PanelResizeConfig): number {
  const stored = safeStorage.getNumber(config.storageKey, config.initialWidth)
  return stored >= config.minWidth && stored <= config.maxWidth ? stored : config.initialWidth
}

function hiddenStorageKey(config: PanelResizeConfig): string {
  return `${config.storageKey}:hidden`
}

function readStoredHidden(config: PanelResizeConfig): boolean {
  return safeStorage.get(hiddenStorageKey(config)) === '1'
}

/**
 * Manages persisted left/right panel widths, sidebar visibility restoration,
 * and the pointer-capture listeners used while resizing either panel.
 */
export function usePanelResize({
  left,
  right,
}: UsePanelResizeOptions): UsePanelResizeResult {
  const leftStorageKey = left.storageKey
  const rightStorageKey = right.storageKey
  const leftHiddenKey = hiddenStorageKey(left)
  const rightHiddenKey = hiddenStorageKey(right)
  const [leftWidth, setLeftWidth] = useState(() =>
    readStoredHidden(left) ? 0 : readStoredWidth(left),
  )
  const [rightWidth, setRightWidth] = useState(() =>
    readStoredHidden(right) ? 0 : readStoredWidth(right),
  )
  const hiddenLeftWidthRef = useRef(readStoredWidth(left))
  const hiddenRightWidthRef = useRef(readStoredWidth(right))
  const dragCleanupRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    if (leftWidth > 0) {
      safeStorage.set(leftStorageKey, String(leftWidth))
      safeStorage.set(leftHiddenKey, '0')
    } else {
      safeStorage.set(leftHiddenKey, '1')
    }
  }, [leftStorageKey, leftHiddenKey, leftWidth])

  useEffect(() => {
    if (rightWidth > 0) {
      safeStorage.set(rightStorageKey, String(rightWidth))
      safeStorage.set(rightHiddenKey, '0')
    } else {
      safeStorage.set(rightHiddenKey, '1')
    }
  }, [rightStorageKey, rightHiddenKey, rightWidth])

  useEffect(() => {
    return () => dragCleanupRef.current?.()
  }, [])

  const startDrag = useCallback(
    (side: 'left' | 'right', event: ReactPointerEvent) => {
      event.preventDefault()
      dragCleanupRef.current?.()

      const target = event.currentTarget
      target.setPointerCapture(event.pointerId)
      const startX = event.clientX
      const startWidth = side === 'left' ? leftWidth : rightWidth

      const onMove = (moveEvent: Event) => {
        const delta = (moveEvent as PointerEvent).clientX - startX
        if (side === 'left') {
          const next = Math.min(
            left.maxWidth,
            Math.max(left.minWidth, startWidth + delta),
          )
          setLeftWidth(next)
        } else {
          const next = Math.min(
            right.maxWidth,
            Math.max(right.minWidth, startWidth - delta),
          )
          setRightWidth(next)
        }
      }

      const cleanup = () => {
        target.removeEventListener('pointermove', onMove)
        target.removeEventListener('pointerup', cleanup)
        target.removeEventListener('pointercancel', cleanup)
        try {
          target.releasePointerCapture(event.pointerId)
        } catch {
          // Pointer may already be released.
        }
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        dragCleanupRef.current = null
      }

      dragCleanupRef.current = cleanup
      target.addEventListener('pointermove', onMove)
      target.addEventListener('pointerup', cleanup)
      target.addEventListener('pointercancel', cleanup)
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'
    },
    [left.maxWidth, left.minWidth, leftWidth, right.maxWidth, right.minWidth, rightWidth],
  )

  const hideLeftPanel = useCallback(() => {
    setLeftWidth((currentWidth) => {
      if (currentWidth > 0) {
        hiddenLeftWidthRef.current = currentWidth
      }
      return 0
    })
  }, [])

  const showLeftPanel = useCallback(() => {
    setLeftWidth(hiddenLeftWidthRef.current ?? left.initialWidth)
  }, [left.initialWidth])

  const toggleLeftPanel = useCallback(() => {
    setLeftWidth((currentWidth) => {
      if (currentWidth > 0) {
        hiddenLeftWidthRef.current = currentWidth
        return 0
      }
      return hiddenLeftWidthRef.current ?? left.initialWidth
    })
  }, [left.initialWidth])

  const hideRightPanel = useCallback(() => {
    setRightWidth((currentWidth) => {
      if (currentWidth > 0) {
        hiddenRightWidthRef.current = currentWidth
      }
      return 0
    })
  }, [])

  const showRightPanel = useCallback(() => {
    setRightWidth(hiddenRightWidthRef.current ?? right.initialWidth)
  }, [right.initialWidth])

  const toggleRightPanel = useCallback(() => {
    setRightWidth((currentWidth) => {
      if (currentWidth > 0) {
        hiddenRightWidthRef.current = currentWidth
        return 0
      }
      return hiddenRightWidthRef.current ?? right.initialWidth
    })
  }, [right.initialWidth])

  return {
    leftWidth,
    rightWidth,
    setLeftWidth,
    setRightWidth,
    startLeftDrag: (event) => startDrag('left', event),
    startRightDrag: (event) => startDrag('right', event),
    hideLeftPanel,
    showLeftPanel,
    toggleLeftPanel,
    hideRightPanel,
    showRightPanel,
    toggleRightPanel,
  }
}
