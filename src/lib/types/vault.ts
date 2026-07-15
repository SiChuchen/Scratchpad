// src/lib/types/vault.ts
export type EntryKind = 'credential' | 'bookmark' | 'note'

export interface VaultField {
  id: string
  entryId: string
  key: string
  value: string
  isSensitive: boolean
  sortOrder: number
}

export interface VaultEntry {
  id: string
  kind: EntryKind
  title: string
  notes: string | null
  createdAt: string
  updatedAt: string
}

export interface VaultEntryDetail {
  entry: VaultEntry
  fields: VaultField[]
  tags: string[]
}

export interface FieldInput {
  key: string
  value: string
  isSensitive: boolean
}

export interface VaultEntryInput {
  kind: EntryKind
  title: string
  fields: FieldInput[]
  notes: string | null
}

export interface VaultSearchHit {
  entry: VaultEntry
  score: number
  source: 'fts5' | 'llm'
}

export interface ProviderPreset {
  id: string
  label: string
  baseUrl: string
  models: string[]
  defaultModel: string
}

export interface LlmConfig {
  providerId: string
  baseUrl: string
  apiKey: string
  model: string
}

export interface LlmTestResult {
  ok: boolean
  message: string
  modelEcho: string | null
}

export interface TagUpdateEvent {
  id: string
  tags: string[]
}

export interface LlmErrorEvent {
  kind: string
  message: string
}
