<script lang="ts">
  import FolderIcon from './FolderIcon.svelte';
  import FolderIconPicker from './FolderIconPicker.svelte';
  import { deleteConversation, history, renameConversation } from './history.svelte';
  import { PROVIDERS, type ConversationMeta } from './types';

  let {
    activeConvId,
    openConvIds,
    onopen,
    onnew,
  }: {
    /** Conversation shown in the active pane. */
    activeConvId: string | null;
    /** Conversations shown in any pane. */
    openConvIds: (string | null)[];
    onopen: (id: string) => void;
    onnew: () => void;
  } = $props();

  const FOLD_KEY = 'wren.sidebar.folded.v1';
  let folded = $state<Record<string, boolean>>(JSON.parse(localStorage.getItem(FOLD_KEY) ?? '{}'));
  let renaming = $state<string | null>(null);
  let renameText = $state('');
  let confirmDelete = $state<string | null>(null);
  /** The folder whose icon picker is open. */
  let picking = $state<string | null>(null);

  $effect(() => {
    try {
      localStorage.setItem(FOLD_KEY, JSON.stringify(folded));
    } catch {}
  });

  /** Conversations grouped by project folder, newest folder first. */
  const groups = $derived.by(() => {
    const map = new Map<string, ConversationMeta[]>();
    for (const m of history.list) {
      const key = m.cwd || '(no folder)';
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(m);
    }
    return [...map.entries()].map(([cwd, items]) => ({
      cwd,
      name: cwd.split(/[\\/]/).filter(Boolean).pop() || cwd,
      items: items.sort((a, b) => b.updatedAt - a.updatedAt),
    }));
  });

  function ago(t: number) {
    const s = Math.max(0, (Date.now() - t) / 1000);
    if (s < 60) return 'now';
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    if (s < 86400 * 7) return `${Math.floor(s / 86400)}d`;
    return new Date(t).toLocaleDateString();
  }

  async function saveRename(id: string) {
    if (renameText.trim()) await renameConversation(id, renameText).catch(() => {});
    renaming = null;
  }
</script>

