export type EdgeAnchor = 'left' | 'right' | 'top' | 'bottom'

export function anchorToNearestEdge(
  rect: { x: number; y: number; width: number; height: number },
  screen: { width: number; height: number },
): EdgeAnchor {
  const distances = {
    left: rect.x,
    right: screen.width - (rect.x + rect.width),
    top: rect.y,
    bottom: screen.height - (rect.y + rect.height),
  }
  return Object.entries(distances).sort((a, b) => a[1] - b[1])[0][0] as EdgeAnchor
}

export function hiddenTabRect(
  anchor: EdgeAnchor,
  tabSize: { width: number; height: number },
  screen: { width: number; height: number },
  winCenter?: { x: number; y: number },
) {
  // Position the tab at the nearest screen edge, aligned with the window center
  const cx = winCenter?.x ?? Math.round(screen.width * 0.4)
  const cy = winCenter?.y ?? Math.round(screen.height * 0.3)

  if (anchor === 'right') {
    return { x: screen.width - 4, y: clampY(cy - Math.round(tabSize.height / 2), tabSize.height, screen.height), width: tabSize.width, height: tabSize.height }
  }
  if (anchor === 'left') {
    return { x: 4 - tabSize.width, y: clampY(cy - Math.round(tabSize.height / 2), tabSize.height, screen.height), width: tabSize.width, height: tabSize.height }
  }
  if (anchor === 'top') {
    return { x: clampX(cx - Math.round(tabSize.width / 2), tabSize.width, screen.width), y: 4 - tabSize.height, width: tabSize.width, height: tabSize.height }
  }
  return { x: clampX(cx - Math.round(tabSize.width / 2), tabSize.width, screen.width), y: screen.height - 4, width: tabSize.width, height: tabSize.height }
}

function clampY(y: number, tabH: number, screenH: number): number {
  return Math.max(4, Math.min(y, screenH - tabH - 4))
}

function clampX(x: number, tabW: number, screenW: number): number {
  return Math.max(4, Math.min(x, screenW - tabW - 4))
}
