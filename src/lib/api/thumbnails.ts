import { contentApi } from '$lib/api/content'
import { dockApi } from '$lib/api/dock'

// ContentSummary 是后端的安全投影，刻意不包含资源路径，
// 因此列表缩略图只能按需取详情拿到 assetPath，再转换为 webview 可用 URL。
// 同一项在列表滚动/重排中会反复挂载，用 Promise 缓存避免重复 IPC。
const cache = new Map<string, Promise<string | null>>()

export function imageThumbnailUrl(id: string): Promise<string | null> {
  let pending = cache.get(id)
  if (!pending) {
    pending = (async () => {
      try {
        const detail = await contentApi.detail(id)
        if (detail.kind === 'image' && detail.available) {
          return dockApi.previewUrl(detail.assetPath)
        }
        return null
      } catch {
        return null
      }
    })()
    cache.set(id, pending)
  }
  return pending
}
