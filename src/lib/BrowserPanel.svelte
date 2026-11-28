<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
  import type { BrowserEvent } from './types';
  import CodeView from './CodeView.svelte';
  import { copyText } from './markdown';

  /**
   * The browser panel next to a pane's chat. The page itself is a separate
   * native webview that Wren keeps exactly over `holder`; it can't call any
   * of Wren's commands (see src-tauri/src/browser.rs).
   * The Code tab shows the page's source: local pages can be edited (saved
   * about 1 s after you stop typing, then the page reloads); web pages are
   * a read-only copy of their HTML.
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
  /** The address the page is really showing (the box may hold half-typed text). */
  let pageUrl = $state('');

  type Source = { kind: 'file' | 'web'; name: string; path: string | null; text: string | null; mtime: number; editable: boolean; note: string | null };
  let tab = $state<'preview' | 'code'>('preview');
  let code: CodeView | undefined = $state();
  /** The source shown in the Code tab, and the address it belongs to. */
  let src = $state<(Source & { text: string }) | null>(null);
  let srcUrl = $state('');
  let srcError = $state('');
  let srcLoading = $state(false);
  let save = $state<'saved' | 'unsaved' | 'saving' | 'failed'>('saved');
  let saveError = $state('');
  let flash = $state('');
  let wrap = $state(localStorage.getItem('wren.codewrap') === '1');
  let pending: string | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let flashTimer: ReturnType<typeof setTimeout> | undefined;
  const canCode = $derived(created && /^https?:/i.test(pageUrl));
  const REVEAL = navigator.userAgent.includes('Mac') ? 'Show in Finder' : navigator.userAgent.includes('Windows') ? 'Show in Explorer' : 'Show in folder';

  const channel = new Channel<BrowserEvent>();
  channel.onmessage = (ev) => {
    switch (ev.kind) {
      case 'loading':
        loading = true;
        notice = null;
        pageUrl = ev.url;
        if (!editing) address = ev.url;
        break;
      case 'loaded':
        loading = false;
        pageUrl = ev.url;
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
    await flushSave();
    try {
      address = await invoke<string>('browser_open', { paneId, url, bounds: box(), onEvent: channel });
      pageUrl = address;
      created = true;
      last = '';
      // The same page again (e.g. the AI updated its visualization): stay in Code.
      if (tab === 'code' && address === srcUrl) loadSource(true);
      else tab = 'preview';
    } catch (e) {
      notice = { text: String(e), url: null };
    }
  }

  function submit(e: SubmitEvent) {
    e.preventDefault();
    if (address.trim()) {
      tab = 'preview';
      go(address.trim());
    }
  }

  // ---- the Code tab ----

  async function showTab(next: 'preview' | 'code') {
    if (next === tab) return;
    if (next === 'preview') {
      await flushSave();
      tab = 'preview';
      return;
    }
    tab = 'code';
    loadSource(srcUrl !== pageUrl);
  }

  /** (Re)read the page's source. `fresh` = always get the text. */
  async function loadSource(fresh: boolean) {
    const url = pageUrl;
    if (fresh) srcError = '';
    // "Loading…" only for another page; the same page updates in place.
    if (!src || srcUrl !== url) {
      srcLoading = true;
      flash = '';
    }
    try {
      const s = await invoke<Source>('browser_source', { url, knownMtime: !fresh && src && srcUrl === url ? src.mtime : null });
      if (url !== pageUrl || tab !== 'code') return;
      // The user typed meanwhile: their text wins (it saves over the file).
      if (pending !== null || save === 'saving') return;
      if (s.text === null) {
        if (src) src.mtime = s.mtime;
        return;
      }
      const same = srcUrl === url && !!src;
      src = { ...s, text: s.text };
      srcUrl = url;
      save = 'saved';
      saveError = '';
      if (same) code?.setText(s.text);
    } catch (e) {
      if (url === pageUrl) srcError = String(e);
    } finally {
      if (url === pageUrl) srcLoading = false;
    }
  }

  function onEdit(text: string) {
    pending = text;
    save = 'unsaved';
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => void flushSave(), 1000);
  }

  /** Write waiting edits now (also before leaving the tab or the page). */
  async function flushSave() {
    clearTimeout(saveTimer);
    if (pending === null || !src?.editable) return;
    const text = pending;
    pending = null;
    save = 'saving';
    try {
      const mtime = await invoke<number>('browser_save_source', { url: srcUrl, text });
      if (src) {
        src.mtime = mtime;
        src.text = text;
      }
      save = pending === null ? 'saved' : 'unsaved';
      saveError = '';
      if (srcUrl === pageUrl) nav('reload');
    } catch (e) {
      save = 'failed';
      saveError = String(e);
      pending ??= text;
    }
  }

  function say(text: string) {
    flash = text;
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flash = ''), 2500);
  }

  async function copyCode() {
    say((await copyText(code?.getText() ?? src?.text ?? '')) ? '✓ Copied' : "Couldn't copy");
  }

  async function saveAs() {
    if (!src) return;
    try {
      const where = await invoke<string | null>('browser_save_as', { name: src.name || 'page.html', text: code?.getText() ?? src.text });
      if (where) say(`✓ Saved a copy as ${where.split(/[\\/]/).pop()}`);
    } catch (e) {
      say(String(e));
    }
  }

  function reveal() {
    invoke('browser_reveal', { url: srcUrl }).catch((e) => say(String(e)));
  }

  function toggleWrap() {
    wrap = !wrap;
    localStorage.setItem('wren.codewrap', wrap ? '1' : '0');
  }

  function codeKeys(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 's') {
      e.preventDefault();
      void flushSave();
    }
  }

  // While the Code tab shows a local file, notice when it changes on disk
  // (e.g. the AI rewrote it) and show the new text.
  $effect(() => {
    if (tab !== 'code' || src?.kind !== 'file') return;
    const t = setInterval(() => {
      if (pending === null && save !== 'saving') loadSource(false);
    }, 1500);
    return () => clearInterval(t);
  });

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
    const visible = !covered && !notice && tab === 'preview' && b.w > 2 && b.h > 2;
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
    void flushSave();
    clearTimeout(flashTimer);
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
    <div class="tabs" role="tablist" aria-label="Show">
      <button type="button" role="tab" aria-selected={tab === 'preview'} class:on={tab === 'preview'} onclick={() => showTab('preview')} title="The page">Preview</button>
      <button type="button" role="tab" aria-selected={tab === 'code'} class:on={tab === 'code'} onclick={() => showTab('code')} disabled={!canCode} title={canCode ? "The page's code (HTML, CSS and JavaScript)" : 'Open a page first'}>Code</button>
    </div>
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
    {:else if tab === 'code'}
      <div class="codepane" role="tabpanel" aria-label="Code" tabindex="-1" onkeydown={codeKeys}>
        <div class="codebar">
          <div class="file" title={src?.path ?? srcUrl}>
            <span class="fname">{src?.name ?? '…'}</span>
            {#if src}
              {#if !src.editable}
                <span class="state">Read-only</span>
              {:else if save === 'saving'}
                <span class="state">Saving…</span>
              {:else if save === 'unsaved'}
                <span class="state">Edited</span>
              {:else if save === 'failed'}
                <span class="state bad" title={saveError}>⚠ Not saved</span>
              {:else}
                <span class="state ok">✓ Saved</span>
              {/if}
            {/if}
          </div>
          {#if flash}<span class="flash" role="status">{flash}</span>{/if}
          <button type="button" class="ghost cb" class:on={wrap} onclick={toggleWrap} aria-pressed={wrap} aria-label="Wrap long lines" title="Wrap long lines">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M3 12h15a3 3 0 1 1 0 6h-4M3 18h7m6-3-3 3 3 3" /></svg>
            <span class="lbl">Wrap</span>
          </button>
          <button type="button" class="ghost cb" onclick={copyCode} disabled={!src} aria-label="Copy" title="Copy all the code">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1" /></svg>
            <span>Copy</span>
          </button>
          <button type="button" class="ghost cb" onclick={saveAs} disabled={!src} aria-label="Save as…" title="Save a copy somewhere else">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v12m0 0-4-4m4 4 4-4M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" /></svg>
            <span class="lbl">Save as…</span>
          </button>
          {#if src?.kind === 'file'}
            <button type="button" class="ghost cb" onclick={reveal} aria-label={REVEAL} title={`${REVEAL}: ${src.path ?? ''}`}>
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
              <span class="lbl">{REVEAL}</span>
            </button>
          {/if}
        </div>
        {#if src?.note || (save === 'failed' && saveError)}
          <p class="srcnote" class:bad={save === 'failed'}>{save === 'failed' ? saveError : src?.note}</p>
        {/if}
        <div class="codebox">
          {#if srcError}
            <div class="notice" role="alert">
              <p>{srcError}</p>
              <div class="row"><button class="ghost" onclick={() => loadSource(true)}>Try again</button></div>
            </div>
          {:else if src && !srcLoading}
            {#key srcUrl}
              <CodeView bind:this={code} text={src.text} name={src.name} editable={src.editable} {wrap} onedit={onEdit} />
            {/key}
          {:else}
            <div class="empty"><p>Loading the code…</p></div>
          {/if}
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
  .tabs { display: inline-flex; flex-shrink: 0; padding: 2px; gap: 2px; border: 1px solid var(--border); border-radius: 999px; background: var(--bg); }
  .tabs button { border: 0; background: transparent; color: var(--muted); font: inherit; font-size: 12px; padding: 2px 9px; border-radius: 999px; cursor: pointer; }
  .tabs button:hover:not(:disabled) { color: var(--text); }
  .tabs button.on { background: var(--surface-2); color: var(--text); font-weight: 600; }
  .tabs button:disabled { opacity: 0.45; cursor: default; }
  .codepane { position: absolute; inset: 0; display: flex; flex-direction: column; background: var(--bg); outline: none; container-type: inline-size; }
  .codebar { display: flex; align-items: center; gap: 2px 4px; padding: 4px 6px 4px 10px; border-bottom: 1px solid var(--border); background: var(--surface); }
  .file { flex: 1; min-width: 4rem; display: flex; align-items: baseline; gap: 8px; overflow: hidden; }
  .fname { font-family: var(--mono); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .state { font-size: 11.5px; color: var(--muted); white-space: nowrap; }
  .state.ok { color: var(--ok); }
  .state.bad, .srcnote.bad { color: var(--danger); }
  .flash { font-size: 12px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 16rem; }
  .cb { display: inline-flex; align-items: center; gap: 4px; padding: 3px 7px; font-size: 12px; color: var(--muted); flex-shrink: 0; }
  .cb:hover:not(:disabled), .cb.on { color: var(--text); }
  .cb.on { background: var(--surface-2); }
  /* Narrow panel: icons only (the names stay in the hover text). */
  @container (max-width: 560px) {
    .lbl { display: none; }
    .flash { max-width: 9rem; }
  }
  .srcnote { margin: 0; padding: 5px 10px; font-size: 11.5px; line-height: 1.45; color: var(--muted); background: var(--surface-2); border-bottom: 1px solid var(--border); }
  .codebox { position: relative; flex: 1; min-height: 0; }
</style>
