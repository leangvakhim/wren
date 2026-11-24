<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Logo from '$lib/Logo.svelte';
  import Pane from '$lib/Pane.svelte';
  import AccountsDialog from '$lib/AccountsDialog.svelte';
  import PrivacyDialog from '$lib/PrivacyDialog.svelte';
  import SettingsDialog from '$lib/SettingsDialog.svelte';
  import UsageDialog from '$lib/UsageDialog.svelte';
  import Sidebar from '$lib/Sidebar.svelte';
  import { loadHistory } from '$lib/history.svelte';
  import { accounts, checkAll, loadAccounts } from '$lib/accounts.svelte';
  import { applyTheme, type Theme } from '$lib/theme';
  import { DEFAULT_CONFIG, defaultAccountId, normalizePermission, type PaneConfig, type Provider } from '$lib/types';

  // Everything Wren remembers lives in this computer's local app storage.
  const STORE = 'wren.state.v1';
  // `order` maps each on-screen position to a pane slot (0-3), so a pane keeps
  // its live session and settings when another pane is closed and it moves over.
  type Saved = { layout: number; theme: Theme; configs: PaneConfig[]; sidebar: boolean; order: number[] };
  const isOrder = (o: unknown): o is number[] =>
    Array.isArray(o) && o.length === 4 && [0, 1, 2, 3].every((n) => o.includes(n));

  function load(): Saved {
    const fallback: Saved = {
      layout: 1,
      theme: 'system',
      sidebar: true,
      order: [0, 1, 2, 3],
      configs: Array.from({ length: 4 }, () => ({ ...DEFAULT_CONFIG })),
    };
    try {
      const s = JSON.parse(localStorage.getItem(STORE) ?? 'null');
      if (s && Array.isArray(s.configs) && s.configs.length === 4) {
        // older saves have no account: use each AI's Default account
        s.configs = s.configs.map((c: PaneConfig) => ({ ...DEFAULT_CONFIG, ...c, permission: normalizePermission(c.permission), accountId: c.accountId || defaultAccountId(c.provider) }));
        if (!isOrder(s.order)) s.order = [0, 1, 2, 3];
        return { ...fallback, ...s };
      }
    } catch {}
    return fallback;
  }

  const saved = load();
  let layout = $state(saved.layout);
  let theme = $state<Theme>(saved.theme);
  let configs = $state<PaneConfig[]>(saved.configs);
  let order = $state<number[]>(saved.order);
  /** On-screen position of the focused pane. */
  let activePane = $state(0);
  let installed = $state<Partial<Record<Provider, string | null>>>({});
  let defaultFolder = $state('');
  let ready = $state(false);
  let showAccounts = $state(false);
  let showPrivacy = $state(false);
  let showSettings = $state(false);
  let showUsage = $state(false);
  let sidebar = $state(saved.sidebar);
  // Pane components by slot (to open a saved conversation in the active one).
  let panes = $state<(Pane | undefined)[]>([]);
  // Which saved conversation each slot is showing.
  let paneConv = $state<(string | null)[]>([null, null, null, null]);

  $effect(() => {
    applyTheme(theme);
  });

  $effect(() => {
    try {
      localStorage.setItem(STORE, JSON.stringify({ layout, theme, sidebar, order: $state.snapshot(order), configs: $state.snapshot(configs) }));
    } catch {}
  });

  $effect(() => {
    if (activePane >= layout) activePane = layout - 1;
  });

  const visible = $derived(order.slice(0, layout));
  const activeSlot = $derived(visible[activePane] ?? visible[0]);

  /** × on a pane: remove it; the panes after it move over (2 columns → 1 pane, etc.). */
  function closePane(pos: number) {
    if (layout <= 1) return;
    const slot = order[pos];
    paneConv[slot] = null;
    // The closed slot goes to the back, so it's the one a bigger layout brings back.
    order = [...order.slice(0, pos), ...order.slice(pos + 1), slot];
    if (activePane > pos) activePane -= 1;
    layout -= 1;
  }

  // A pane whose account was removed falls back to that AI's Default account.
  $effect(() => {
    if (!accounts.loaded) return;
    for (const c of configs) {
      if (!accounts.list.some((a) => a.id === c.accountId && a.provider === c.provider)) {
        c.accountId = defaultAccountId(c.provider);
      }
    }
  });

  onMount(async () => {
    // New panes start in ~/Desktop on macOS, the home folder on Windows/Linux.
    defaultFolder = await invoke<string>('default_folder').catch(() => '');
    for (const c of configs) if (!c.cwd) c.cwd = defaultFolder;
    await loadAccounts().catch(() => {});
    await loadHistory().catch(() => {});
    ready = true;
    installed = await invoke('detect_clis');
    checkAll(installed);
  });

  const THEMES: { value: Theme; label: string }[] = [
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
    { value: 'system', label: 'System' },
  ];
</script>

