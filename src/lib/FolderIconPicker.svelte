<script lang="ts">
  import { FOLDER_COLORS, FOLDER_ICONS } from './folderIcons';
  import { folders, lookFor, setLook } from './folders.svelte';

  let { cwd, name, onclose }: { cwd: string; name: string; onclose: () => void } = $props();

  const look = $derived(lookFor(cwd));
  const suggested = $derived(folders.auto[cwd] ?? null);
  let root = $state<HTMLDivElement>();

  $effect(() => {
    root?.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')?.focus();
    const close = (e: PointerEvent) => {
      if (root && !root.contains(e.target as Node)) onclose();
    };
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  });

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    onclose();
  }

  function pick(icon: string, color = look.auto ? '' : look.color) {
    setLook(cwd, { icon, color });
  }
</script>

<div
  class="pop"
  bind:this={root}
  data-overlay
  role="dialog"
  aria-label={`Icon for ${name}`}
  tabindex="-1"
  onkeydown={onKey}
>
  <div class="head">
    <b>Icon for {name}</b>
    {#if !look.auto}
      <button class="link" onclick={() => setLook(cwd, null)} title="Use the icon Wren picks from the project files">Automatic</button>
    {/if}
  </div>
  <div class="grid" role="group" aria-label="Icon">
    {#each FOLDER_ICONS as ic (ic.id)}
      <button class="ic" aria-pressed={look.icon === ic.id} aria-label={ic.label} title={ic.id === suggested && look.auto ? `${ic.label} (picked from the files)` : ic.label} onclick={() => pick(ic.id)}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html ic.body}</svg>
      </button>
    {/each}
  </div>
  <div class="colors" role="group" aria-label="Color">
    {#each FOLDER_COLORS as c (c.id)}
      <button
        class="sw"
        class:plain={!c.value}
        aria-pressed={!look.auto && look.color === c.id}
        aria-label={c.label}
        title={c.label}
        style:--sw={c.value || null}
        onclick={() => setLook(cwd, { icon: look.icon, color: c.id })}
      ></button>
    {/each}
  </div>
  <p class="hint">{look.auto ? (suggested ? 'Picked by Wren from the project files. Choose one to change it.' : 'Choose an icon for this folder.') : 'Saved on this computer only.'}</p>
</div>

<style>
  .pop {
    position: absolute;
    top: calc(100% + 4px);
    left: 4px;
    z-index: 30;
    width: 236px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.25);
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-weight: 400;
  }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 12.5px; }
  .head b { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 600; }
  .link { border: 0; background: none; color: var(--accent); font: inherit; font-size: 12px; cursor: pointer; padding: 2px 4px; }
  .link:hover { text-decoration: underline; }
  .grid { display: grid; grid-template-columns: repeat(6, 1fr); gap: 4px; }
  .ic {
    display: grid; place-items: center; height: 32px;
    border: 1px solid transparent; border-radius: 8px; background: none; color: var(--text); cursor: pointer;
  }
  .ic:hover { background: var(--surface-2); }
  .ic[aria-pressed='true'] { border-color: var(--accent); background: var(--surface-2); }
  .colors { display: flex; gap: 4px; justify-content: space-between; }
  .sw {
    width: 20px; height: 20px; border-radius: 50%; padding: 0; cursor: pointer;
    border: 2px solid var(--surface); background: var(--sw, var(--text)); box-shadow: 0 0 0 1px var(--border);
  }
  .sw.plain { background: linear-gradient(135deg, var(--text) 50%, var(--muted) 50%); }
  .sw[aria-pressed='true'] { box-shadow: 0 0 0 2px var(--accent); }
  .hint { margin: 0; font-size: 11.5px; color: var(--muted); line-height: 1.4; }
</style>