<nav class="side" aria-label="Conversations">
  <div class="top">
    <span class="title">Conversations</span>
    <button class="ghost" onclick={onnew} title="New conversation in the active pane">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      New
    </button>
  </div>

  {#if groups.length === 0}
    <p class="empty">Your conversations will appear here, grouped by folder. They're saved only on this computer.</p>
  {/if}

  <ul class="folders">
    {#each groups as g (g.cwd)}
      <li>
        <div class="folder">
          {#if g.cwd !== '(no folder)'}
            <button class="ficon" aria-label={`Change the icon of ${g.name}`} title="Change icon" aria-expanded={picking === g.cwd} onclick={() => (picking = picking === g.cwd ? null : g.cwd)}>
              <FolderIcon cwd={g.cwd} />
            </button>
          {:else}
            <span class="ficon" aria-hidden="true"><FolderIcon cwd="" /></span>
          {/if}
          <button class="fold" aria-expanded={!folded[g.cwd]} title={g.cwd} onclick={() => (folded[g.cwd] = !folded[g.cwd])}>
            <span class="fname">{g.name}</span>
            <span class="count">{g.items.length}</span>
            <svg class="caret" class:shut={folded[g.cwd]} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
          </button>
          {#if picking === g.cwd}
            <FolderIconPicker cwd={g.cwd} name={g.name} onclose={() => (picking = null)} />
          {/if}
        </div>
        {#if !folded[g.cwd]}
          <ul class="convs">
            {#each g.items as c (c.id)}
              <li class="conv" class:active={c.id === activeConvId} class:inpane={openConvIds.includes(c.id)}>
                {#if renaming === c.id}
                  <label class="sr" for={`ren-${c.id}`}>Conversation title</label>
                  <!-- svelte-ignore a11y_autofocus -->
                  <input
                    id={`ren-${c.id}`}
                    bind:value={renameText}
                    autofocus
                    onkeydown={(e) => { if (e.key === 'Enter') saveRename(c.id); if (e.key === 'Escape') renaming = null; }}
                    onblur={() => saveRename(c.id)}
                  />
                {:else if confirmDelete === c.id}
                  <span class="confirm">Delete this conversation?</span>
                  <button class="mini danger" onclick={async () => { await deleteConversation(c.id); confirmDelete = null; }}>Delete</button>
                  <button class="mini" onclick={() => (confirmDelete = null)}>Keep</button>
                {:else}
                  <button class="open" onclick={() => onopen(c.id)} title={c.title}>
                    <span class="ctitle">{c.title || 'Untitled'}</span>
                    <span class="meta">{PROVIDERS[c.provider]?.label ?? c.provider} · {ago(c.updatedAt)}</span>
                  </button>
                  <span class="actions">
                    <button class="icon" aria-label="Rename" title="Rename" onclick={() => { renaming = c.id; renameText = c.title; }}>
                      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></svg>
                    </button>
                    <button class="icon" aria-label="Delete" title="Delete" onclick={() => (confirmDelete = c.id)}>
                      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" /></svg>
                    </button>
                  </span>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    {/each}
  </ul>
</nav>

<style>
  .side {
    width: 264px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    background: var(--surface-2);
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 8px;
    gap: 6px;
  }
  .top { display: flex; align-items: center; justify-content: space-between; padding: 0 4px 4px; }
  .title { font-weight: 650; font-size: 13px; }
  .empty { color: var(--muted); font-size: 12px; padding: 4px 6px; margin: 0; line-height: 1.5; }
  ul { list-style: none; margin: 0; padding: 0; }
  .folder { position: relative; display: flex; align-items: center; gap: 2px; border-radius: 8px; }
  .folder:hover { background: var(--surface); }
  .ficon {
    display: grid; place-items: center; width: 28px; height: 28px; flex-shrink: 0;
    border: 0; border-radius: 6px; background: transparent; color: var(--text); cursor: pointer;
  }
  .ficon:hover, .ficon[aria-expanded='true'] { background: var(--surface-2); }
  .fold {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: transparent;
    padding: 6px;
    min-height: 32px;
    cursor: pointer;
    font-weight: 600;
    font-size: 13px;
    text-align: left;
  }
  .caret { transition: transform 0.15s; flex-shrink: 0; }
  .caret.shut { transform: rotate(-90deg); }
  .fname { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count { color: var(--muted); font-weight: 500; font-size: 11px; }
  .convs { padding-left: 14px; margin-bottom: 4px; }
  .conv { display: flex; align-items: center; gap: 2px; border-radius: 8px; padding-right: 4px; }
  .conv:hover, .conv.active { background: var(--surface); }
  .conv.active { box-shadow: inset 3px 0 0 var(--accent-fill); }
  .open {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    border: 0;
    background: transparent;
    padding: 6px 8px;
    min-height: 40px;
    cursor: pointer;
    text-align: left;
  }
  .ctitle { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  .meta { font-size: 11px; color: var(--muted); }
  .conv.inpane .meta::after { content: ' · open'; color: var(--accent); }
  .actions { display: none; gap: 2px; }
  .conv:hover .actions, .conv:focus-within .actions { display: flex; }
  .icon { border: 0; background: transparent; padding: 6px; border-radius: 6px; cursor: pointer; color: var(--muted); }
  .icon:hover { color: var(--text); background: var(--surface-2); }
  .confirm { font-size: 12px; padding: 6px; flex: 1; }
  .mini { font-size: 12px; border: 1px solid var(--border); background: var(--surface); border-radius: 6px; padding: 3px 8px; cursor: pointer; }
  .danger { color: var(--danger); }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    font-size: 13px;
    margin: 4px;
    padding: 5px 8px;
    border: 1px solid var(--accent);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
  }
</style>
