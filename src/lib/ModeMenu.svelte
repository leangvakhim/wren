<script lang="ts">
  import { onMount } from 'svelte';
  import { MODES, type Mode, type Provider } from './types';

  /** The "+" button in the message box: Study and Deep research (like Claude Desktop). */
  let {
    mode = $bindable(null),
    provider,
    disabled = false,
    onchange,
    onaddfiles,
  }: { mode?: Mode | null; provider: Provider; disabled?: boolean; onchange: () => void; onaddfiles?: () => void } = $props();

  let open = $state(false);
  let root: HTMLDivElement | undefined = $state();

  /** What each AI uses for the mode (shown under the option). */
  const HOW: Record<Mode, Record<Provider, string>> = {
    study: {
      claude: "Claude Code's built-in Learning style + tutor instructions",
      codex: 'Tutor instructions for Codex',
      antigravity: 'Tutor instructions sent with each message',
    },
    research: {
      claude: 'Web search + fetch, parallel sub-agents, high effort',
      codex: 'Live web search, high reasoning effort',
      antigravity: "Antigravity's own web search + research instructions",
    },
  };

  function pick(m: Mode) {
    mode = mode === m ? null : m;
    open = false;
    onchange();
  }

  function clear() {
    mode = null;
    onchange();
  }

  onMount(() => {
    const close = (e: PointerEvent) => {
      if (open && root && !root.contains(e.target as Node)) open = false;
    };
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  });
</script>

{#snippet icon(m: Mode)}
  {#if m === 'study'}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 9 12 4 2 9l10 5 10-5z" /><path d="M6 11v5c3 2 9 2 12 0v-5" /></svg>
  {:else}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5M8 11h6M11 8v6" /></svg>
  {/if}
{/snippet}

<div class="wrap" bind:this={root}>
  <button
    class="plus"
    {disabled}
    aria-haspopup="menu"
    aria-expanded={open}
    aria-label="Add files, Study or Deep research"
    title="Add files, Study or Deep research"
    onclick={() => (open = !open)}
    onkeydown={(e) => e.key === 'Escape' && (open = false)}
  >
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
  </button>
  {#if open}
    <div class="pop" role="menu" tabindex="-1" aria-label="Add or choose a mode" onkeydown={(e) => e.key === 'Escape' && (open = false)}>
      {#if onaddfiles}
        <button class="row" role="menuitem" onclick={() => { open = false; onaddfiles(); }}>
          <span class="title">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m21.4 11.6-8.5 8.5a5.5 5.5 0 0 1-7.8-7.8l8.5-8.5a3.7 3.7 0 0 1 5.2 5.2l-8.5 8.5a1.8 1.8 0 0 1-2.6-2.6l7.8-7.8" /></svg>
            Add files…
          </span>
          <span class="hint">Images, PDFs, spreadsheets, documents, zips, code… up to 100 MB each. You can also paste (⌘V) or drop files in the message box.</span>
        </button>
        <div class="sep" role="separator"></div>
      {/if}
      {#each Object.keys(MODES) as k}
        {@const m = k as Mode}
        <button class="row" class:sel={mode === m} role="menuitemcheckbox" aria-checked={mode === m} onclick={() => pick(m)}>
          <span class="title">{@render icon(m)} {MODES[m].label}{#if mode === m}<span class="on">On</span>{/if}</span>
          <span class="hint">{MODES[m].hint}</span>
          <small>{HOW[m][provider]}</small>
        </button>
      {/each}
      <p class="note">Turning a mode on or off keeps the conversation.</p>
    </div>
  {/if}
</div>
{#if mode}
  <button class="chip on" {disabled} onclick={clear} title={`${MODES[mode].label} is on. Click to turn it off.`} aria-label={`Turn off ${MODES[mode].label}`}>
    {@render icon(mode)}
    <span>{MODES[mode].label}</span>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
  </button>
{/if}

<style>
  .wrap { position: relative; }
  .plus {
    width: 30px;
    height: 30px;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--text);
    display: grid;
    place-items: center;
    cursor: pointer;
    padding: 0;
  }
  .plus:hover:not(:disabled) { border-color: var(--accent); }
  .pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 20;
    width: 300px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.25);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    text-align: left;
    border: 0;
    background: transparent;
    color: var(--text);
    padding: 8px;
    border-radius: 8px;
    cursor: pointer;
  }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: var(--surface-2); box-shadow: inset 3px 0 0 var(--accent-fill); }
  .title { display: inline-flex; align-items: center; gap: 6px; font-weight: 600; font-size: 13px; }
  .on { font-size: 11px; font-weight: 600; color: var(--accent); margin-left: 4px; }
  .hint { font-size: 12px; color: var(--muted); line-height: 1.4; }
  small { font-size: 11px; color: var(--muted); font-family: var(--mono); }
  .sep { height: 1px; background: var(--border); margin: 4px 6px; }
  .note { color: var(--muted); font-size: 11px; padding: 4px 8px 0; margin: 0; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 30px;
    padding: 3px 10px;
    border: 1px solid var(--accent-fill);
    border-radius: 999px;
    background: color-mix(in oklab, var(--accent-fill) 18%, var(--surface));
    color: var(--text);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
  }
</style>
