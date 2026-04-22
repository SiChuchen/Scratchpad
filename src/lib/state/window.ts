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
) {
  if (anchor === 'right') {
    return { x: screen.width - 6, y: Math.round(screen.height * 0.3), width: tabSize.width, height: tabSize.height }
  }
  if (anchor === 'left') {
    return { x: 6 - tabSize.width, y: Math.round(screen.height * 0.3), width: tabSize.width, height: tabSize.height }
  }
  if (anchor === 'top') {
    return { x: Math.round(screen.width * 0.4), y: 6 - tabSize.height, width: tabSize.width, height: tabSize.height }
  }
  return { x: Math.round(screen.width * 0.4), y: screen.height - 6, width: tabSize.width, height: tabSize.height }
}
