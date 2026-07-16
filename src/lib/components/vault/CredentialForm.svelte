<script lang="ts">
  import { vaultApi } from '$lib/api/vault'
  import type { VaultEntry } from '$lib/types/vault'

  let { onSaved }: { onSaved?: (e: VaultEntry) => void } = $props()

  let title = $state('')
  let username = $state('')
  let password = $state('')
  let host = $state('')
  let notes = $state('')

  async function submit() {
    if (!title.trim()) return
    const entry = await vaultApi.createEntry({
      kind: 'credential',
      title: title.trim(),
      fields: [
        { key: 'user', value: username, isSensitive: false },
        { key: 'password', value: password, isSensitive: true },
        { key: 'host', value: host, isSensitive: false },
      ].filter(f => f.value.length > 0),
      notes: notes.trim() || null,
    })
    onSaved?.(entry)
    title = username = password = host = notes = ''
  }
</script>

<form onsubmit={(e) => { e.preventDefault(); submit() }} class="vault-form">
  <label class="field">
    <span class="label">标题</span>
    <input class="input" bind:value={title} placeholder="例如：生产数据库" required />
  </label>
  <div class="row">
    <label class="field">
      <span class="label">用户名</span>
      <input class="input" bind:value={username} placeholder="user" />
    </label>
    <label class="field">
      <span class="label">主机 / URL</span>
      <input class="input" bind:value={host} placeholder="10.0.0.1 或 https://..." />
    </label>
  </div>
  <label class="field">
    <span class="label">密码</span>
    <input class="input" type="password" bind:value={password} placeholder="••••" />
  </label>
  <label class="field">
    <span class="label">备注</span>
    <textarea class="input textarea" bind:value={notes} rows={2}></textarea>
  </label>
  <div class="actions">
    <button class="btn-submit" type="submit">保存</button>
  </div>
</form>

<style>
  .vault-form {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .row {
    display: flex;
    gap: 0.4rem;
  }

  .row .field {
    flex: 1;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .label {
    font-size: var(--font-sm, 0.6rem);
    color: var(--text-muted);
    font-weight: 500;
  }

  .input {
    background: var(--surface-2);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md, 0.3rem);
    color: var(--text-primary);
    font-size: var(--font-sm, 0.7rem);
    font-family: inherit;
    padding: 0.3rem 0.45rem;
    outline: none;
    transition: border-color 0.12s;
    width: 100%;
  }

  .input::placeholder {
    color: var(--text-faint);
  }

  .input:focus {
    border-color: color-mix(in srgb, var(--color-primary) 40%, transparent);
  }

  .textarea {
    resize: vertical;
    min-height: 2rem;
    line-height: 1.45;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
    margin-top: 0.1rem;
  }

  .btn-submit {
    padding: 0.25rem 0.7rem;
    background: color-mix(in srgb, var(--color-primary) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-primary) 30%, transparent);
    color: var(--color-primary);
    font-size: var(--font-sm, 0.65rem);
    font-weight: 500;
    border-radius: var(--radius-md, 0.3rem);
    cursor: pointer;
    font-family: inherit;
    transition: background 0.12s;
  }

  .btn-submit:hover {
    background: color-mix(in srgb, var(--color-primary) 25%, transparent);
  }
</style>
