<script lang="ts">
  import type { PanePatch, PhonePaneMeta } from '../../src/lib/remoteTypes';
  import { request } from './link.svelte';
  import Setup from './Setup.svelte';

  let { slot, meta, onclose }: { slot: number; meta: PhonePaneMeta; onclose: () => void } = $props();

  // Start from the pane's current settings (a copy; nothing changes until Apply).
  // svelte-ignore state_referenced_locally
  const start = { ...meta };
  let provider = $state(start.provider);
  let accountId = $state(start.accountId);
  let model = $state(start.model);
  let effort = $state(start.effort);
  let permission = $state(start.permission);
  let mode = $state(start.mode);
  let cwd = $state(start.cwd);
  let saving = $state(false);
  let error = $state('');

  async function apply() {
    const patch: PanePatch = {};
    if (provider !== start.provider) patch.provider = provider;
    if (accountId !== start.accountId) patch.accountId = accountId;
    if (permission !== start.permission) patch.permission = permission;
    if (model !== start.model) patch.model = model;
    if (effort !== start.effort) patch.effort = effort;
    if (mode !== start.mode) patch.mode = mode;
    if (cwd.trim() && cwd.trim() !== start.cwd) patch.cwd = cwd.trim();
    if (!Object.keys(patch).length) return onclose();
    saving = true;
    error = '';
    try {
      await request({ op: 'config', slot, patch }, 120_000);
      onclose();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      saving = false;
    }
  }
</script>

<div class="sheet-bg" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label="Pane settings">
  <div class="sheet-head">
    <b>This pane</b>
    <button class="ghost" onclick={onclose}>Cancel</button>
  </div>
  {#if meta.busy}<p class="muted small">It's working: you can change the permission and model now; the rest after it finishes.</p>{/if}
  <Setup bind:provider bind:accountId bind:model bind:effort bind:permission bind:mode bind:cwd locked={meta.busy} />
  <p class="muted small">Changing the AI, account or folder keeps this conversation going.</p>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <button class="primary big" disabled={saving} onclick={apply}>{saving ? 'Applying…' : 'Apply'}</button>
</div>
