<script lang="ts">
  // The AI / account / model / permission / mode / folder fields, shared by
  // "New task" and a pane's settings.
  import { MODES, PERMISSIONS, PROVIDERS, type Mode, type ModelInfo, type Permission, type Provider } from '../../src/lib/types';
  import { link } from './link.svelte';
  import { EFFORTS, loadModels } from './models';

  let {
    provider = $bindable(),
    accountId = $bindable(),
    model = $bindable(),
    effort = $bindable(),
    permission = $bindable(),
    mode = $bindable(),
    cwd = $bindable(),
    locked = false,
  }: {
    provider: Provider;
    accountId: string;
    model: string;
    effort: string;
    permission: Permission;
    mode: Mode | null;
    cwd: string;
    /** The pane is working: only permission, model and effort can change. */
    locked?: boolean;
  } = $props();

  const installed = $derived(link.app?.installed ?? { claude: true, codex: true, antigravity: true });
  const accounts = $derived((link.app?.accounts ?? []).filter((a) => a.provider === provider));

  let models = $state<ModelInfo[]>([]);
  let loadingModels = $state(false);
  $effect(() => {
    const p = provider;
    const a = accountId;
    models = [];
    if (!a) return;
    loadingModels = true;
    loadModels(p, a).then((list) => {
      if (p === provider && a === accountId) models = list;
      loadingModels = false;
    });
  });
  const chosen = $derived(models.find((m) => m.id === model) ?? models.find((m) => m.is_default));
  const efforts = $derived(chosen?.efforts ?? []);

  function changeProvider() {
    const list = (link.app?.accounts ?? []).filter((a) => a.provider === provider);
    accountId = (list.find((a) => a.usable) ?? list[0])?.id ?? '';
    model = '';
    effort = '';
  }

  // Folders: recent ones, or type another path.
  const folders = $derived(link.app?.folders ?? []);
  let other = $state(false);
  $effect(() => {
    if (cwd && folders.length && !folders.includes(cwd)) other = true;
  });
  const leaf = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;
</script>

<div class="form">
  <label class="field">
    <span>AI</span>
    <select bind:value={provider} onchange={changeProvider} disabled={locked}>
      {#each Object.entries(PROVIDERS) as [value, p]}
        <option {value} disabled={!installed[value as Provider]}>{p.label}{installed[value as Provider] ? '' : ' (not installed)'}</option>
      {/each}
    </select>
  </label>

  <label class="field">
    <span>Account</span>
    <select bind:value={accountId} disabled={locked}>
      {#each accounts as a (a.id)}
        <option value={a.id}>{a.name}{a.usable ? '' : ' (not signed in)'}</option>
      {:else}
        <option value="">No account yet (add one on the computer)</option>
      {/each}
    </select>
  </label>

  <label class="field">
    <span>Model {#if loadingModels}<small>loading…</small>{/if}</span>
    <select bind:value={model} onchange={() => (effort = '')}>
      <option value="">Default{models.find((m) => m.is_default) ? ` (${models.find((m) => m.is_default)?.label})` : ''}</option>
      {#each models as m (m.id)}
        <option value={m.id}>{m.label}</option>
      {/each}
      {#if model && !models.some((m) => m.id === model)}<option value={model}>{model}</option>{/if}
    </select>
  </label>

  {#if efforts.length}
    <label class="field">
      <span>Effort</span>
      <select bind:value={effort}>
        <option value="">Default{chosen?.default_effort ? ` (${EFFORTS[chosen.default_effort] ?? chosen.default_effort})` : ''}</option>
        {#each efforts as e}<option value={e}>{EFFORTS[e] ?? e}</option>{/each}
      </select>
    </label>
  {/if}

  <label class="field">
    <span>Permission</span>
    <select bind:value={permission} class:danger={permission === 'full'}>
      {#each PERMISSIONS as p}<option value={p.value}>{p.label}</option>{/each}
    </select>
    <small class="muted">{PERMISSIONS.find((p) => p.value === permission)?.hint}</small>
  </label>

  <label class="field">
    <span>Mode</span>
    <select
      value={mode ?? ''}
      disabled={locked}
      onchange={(e) => (mode = ((e.target as HTMLSelectElement).value || null) as Mode | null)}
    >
      <option value="">Normal</option>
      {#each Object.entries(MODES) as [value, m]}<option {value}>{m.label}</option>{/each}
    </select>
  </label>

  <label class="field">
    <span>Project folder</span>
    {#if !other}
      <select
        value={cwd}
        disabled={locked}
        onchange={(e) => {
          const v = (e.target as HTMLSelectElement).value;
          if (v === '__other') other = true;
          else cwd = v;
        }}
      >
        {#each folders as f (f)}<option value={f}>{leaf(f)} — {f}</option>{/each}
        <option value="__other">Another folder…</option>
      </select>
    {:else}
      <input bind:value={cwd} disabled={locked} placeholder="/Users/you/Projects/app" autocapitalize="off" autocomplete="off" spellcheck="false" />
      {#if folders.length}<button class="linklike" disabled={locked} onclick={() => { other = false; cwd = folders[0]; }}>Pick a recent folder</button>{/if}
    {/if}
  </label>
</div>
