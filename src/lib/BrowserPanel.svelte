<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import type { BrowserEvent } from './types';

  /**
   * The browser panel next to a pane's chat. The page itself is a separate
   * native webview that Wren keeps exactly over `holder`; it can't call any
   * of Wren's commands (see src-tauri/src/browser.rs).
   */
  let { paneId, onclose }: { paneId: string; onclose: () => void } = $props();

  let holder: HTMLDivElement | undefined = $state();
  let address = $state('');
  let title = $state('');
  let loading = $state(false);
  let notice = $state<{ text: string; url: string | null } | null>(null);
  /** The native page exists (after the first address is opened). */
  let created = $state(false);
  let editing = false;

  const channel = new Channel<BrowserEvent>();
  channel.onmessage = (ev) => {
    switch (ev.kind) {
      case 'loading':
        loading = true;
        notice = null;
        if (!editing) address = ev.url;
        break;
      case 'loaded':
        loading = false;
        if (!editing) address = ev.url;
        break;
      case 'title':
        title = ev.title;
        break;
      case 'blocked':
        loading = false;
        notice = { text: ev.reason, url: /^https?:/i.test(ev.url) ? ev.url : null };
        break;
    }
  };

  /** The box the page must cover, in CSS pixels of Wren's window. */
  function box() {
    const r = holder?.getBoundingClientRect();
    // vh: Wren's page height, so macOS can tell how far the page sits below the title bar.
    return r ? { x: r.left, y: r.top, w: r.width, h: r.height, vh: window.innerHeight } : { x: 0, y: 0, w: 0, h: 0, vh: 0 };
  }

  /** Open an address (typed, a link from the chat, or a visualization). */
  export async function go(url: string) {
    notice = null;
    editing = false;
    address = url;
    try {
      address = await invoke<string>('browser_open', { paneId, url, bounds: box(), onEvent: channel });
      created = true;
      last = '';
    } catch (e) {
      notice = { text: String(e), url: null };
    }
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (address.trim()) go(address.trim());
  }

  function nav(action: 'back' | 'forward' | 'reload' | 'stop') {
    if (created) invoke('browser_go', { paneId, action }).catch((e) => (notice = { text: String(e), url: null }));
  }

  function openOutside(url = address) {
    invoke('open_external', { url }).catch((e) => (notice = { text: String(e), url: null }));
  }

  // Keep the native page over `holder`. It sits above everything in Wren's
  // window, so it hides while a dialog, menu or picture viewer is open.
  let raf = 0;
  let last = '';
  function follow() {
    raf = requestAnimationFrame(follow);
    if (!created) return;
    const b = box();
    const covered = !!document.querySelector('dialog[open], .lightbox, [data-overlay]');
    const visible = !covered && !notice && b.w > 2 && b.h > 2;
    const key = visible ? `${Math.round(b.x)},${Math.round(b.y)},${Math.round(b.w)},${Math.round(b.h)}` : 'hidden';
    if (key === last) return;
    last = key;
    invoke('browser_place', { paneId, bounds: b, visible }).catch(() => {});
  }

  onMount(() => {
    raf = requestAnimationFrame(follow);
    // Full screen on/off changes the title bar (Wren corrects for it), not the box: place again.
    const again = () => (last = '');
    window.addEventListener('resize', again);
    return () => window.removeEventListener('resize', again);
  });

  onDestroy(() => {
    cancelAnimationFrame(raf);
    invoke('browser_close', { paneId }).catch(() => {});
  });
</script>

<div class="browser" role="region" aria-label="Browser">
  <form class="toolbar" onsubmit={submit}>
    <button type="button" class="ghost ib" onclick={() => nav('back')} aria-label="Back" title="Back">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
    </button>
    <button type="button" class="ghost ib" onclick={() => nav('forward')} aria-label="Forward" title="Forward">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 18 6-6-6-6" /></svg>
    </button>
    <button type="button" class="ghost ib" onclick={() => nav(loading ? 'stop' : 'reload')} aria-label={loading ? 'Stop loading' : 'Reload'} title={loading ? 'Stop' : 'Reload'}>
      {#if loading}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      {:else}
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" /></svg>
      {/if}
    </button>
    <label class="sr" for={`${paneId}-address`}>Address</label>
    <input
      id={`${paneId}-address`}
      class="address"
      bind:value={address}
      onfocus={(e) => { editing = true; (e.currentTarget as HTMLInputElement).select(); }}
      onblur={() => (editing = false)}
      placeholder="Type an address, e.g. localhost:5173"
      spellcheck="false"
      autocomplete="off"
      title={title || address}
    />
    <button type="button" class="ghost ib" onclick={() => openOutside()} disabled={!address} aria-label="Open in your browser" title="Open in your browser">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
    </button>
    <button type="button" class="ghost ib" onclick={onclose} aria-label="Close the browser" title="Close the browser">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </form>
  {#if loading}<div class="progress" aria-hidden="true"></div>{/if}
  <div class="holder" bind:this={holder}>
    {#if notice}
      <div class="notice" role="alert">
        <p>{notice.text}</p>
        <div class="row">
          {#if notice.url}<button class="primary" onclick={() => openOutside(notice!.url!)}>Open in your browser</button>{/if}
          <button class="ghost" onclick={() => { notice = null; last = ''; }}>{created ? 'Back to the page' : 'OK'}</button>
        </div>
      </div>
    {:else if !created}
      <div class="empty">
        <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
        <p>Type an address above, click a link in the chat, or turn on <b>Visualize</b> in the + menu and ask about an idea.</p>
        <p class="hint">Sign-in pages of Claude, ChatGPT and Google open in your normal browser.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .browser { display: flex; flex-direction: column; min-width: 0; min-height: 0; flex: 1; background: var(--surface); }
  .toolbar { display: flex; align-items: center; gap: 4px; padding: 6px; border-bottom: 1px solid var(--border); background: var(--surface-2); }
  .ib { padding: 4px 6px; display: inline-flex; align-items: center; justify-content: center; color: var(--muted); flex-shrink: 0; }
  .ib:hover:not(:disabled) { color: var(--text); }
  .address {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 12.5px;
  }
  .address:focus { outline: none; border-color: var(--accent); }
  .progress { height: 2px; background: linear-gradient(90deg, transparent, var(--accent-fill), transparent); background-size: 50% 100%; background-repeat: no-repeat; animation: slide 1.1s linear infinite; }
  @keyframes slide { from { background-position: -50% 0; } to { background-position: 150% 0; } }
  @media (prefers-reduced-motion: reduce) { .progress { animation: none; background: var(--accent-fill); opacity: 0.5; } }
  .holder { position: relative; flex: 1; min-height: 0; }
  .empty, .notice { position: absolute; inset: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; padding: 20px; text-align: center; color: var(--muted); font-size: 13px; }
  .empty p, .notice p { margin: 0; max-width: 34ch; line-height: 1.5; }
  .notice { color: var(--text); background: var(--surface); }
  .notice .row { display: flex; gap: 8px; flex-wrap: wrap; justify-content: center; }
  .hint { font-size: 12px; }
</style>
