import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

function cssBlock(source: string, selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const match = source.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))
  return match?.[1] ?? ''
}

describe('window shell styles', () => {
  it('keeps the global app mount transparent for the minimized tab first frame', () => {
    const globalCss = readFileSync('src/app.css', 'utf8')
    const appSvelte = readFileSync('src/App.svelte', 'utf8')
    const minimizedSvelte = readFileSync('src/MinimizedApp.svelte', 'utf8')
    const mainTs = readFileSync('src/main.ts', 'utf8')

    const globalAppBlock = cssBlock(globalCss, '#app')
    expect(globalAppBlock).not.toMatch(/background\s*:/)
    expect(globalAppBlock).not.toMatch(/backdrop-filter\s*:/)
    expect(globalAppBlock).not.toMatch(/border\s*:/)
    expect(globalAppBlock).not.toMatch(/box-shadow\s*:/)

    const shellBlock = cssBlock(appSvelte, '.app-shell')
    expect(shellBlock).toMatch(/background\s*:/)
    expect(shellBlock).toMatch(/backdrop-filter\s*:/)
    expect(shellBlock).toMatch(/border\s*:/)
    expect(shellBlock).toMatch(/box-shadow\s*:/)

    expect(minimizedSvelte).not.toMatch(/:global\(html\)/)
    expect(minimizedSvelte).not.toMatch(/:global\(body\)/)
    expect(minimizedSvelte).not.toMatch(/:global\(#app\)/)
    expect(mainTs).toMatch(/classList\.add/)
    expect(globalCss).toMatch(/html\.minimized-tab-window/)
  })
})
