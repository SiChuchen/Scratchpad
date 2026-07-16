// src/test/setup.ts
//
// Vitest 全局 setup：注入 @testing-library/jest-dom 的 DOM 断言匹配器
// （toBeInTheDocument / toBeVisible 等），供组件测试使用。
import '@testing-library/jest-dom/vitest'
