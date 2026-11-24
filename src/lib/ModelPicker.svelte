<script lang="ts">
  import { onMount } from 'svelte';
  import { ensureModels, effortLabel, modelLists, modelsKey } from './models.svelte';
  import type { Provider } from './types';

  let {
    provider,
    accountId,
    model = $bindable(''),
    effort = $bindable(''),
    disabled = false,
    onchange,
  }: {
    provider: Provider;
    accountId: string;
    model?: string;
    effort?: string;
    disabled?: boolean;
    onchange: () => void;
  } = $props();

  let open = $state(false);
  let custom = $state('');
  let root: HTMLDivElement | undefined = $state();

  $effect(() => {
    ensureModels(provider, accountId);
  });

  const list = $derived(modelLists[modelsKey(provider, accountId)]);
  const current = $derived(list?.find((m) => m.id === model));
  /** What "Default" means right now (Claude reports it exactly; Codex usually). */
  const defaultInfo = $derived(list?.find((m) => m.is_default));
  const efforts = $derived(current ? current.efforts : model ? [] : (defaultInfo?.efforts ?? list?.[0]?.efforts ?? []));
  const label = $derived(
    `${model ? (current?.label ?? model) : defaultInfo ? `${defaultInfo.label} (default)` : 'Default model'}${effort ? ' · ' + effortLabel(effort) : ''}`,
  );
  const defaultNote = $derived(
    defaultInfo ? `${provider === 'claude' ? 'currently' : 'usually'} ${defaultInfo.label}` : "the CLI's own choice",
  );

  function pickModel(id: string) {
    model = id;
    const info = list?.find((m) => m.id === id);
    // keep the effort only if the new model supports it
    if (effort && info && !info.efforts.includes(effort)) effort = '';
    onchange();
  }

  function pickEffort(e: string) {
    effort = e;
    onchange();
  }

  function useCustom() {
    const id = custom.trim();
    if (!id) return;
    custom = '';
    pickModel(id);
    open = false;
  }

  onMount(() => {
    const close = (e: PointerEvent) => {
      if (open && root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  });
</script>

<div class="picker" bind:this={root}>
  <button
    class="chip"
    {disabled}
    aria-haspopup="true"
    aria-expanded={open}
    title="Model and effort"
    onclick={() => (open = !open)}
    onkeydown={(e) => e.key === 'Escape' && (open = false)}
  >
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l1.9 4.8L19 9.7l-4 3.6 1.2 5.2L12 15.8 7.8 18.5 9 13.3 5 9.7l5.1-1.9z" /></svg>
    <span>{label}</span>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
  </button>

  {#if open}
    <div class="pop" role="dialog" aria-label="Choose model and effort" tabindex="-1" onkeydown={(e) => e.key === 'Escape' && (open = false)}>
      <div class="section">Model</div>
      <button class="row" class:sel={!model} onclick={() => pickModel('')}>
        <span class="name">Default</span><span class="desc">{defaultNote}</span>
      </button>
      {#if !list}
        <div class="muted">Loading models…</div>
      {:else}
        {#each list as m (m.id)}
          <button class="row" class:sel={model === m.id} onclick={() => pickModel(m.id)} title={m.resolved ? `${m.id} → ${m.resolved}` : m.id}>
            <span class="name">{m.label}</span>
            {#if m.description}<span class="desc">{m.description}</span>{/if}
            {#if m.resolved && m.resolved !== m.id}
              <small>{m.resolved}</small>
            {:else if m.label !== m.id}
              <small>{m.id}</small>
            {/if}
          </button>
        {/each}
      {/if}
      <form class="custom" onsubmit={(e) => { e.preventDefault(); useCustom(); }}>
        <label class="sr" for="custom-model">Other model id</label>
        <input id="custom-model" placeholder="Other model id…" bind:value={custom} />
        <button class="ghost" type="submit" disabled={!custom.trim()}>Use</button>
      </form>

      {#if efforts.length}
        <div class="section">Effort</div>
        <div class="efforts" role="group" aria-label="Effort">
          <button class:on={!effort} aria-pressed={!effort} onclick={() => pickEffort('')}>Default</button>
          {#each efforts as e}
            <button class:on={effort === e} aria-pressed={effort === e} onclick={() => pickEffort(e)}>{effortLabel(e)}</button>
          {/each}
        </div>
      {/if}
      <p class="note">
        {#if provider === 'claude'}Each Claude model follows the newest version of its family, so the version shown updates by itself.{' '}{/if}Changing the model keeps this conversation.
      </p>
    </div>
  {/if}
</div>

<style>
  .picker { position: relative; min-width: 0; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 240px;
    min-height: 30px;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 12.5px;
    cursor: pointer;
  }
  .chip span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip:hover:not(:disabled) { border-color: var(--accent); }
  .pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 20;
    width: 320px;
    max-height: 460px;
    overflow-y: auto;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.25);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .section { font-size: 11px; font-weight: 600; color: var(--muted); text-transform: uppercase; letter-spacing: 0.04em; padding: 6px 8px 4px; }
  .row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    text-align: left;
    border: 0;
    background: transparent;
    padding: 7px 8px;
    border-radius: 8px;
    cursor: pointer;
    min-height: 36px;
    margin-bottom: 2px;
    flex-shrink: 0;
  }
  .row small { color: var(--muted); font-family: var(--mono); font-size: 11px; }
  .row .name { font-weight: 600; color: var(--text); }
  .row .desc { color: var(--muted); font-size: 12px; line-height: 1.35; }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: var(--surface-2); box-shadow: inset 3px 0 0 var(--accent-fill); }
  .custom { display: flex; gap: 6px; padding: 6px 4px 2px; }
  .custom input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: 12px;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
  }
  .efforts { display: flex; flex-wrap: wrap; gap: 4px; padding: 0 4px 4px; }
  .efforts button {
    border: 1px solid var(--border);
    background: var(--surface-2);
    border-radius: 999px;
    padding: 4px 10px;
    font-size: 12px;
    min-height: 30px;
    cursor: pointer;
  }
  .efforts button.on { background: var(--accent-fill); border-color: var(--accent-fill); color: #17181c; font-weight: 600; }
  .muted, .note { color: var(--muted); font-size: 12px; padding: 4px 8px; margin: 0; }
</style>
