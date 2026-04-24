export type TokenType = 'color' | 'length' | 'shadow'

export interface TokenSchemaEntry {
  type: TokenType
  label: string
  min?: number
  max?: number
}

export const TOKEN_SCHEMA: Record<string, TokenSchemaEntry> = {
  // Color tokens (17)
  '--color-primary':        { type: 'color', label: '主强调色' },
  '--color-primary-light':  { type: 'color', label: '强调色浅' },
  '--color-primary-faint':  { type: 'color', label: '强调色极浅' },
  '--color-accent':         { type: 'color', label: '次强调色' },
  '--color-danger':         { type: 'color', label: '危险色' },
  '--color-success':        { type: 'color', label: '成功色' },
  '--color-info':           { type: 'color', label: '信息色' },
  '--color-file':           { type: 'color', label: '文件色' },
  '--surface-0':            { type: 'color', label: '容器底色' },
  '--surface-1':            { type: 'color', label: '卡片表面' },
  '--surface-2':            { type: 'color', label: '凹陷表面' },
  '--text-primary':         { type: 'color', label: '主文字' },
  '--text-muted':           { type: 'color', label: '弱文字' },
  '--text-faint':           { type: 'color', label: '极淡文字' },
  '--border-default':       { type: 'color', label: '默认边框' },
  '--border-subtle':        { type: 'color', label: '分割线' },
  '--border-emphasis':      { type: 'color', label: '强调边框' },
  // Effect token (1)
  '--shadow-default':       { type: 'shadow', label: '基础阴影' },
  // Layout tokens (6)
  '--space-sm':             { type: 'length', label: '间距-小', min: 0.05, max: 1.0 },
  '--space-md':             { type: 'length', label: '间距-中', min: 0.05, max: 1.0 },
  '--space-lg':             { type: 'length', label: '间距-大', min: 0.05, max: 2.0 },
  '--radius-sm':            { type: 'length', label: '圆角-小', min: 0, max: 2.0 },
  '--radius-md':            { type: 'length', label: '圆角-中', min: 0, max: 2.0 },
  '--radius-lg':            { type: 'length', label: '圆角-大', min: 0, max: 2.0 },
}

export function validateToken(key: string, value: string): { valid: boolean; error?: string } {
  const schema = TOKEN_SCHEMA[key]
  if (!schema) return { valid: false, error: '未知 token' }

  switch (schema.type) {
    case 'color': {
      if (/^(rgba?\(|#|oklch\()/.test(value.trim())) return { valid: true }
      return { valid: false, error: '格式应为 rgba(...), #hex, 或 oklch(...)' }
    }
    case 'length': {
      const num = parseFloat(value)
      if (isNaN(num)) return { valid: false, error: '请输入数值' }
      if (schema.min !== undefined && num < schema.min) return { valid: false, error: `最小 ${schema.min}` }
      if (schema.max !== undefined && num > schema.max) return { valid: false, error: `最大 ${schema.max}` }
      return { valid: true }
    }
    case 'shadow': {
      if (/\d/.test(value)) return { valid: true }
      return { valid: false, error: '格式应为 box-shadow 值' }
    }
  }
}
