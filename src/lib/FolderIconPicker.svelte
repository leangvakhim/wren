<script lang="ts">
  import { open } from '@tauri-apps/plugin-dialog';
  import { untrack } from 'svelte';
  import { FOLDER_COLORS } from './folderIcons';
  import { folders, lookFor, setLook } from './folders.svelte';
  import {
    BUILT_IN,
    addOwn,
    catalog,
    downloadTabler,
    emojiId,
    isEmoji,
    loadEmoji,
    loadLucide,
    loadOwn,
    loadTabler,
    removeOwn,
    removeTabler,
    type CatalogIcon,
  } from './iconCatalog.svelte';

  let { cwd, name, onclose }: { cwd: string; name: string; onclose: () => void } = $props();

  type Tab = 'icons' | 'emoji' | 'own';
  const PAGE = 240;

  const look = $derived(lookFor(cwd));
  const suggested = $derived(folders.auto[cwd] ?? null);
  const startIcon = untrack(() => lookFor(cwd).icon);
  let tab = $state<Tab>(startIcon.startsWith('emoji:') ? 'emoji' : startIcon.startsWith('own:') ? 'own' : 'icons');
  let query = $state('');
  let category = $state('');
  let limit = $state(PAGE);
  let root = $state<HTMLDivElement>();
  let search = $state<HTMLInputElement>();
  let pos = $state({ top: -9999, left: 0 });

  // Shown above everything (fixed), next to the folder row it belongs to.
  function place() {
    const a = root?.parentElement?.getBoundingClientRect();
    if (!a || !root) return;
    const h = root.offsetHeight;
    const w = root.offsetWidth;
    const top = a.bottom + 4 + h > innerHeight - 8 ? Math.max(8, innerHeight - 8 - h) : a.bottom + 4;
    pos = { top, left: Math.max(8, Math.min(a.left + 4, innerWidth - 8 - w)) };
  }

  $effect(() => {
    loadLucide();
    loadTabler();
    place();
    search?.focus();
    const close = (e: PointerEvent) => {
      if (root && !root.contains(e.target as Node)) onclose();
    };
    window.addEventListener('pointerdown', close);
    window.addEventListener('resize', place);
    window.addEventListener('scroll', place, true);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('resize', place);
      window.removeEventListener('scroll', place, true);
    };
  });

  $effect(() => {
    if (tab === 'emoji') loadEmoji();
    if (tab === 'own') loadOwn();
  });

  function onKey(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    e.stopPropagation();
    onclose();
  }

  function show(t: Tab) {
    tab = t;
    query = '';
    catalog.error = '';
    limit = PAGE;
    queueMicrotask(() => search?.focus());
  }

  function pick(icon: string, color = look.auto ? '' : look.color) {
    setLook(cwd, { icon, color });
  }

  const terms = $derived(query.toLowerCase().split(/\s+/).filter(Boolean));
  const rank = (i: CatalogIcon) => (i.name.toLowerCase().startsWith(terms[0]) ? 0 : i.name.toLowerCase().includes(terms[0]) ? 1 : 2);

  // ---- Icons ----
  const sets = $derived([
    { key: 'wren', title: 'Wren', icons: BUILT_IN },
    { key: 'lucide', title: 'Lucide', icons: catalog.lucide ?? [] },
    ...(catalog.tabler ? [{ key: 'tabler', title: 'Tabler', icons: catalog.tabler }] : []),
  ]);
  const iconCount = $derived(sets.reduce((n, s) => n + s.icons.length, 0));
  const categories = $derived([...new Set(sets.flatMap((s) => s.icons.flatMap((i) => i.categories)))].sort());
  const found = $derived(
    sets
      .map((s) => {
        if (!terms.length && !category) return s;
        const icons = s.icons.filter((i) => (!category || i.categories.includes(category)) && terms.every((t) => i.search.includes(t)));
        return { ...s, icons: terms.length ? icons.sort((a, b) => rank(a) - rank(b)) : icons };
      })
      .filter((s) => s.icons.length),
  );

  // ---- Emoji ----
  const emojiFound = $derived(
    (catalog.emoji ?? [])
      .map((g) => ({ key: g.name, title: g.name, items: terms.length ? g.items.filter((e) => terms.every((t) => e.name.includes(t))) : g.items }))
      .filter((g) => g.items.length),
  );
  const typed = $derived(query.trim());
  const pasted = $derived(isEmoji(typed) && !(catalog.emoji ?? []).some((g) => g.items.some((e) => e.text === typed)) ? typed : '');

  /** At most `limit` items over all sections (big sets render in pages). */
  function page<T>(sections: { key: string; title: string; items: T[] }[]) {
    let left = limit;
    const total = sections.reduce((n, s) => n + s.items.length, 0);
    const shown = sections.map((s) => {
      const n = Math.min(s.items.length, left);
      left -= n;
      return { ...s, items: s.items.slice(0, n) };
    });
    return { shown: shown.filter((s) => s.items.length), more: total - Math.min(total, limit) };
  }
  const iconPage = $derived(page(found.map((s) => ({ key: s.key, title: s.title, items: s.icons }))));
  const emojiPage = $derived(page(emojiFound));
  const more = $derived(tab === 'icons' ? iconPage.more : tab === 'emoji' ? emojiPage.more : 0);

  function onSearchKey(e: KeyboardEvent) {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    const first = tab === 'icons' ? iconPage.shown[0]?.items[0]?.id : tab === 'emoji' ? (pasted ? emojiId(pasted) : emojiPage.shown[0]?.items[0] && emojiId(emojiPage.shown[0].items[0].text)) : undefined;
    if (first) pick(first);
  }

  async function addPictures() {
    const chosen = await open({ multiple: true, filters: [{ name: 'Pictures', extensions: ['svg', 'png', 'jpg', 'jpeg', 'gif', 'webp'] }] });
    const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
    const last = paths.length ? await addOwn(paths) : null;
    if (last) pick(`own:${last.id}`);
  }

  function removePicture(id: string) {
    if (look.icon === `own:${id}`) setLook(cwd, null);
    removeOwn(id);
  }

  const fmt = (n: number) => n.toLocaleString('en-US');
