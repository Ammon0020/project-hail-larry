import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MouseEvent as ReactMouseEvent,
  type SetStateAction,
} from 'react'

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
  startLeftDrag: (event: ReactMouseEvent) => void
  startRightDrag: (event: ReactMouseEvent) => void
  hideLeftPanel: () => void
  showLeftPanel: () => void
  toggleLeftPanel: () => void
  hideRightPanel: () => void
  showRightPanel: () => void
  toggleRightPanel: () => void
}

function readStoredWidth(config: PanelResizeConfig): number {
  const stored = Number(localStorage.getItem(config.storageKey))
  return Number.isFinite(stored) && stored >= config.minWidth && stored <= config.maxWidth
    ? stored
    : config.initialWidth
}

/**
 * Manages persisted left/right panel widths, sidebar visibility restoration,
 * and the window-level pointer listeners used while resizing either panel.
 */
export function usePanelResize({
  left,
  right,
}: UsePanelResizeOptions): UsePanelResizeResult {
  const [leftWidth, setLeftWidth] = useState(() => readStoredWidth(left))
  const [rightWidth, setRightWidth] = useState(() => readStoredWidth(right))
  const hiddenLeftWidthRef = useRef(left.initialWidth)
  const hiddenRightWidthRef = useRef(right.initialWidth)
  const dragCleanupRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    if (leftWidth > 0) {
      localStorage.setItem(left.storageKey, String(leftWidth))
    }
  }, [left.storageKey, leftWidth])

  useEffect(() => {
    if (rightWidth > 0) {
      localStorage.setItem(right.storageKey, String(rightWidth))
    }
  }, [right.storageKey, rightWidth])

  useEffect(() => {
    return () => dragCleanupRef.current?.()
  }, [])

  const startDrag = useCallback(
    (side: 'left' | 'right', event: ReactMouseEvent) => {
      event.preventDefault()
      dragCleanupRef.current?.()

      const startX = event.clientX
      const startWidth = side === 'left' ? leftWidth : rightWidth

      const onMove = (moveEvent: MouseEvent) => {
        const delta = moveEvent.clientX - startX
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
        window.removeEventListener('mousemove', onMove)
        window.removeEventListener('mouseup', cleanup)
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        dragCleanupRef.current = null
      }

      dragCleanupRef.current = cleanup
      window.addEventListener('mousemove', onMove)
      window.addEventListener('mouseup', cleanup)
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
