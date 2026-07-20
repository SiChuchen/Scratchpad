// Preview entry — installs the Tauri mock BEFORE importing the real app.
import { installTauriMock } from './tauri-mock'

const params = new URLSearchParams(location.search)
const label = params.get('window') ?? 'main'
const theme = params.get('theme')
const lang = params.get('lang')

installTauriMock(label)

// Allow overriding the theme preset / language for screenshots: ?theme=light-matte&lang=en
if (theme) {
  localStorage.setItem('preview-theme', theme)
}
if (lang) {
  localStorage.setItem('preview-lang', lang)
}

await import('../main')
