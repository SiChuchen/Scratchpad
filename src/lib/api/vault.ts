// src/lib/api/vault.ts
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  EntryKind,
  LlmConfig,
  LlmTestResult,
  ProviderPreset,
  VaultEntry,
  VaultEntryDetail,
  VaultEntryInput,
  VaultSearchHit,
  TagUpdateEvent,
  LlmErrorEvent,
} from '$lib/types/vault'

export const vaultApi = {
  createEntry: (input: VaultEntryInput) =>
    invoke<VaultEntry>('ipc_vault_create_entry', { input }),
  updateEntry: (id: string, input: VaultEntryInput) =>
    invoke<void>('ipc_vault_update_entry', { id, input }),
  deleteEntry: (id: string) =>
    invoke<void>('ipc_vault_delete_entry', { id }),
  listEntries: (kind?: EntryKind) =>
    invoke<VaultEntry[]>('ipc_vault_list_entries', { kind: kind ?? null }),
  getEntry: (id: string) =>
    invoke<VaultEntryDetail>('ipc_vault_get_entry', { id }),
  updateTags: (id: string, tags: string[]) =>
    invoke<void>('ipc_vault_update_tags', { id, tags }),
  retag: (id: string) =>
    invoke<void>('ipc_vault_retag', { id }),
  search: (query: string, limit = 20) =>
    invoke<VaultSearchHit[]>('ipc_vault_search', { query, limit }),
  getLlmPresets: () =>
    invoke<ProviderPreset[]>('ipc_vault_get_llm_presets'),
  getLlmConfig: () =>
    invoke<LlmConfig | null>('ipc_vault_get_llm_config'),
  setLlmConfig: (config: LlmConfig) =>
    invoke<void>('ipc_vault_set_llm_config', { config }),
  testLlm: (config: LlmConfig) =>
    invoke<LlmTestResult>('ipc_vault_test_llm', { config }),
}

export function onTagsUpdated(cb: (e: TagUpdateEvent) => void): Promise<UnlistenFn> {
  return listen<TagUpdateEvent>('vault-tags-updated', e => cb(e.payload))
}

export function onLlmError(cb: (e: LlmErrorEvent) => void): Promise<UnlistenFn> {
  return listen<LlmErrorEvent>('vault-llm-error', e => cb(e.payload))
}