<div class="app">
  <header class="top">
    <button
      class="ghost iconbtn"
      onclick={() => (sidebar = !sidebar)}
      aria-label={sidebar ? 'Hide sidebar' : 'Show sidebar'}
      aria-expanded={sidebar}
      title={sidebar ? 'Hide sidebar' : 'Show sidebar'}
    >
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" />
        {#if sidebar}<path d="m15 10-2 2 2 2" />{:else}<path d="m13 10 2 2-2 2" />{/if}
      </svg>
    </button>
    <div class="brand">
      <Logo size={28} color="var(--accent-fill)" hole="var(--bg)" />
      <span class="word">wren</span>
    </div>

    <div class="seg" role="group" aria-label="Number of conversations">
      {#each [1, 2, 3, 4] as n}
        <button class:on={layout === n} aria-pressed={layout === n} onclick={() => (layout = n)} title={`${n} conversation${n > 1 ? 's' : ''}`}>
          <svg width="22" height="16" viewBox="0 0 22 16" aria-hidden="true">
            {#if n === 1}<rect x="1" y="1" width="20" height="14" rx="2" />
            {:else if n === 2}<rect x="1" y="1" width="9" height="14" rx="2" /><rect x="12" y="1" width="9" height="14" rx="2" />
            {:else if n === 3}<rect x="1" y="1" width="5.3" height="14" rx="1.5" /><rect x="8.3" y="1" width="5.3" height="14" rx="1.5" /><rect x="15.7" y="1" width="5.3" height="14" rx="1.5" />
            {:else}<rect x="1" y="1" width="9" height="6" rx="1.5" /><rect x="12" y="1" width="9" height="6" rx="1.5" /><rect x="1" y="9" width="9" height="6" rx="1.5" /><rect x="12" y="9" width="9" height="6" rx="1.5" />{/if}
          </svg>
          <span class="sr">{n}</span>
        </button>
      {/each}
    </div>

    <span class="spacer"></span>

    <button class="ghost" onclick={() => (showUsage = true)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20V10M10 20V4M16 20v-7M22 20H2" /></svg>
      Usage
    </button>

    <button class="ghost" onclick={() => (showPrivacy = true)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.3-8 9-4.6-.7-8-4.5-8-9V6z" /></svg>
      Privacy
    </button>

    <button class="ghost" onclick={() => (showAccounts = true)}>
      <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="8" r="4" /><path d="M4 21a8 8 0 0 1 16 0" /></svg>
      Accounts
    </button>

    <button class="ghost iconbtn" onclick={() => (showSettings = true)} aria-label="Settings" title="Settings">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></svg>
    </button>

    <div class="seg" role="group" aria-label="Appearance">
      {#each THEMES as t}
        <button class:on={theme === t.value} aria-pressed={theme === t.value} onclick={() => (theme = t.value)}>{t.label}</button>
      {/each}
    </div>
  </header>

  <div class="body">
  {#if sidebar && ready}
    <Sidebar
      activeConvId={paneConv[activeSlot]}
      openConvIds={visible.map((s) => paneConv[s])}
      onopen={(id) => panes[activeSlot]?.openConversation(id)}
      onnew={() => panes[activeSlot]?.startNew()}
    />
  {/if}
  <main class="grid" data-layout={layout}>
    {#if ready}
    {#each visible as slot, i (slot)}
      <Pane
        paneId={`pane-${slot + 1}`}
        index={i}
        bind:this={panes[slot]}
        bind:config={configs[slot]}
        {installed}
        active={layout > 1 && activePane === i}
        {defaultFolder}
        closable={layout > 1}
        onactivate={() => (activePane = i)}
        onopenaccounts={() => (showAccounts = true)}
        onconv={(id) => (paneConv[slot] = id)}
        onclose={() => closePane(i)}
      />
    {/each}
    {/if}
  </main>
  </div>
</div>

<PrivacyDialog bind:open={showPrivacy} {installed} />
<SettingsDialog bind:open={showSettings} {installed} />
<UsageDialog bind:open={showUsage} />

<AccountsDialog bind:open={showAccounts} {installed} />

<style>
  .app { height: 100vh; display: flex; flex-direction: column; }
  .top {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .brand { display: flex; align-items: center; gap: 8px; }
  .word { font-weight: 650; font-size: 20px; letter-spacing: -0.03em; }
  .spacer { flex: 1; }
  .seg {
    display: flex;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: 9px;
    padding: 2px;
    gap: 2px;
  }
  .seg button {
    border: 0;
    background: transparent;
    color: var(--muted);
    padding: 5px 10px;
    min-height: 30px;
    border-radius: 7px;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    display: grid;
    place-items: center;
  }
  .seg button svg { fill: currentColor; }
  .seg button.on { background: var(--surface); color: var(--text); box-shadow: 0 1px 2px rgb(0 0 0 / 0.12); }
  .body { flex: 1; min-height: 0; display: flex; }
  .iconbtn { padding: 4px 7px; }
  .grid {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: grid;
    gap: 10px;
    padding: 10px;
  }
  .grid[data-layout='1'] { grid-template-columns: minmax(0, 1fr); }
  .grid[data-layout='2'] { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .grid[data-layout='3'] { grid-template-columns: repeat(3, minmax(0, 1fr)); }
  .grid[data-layout='4'] { grid-template-columns: repeat(2, minmax(0, 1fr)); grid-template-rows: repeat(2, minmax(0, 1fr)); }
  @media (max-width: 900px) {
    .grid[data-layout='3'] { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
</style>
