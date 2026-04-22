import { describe, expect, it } from 'vitest'
import { anchorToNearestEdge, hiddenTabRect } from './window'

describe('window helpers', () => {
  it('anchors a minimized dock to the nearest edge', () => {
    expect(
      anchorToNearestEdge({ x: 1800, y: 200, width: 360, height: 640 }, { width: 1920, height: 1080 }),
    ).toBe('right')
  })

  it('anchors to left edge when close to it', () => {
    expect(
      anchorToNearestEdge({ x: 10, y: 200, width: 360, height: 640 }, { width: 1920, height: 1080 }),
    ).toBe('left')
  })

  it('returns a mostly hidden sliver rect for the right edge', () => {
    const rect = hiddenTabRect('right', { width: 48, height: 48 }, { width: 1920, height: 1080 })
    expect(rect.x).toBeGreaterThan(1860)
    expect(rect.width).toBe(48)
    expect(rect.height).toBe(48)
  })

  it('positions tab aligned with window center', () => {
    const rect = hiddenTabRect('right', { width: 48, height: 48 }, { width: 1920, height: 1080 }, { x: 960, y: 540 })
    expect(rect.x).toBe(1916)
    expect(rect.y).toBe(540 - 24)
  })
})
