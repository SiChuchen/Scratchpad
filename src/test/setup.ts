// src/test/setup.ts
//
// Vitest 全局 setup：注入 @testing-library/jest-dom 的 DOM 断言匹配器
// （toBeInTheDocument / toBeVisible 等），供组件测试使用。
import '@testing-library/jest-dom/vitest'
import { loadLocale } from '$lib/i18n'

// 组件测试默认预期 zh-CN 文案（许多用例查询中文 placeholder/aria-label）。
// jsdom 的 navigator.language 不稳定，这里强制加载 zh-CN。
loadLocale('zh-CN')
