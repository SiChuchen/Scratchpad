import { describe, expect, it, vi } from 'vitest'

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: vi.fn(),
}))

import { dockApi } from './dock'

describe('dockApi', () => {
  it('exposes clipboard copy methods', () => {
    expect(dockApi.copyFile).toEqual(expect.any(Function))
    expect(dockApi.copyImage).toEqual(expect.any(Function))
  })
})