</script>

<div
  class="pop"
  bind:this={root}
  data-overlay
  role="dialog"
  aria-label={`Icon for ${name}`}
  tabindex="-1"
  onkeydown={onKey}
  style:top="{pos.top}px"
  style:left="{pos.left}px"
>
  <div class="head">
    <b>Icon for {name}</b>
    {#if !look.auto}
      <button class="link" onclick={() => setLook(cwd, null)} title="Use the icon Wren picks from the project files">Automatic</button>
    {/if}
  </div>

  <div class="tabs" role="tablist" aria-label="Kind of icon">
    <button role="tab" aria-selected={tab === 'icons'} onclick={() => show('icons')}>Icons</button>
    <button role="tab" aria-selected={tab === 'emoji'} onclick={() => show('emoji')}>Emoji</button>
    <button role="tab" aria-selected={tab === 'own'} onclick={() => show('own')}>Your pictures</button>
  </div>

  {#if tab !== 'own'}
    <div class="find">
      <input
        bind:this={search}
        bind:value={query}
        oninput={() => (limit = PAGE)}
        onkeydown={onSearchKey}
        type="search"
        placeholder={tab === 'icons' ? `Search ${fmt(iconCount)} icons` : 'Search emoji, or paste one'}
        aria-label={tab === 'icons' ? 'Search icons' : 'Search emoji'}
        spellcheck="false"
        autocomplete="off"
      />
      {#if tab === 'icons'}
        <select bind:value={category} onchange={() => (limit = PAGE)} aria-label="Category">
          <option value="">All</option>
          {#each categories as c (c)}<option value={c}>{c}</option>{/each}
        </select>
      {/if}
    </div>
  {/if}

  <div class="scroll" role="group" aria-label={tab === 'icons' ? 'Icons' : tab === 'emoji' ? 'Emoji' : 'Your pictures'}>
    {#if tab === 'icons'}
      {#each iconPage.shown as s (s.key)}
        <div class="sec">{s.title}</div>
        <div class="grid">
          {#each s.items as ic (ic.id)}
            <button
              class="ic"
              aria-pressed={look.icon === ic.id}
              aria-label={s.key === 'wren' ? ic.name : `${ic.name} (${s.title})`}
              title={ic.id === suggested && look.auto ? `${ic.name} (picked from the files)` : s.key === 'wren' ? ic.name : `${ic.name} (${s.title})`}
              onclick={() => pick(ic.id)}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">{@html ic.body}</svg>
            </button>
          {/each}
        </div>
      {:else}
        <p class="empty">{catalog.lucide ? 'No icons match.' : 'Loading icons…'}</p>
      {/each}
    {:else if tab === 'emoji'}
      {#if pasted}
        <div class="sec">Pasted</div>
        <div class="grid">
          <button class="ic emo" aria-pressed={look.icon === emojiId(pasted)} aria-label={`Use ${pasted}`} title="Use this emoji" onclick={() => pick(emojiId(pasted))}>{pasted}</button>
        </div>
      {/if}
      {#each emojiPage.shown as g (g.key)}
        <div class="sec">{g.title}</div>
        <div class="grid">
          {#each g.items as e (e.text)}
            <button class="ic emo" aria-pressed={look.icon === emojiId(e.text)} aria-label={e.name} title={e.name} onclick={() => pick(emojiId(e.text))}>{e.text}</button>
          {/each}
        </div>
      {:else}
        {#if !pasted}<p class="empty">{catalog.emoji ? 'No emoji match.' : 'Loading emoji…'}</p>{/if}
      {/each}
    {:else}
      {#if catalog.own?.length}
        <div class="grid">
          {#each catalog.own as p (p.id)}
            <div class="cell">
              <button class="ic" aria-pressed={look.icon === `own:${p.id}`} aria-label="Your picture" title="Your picture" onclick={() => pick(`own:${p.id}`)}>
                <img src={p.url} alt="" width="20" height="20" draggable="false" />
              </button>
              <button class="rm" aria-label="Remove this picture" title="Remove this picture" onclick={() => removePicture(p.id)}>×</button>
            </div>
          {/each}
        </div>
      {:else}
        <p class="empty">{catalog.own ? 'Use a logo or any picture as the icon of a folder.' : 'Loading…'}</p>
      {/if}
      <button class="btn" onclick={addPictures}>Add pictures…</button>
      <p class="hint">SVG, PNG, JPEG, GIF or WebP, up to 1 MB each. Wren keeps a copy in its own data folder, so moving the original is fine.</p>
    {/if}
    {#if more > 0}
      <button class="btn more" onclick={() => (limit += PAGE * 2)}>Show more ({fmt(more)} left)</button>
    {/if}
  </div>

  {#if catalog.error}<p class="err" role="alert">{catalog.error}</p>{/if}

  {#if tab === 'icons'}
    <div class="pack">
      {#if catalog.tabler}
        <span>Tabler icons {catalog.tablerVersion} downloaded ({fmt(catalog.tabler.length)}).</span>
        <button class="link" onclick={removeTabler}>Remove</button>
      {:else if catalog.tabler === null}
        <span>Get {fmt(5509)} more from Tabler (about 4.5 MB, downloaded once from cdn.jsdelivr.net).</span>
        <button class="btn" disabled={catalog.downloading} onclick={downloadTabler}>{catalog.downloading ? 'Downloading…' : 'Download'}</button>
      {/if}
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
  {/if}
  <p class="hint">
    {look.auto ? (suggested ? 'Picked by Wren from the project files. Choose one to change it.' : 'Choose an icon for this folder.') : 'Saved on this computer only.'}
    {#if tab === 'icons'}<span class="credit">Lucide (ISC){catalog.tabler ? ' · Tabler (MIT)' : ''}</span>{/if}
  </p>
</div>

<style>
  .pop {
    position: fixed;
    z-index: 60;
    width: 344px;
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
  .link { border: 0; background: none; color: var(--accent); font: inherit; font-size: 12px; cursor: pointer; padding: 2px 4px; flex-shrink: 0; }
  .link:hover { text-decoration: underline; }
  .tabs { display: flex; gap: 2px; padding: 2px; border-radius: 8px; background: var(--surface-2); }
  .tabs button {
    flex: 1; border: 0; background: none; color: var(--muted); font: inherit; font-size: 12px;
    padding: 4px 6px; border-radius: 6px; cursor: pointer;
  }
  .tabs button[aria-selected='true'] { background: var(--surface); color: var(--text); box-shadow: 0 0 0 1px var(--border); }
  .find { display: flex; gap: 6px; }
  .find input, .find select {
    min-width: 0; font: inherit; font-size: 12.5px; color: var(--text); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: 7px; padding: 5px 8px;
  }
  .find input { flex: 1; }
  .find select { max-width: 116px; }
  .find input:focus-visible, .find select:focus-visible { outline: 2px solid var(--accent); outline-offset: -1px; }
  .scroll { height: 236px; overflow-y: auto; overscroll-behavior: contain; display: flex; flex-direction: column; gap: 4px; padding-right: 2px; }
  .sec { font-size: 11px; font-weight: 600; color: var(--muted); padding: 4px 2px 0; }
  .grid { display: grid; grid-template-columns: repeat(8, 1fr); gap: 3px; }
  .ic {
    display: grid; place-items: center; height: 34px; padding: 0;
    border: 1px solid transparent; border-radius: 8px; background: none; color: var(--text); cursor: pointer;
  }
  .ic:hover { background: var(--surface-2); }
  .ic[aria-pressed='true'] { border-color: var(--accent); background: var(--surface-2); }
  .ic img { object-fit: contain; border-radius: 3px; }
  .emo { font-size: 19px; line-height: 1; font-family: 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', sans-serif; }
  .cell { position: relative; }
  .cell .ic { width: 100%; }
  .rm {
    position: absolute; top: -4px; right: -4px; width: 16px; height: 16px; padding: 0; border-radius: 50%;
    border: 1px solid var(--border); background: var(--surface); color: var(--muted); font-size: 11px; line-height: 1;
    cursor: pointer; opacity: 0;
  }
  .cell:hover .rm, .rm:focus-visible { opacity: 1; }
  .rm:hover { color: var(--text); }
  .btn {
    align-self: flex-start; font: inherit; font-size: 12px; color: var(--text); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: 7px; padding: 4px 10px; cursor: pointer; flex-shrink: 0;
  }
  .btn:hover:not(:disabled) { border-color: var(--accent); }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .more { align-self: center; margin: 4px 0; }
  .empty { margin: 8px 2px; font-size: 12px; color: var(--muted); }
  .pack { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 11.5px; color: var(--muted); line-height: 1.35; }
  .pack:empty { display: none; }
  .err { margin: 0; font-size: 11.5px; color: var(--status-critical, #d03b3b); line-height: 1.35; }
  .colors { display: flex; gap: 4px; justify-content: space-between; }
  .sw {
    width: 20px; height: 20px; border-radius: 50%; padding: 0; cursor: pointer;
    border: 2px solid var(--surface); background: var(--sw, var(--text)); box-shadow: 0 0 0 1px var(--border);
  }
  .sw.plain { background: linear-gradient(135deg, var(--text) 50%, var(--muted) 50%); }
  .sw[aria-pressed='true'] { box-shadow: 0 0 0 2px var(--accent); }
  .hint { margin: 0; font-size: 11.5px; color: var(--muted); line-height: 1.4; }
  .credit { display: block; font-size: 10.5px; opacity: 0.8; margin-top: 2px; }
</style>
