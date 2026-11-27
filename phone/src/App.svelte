<script lang="ts">
  import { onMount } from 'svelte';
  import Logo from '../../src/lib/Logo.svelte';
  import { PROVIDERS } from '../../src/lib/types';
  import { boot, deviceName, isIOS, link, pair, reconnectNow, request, sendVisible, standalone, unpair, usePairLink, watch } from './link.svelte';
  import Chat from './Chat.svelte';
  import NewTask from './NewTask.svelte';
  import Settings from './Settings.svelte';
  import { ago, folderOf, paneState } from './util';

  type View = { kind: 'home' } | { kind: 'pane'; slot: number } | { kind: 'new' } | { kind: 'chats' } | { kind: 'settings' };
  let view = $state<View>({ kind: 'home' });
  let name = $state(deviceName());
  let pasted = $state('');
  let pasteError = $state('');
  let busy = $state(false);
  let error = $state('');

  function go(v: View) {
    view = v;
    watch(v.kind === 'pane' ? v.slot : null);
    error = '';
    window.scrollTo(0, 0);
  }

  onMount(() => {
    boot();
    const vis = () => {
      sendVisible();
      if (document.visibilityState === 'visible') reconnectNow();
    };
    document.addEventListener('visibilitychange', vis);
    window.addEventListener('online', reconnectNow);
    // A tapped notification opens its pane.
    const fromSw = (e: MessageEvent) => {
      const slot = Number(e.data?.open);
      if (e.data?.open !== undefined && e.data.open !== '' && !Number.isNaN(slot)) go({ kind: 'pane', slot });
    };
    navigator.serviceWorker?.addEventListener('message', fromSw);
    navigator.serviceWorker?.register('./sw.js').catch(() => {});
    const m = /#pane=(\d)/.exec(location.hash);
    if (m) go({ kind: 'pane', slot: Number(m[1]) });
    return () => {
      document.removeEventListener('visibilitychange', vis);
      window.removeEventListener('online', reconnectNow);
      navigator.serviceWorker?.removeEventListener('message', fromSw);
    };
  });

  // A pane closed on the computer: back home.
  $effect(() => {
    if (view.kind === 'pane' && link.app && !link.app.panes.some((p) => view.kind === 'pane' && p.slot === view.slot)) go({ kind: 'home' });
  });

  async function openChat(convId: string) {
    busy = true;
    error = '';
    try {
      const r = await request<{ slot: number }>({ op: 'open_chat', convId });
      go({ kind: 'pane', slot: r.slot });
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  function tryPaste() {
    pasteError = usePairLink(pasted.trim()) ? '' : "That isn't a Wren pairing link. Copy it from Wren → Phone → Copy link.";
  }

  const panes = $derived([...(link.app?.panes ?? [])].sort((a, b) => a.pos - b.pos));
  const mustAddFirst = $derived(isIOS() && !standalone());
</script>

<div class="app">
  {#if link.stage === 'loading'}
    <div class="center"><Logo size={56} color="var(--accent-fill)" hole="var(--bg)" /></div>
  {:else if link.stage === 'unpaired' || (link.stage === 'denied' && !link.everReady)}
    <div class="center intro">
      <Logo size={56} color="var(--accent-fill)" hole="var(--bg)" />
      <h1>wren</h1>
      {#if link.stage === 'denied'}
        <p class="err">
          {link.why === 'declined' ? 'Pairing was refused on the computer.' : link.why === 'removed' || link.why === 'unknown' ? 'This phone was removed in Wren.' : 'That pairing code was already used or has expired.'}
        </p>
      {/if}
      <p>To use Wren from this phone, open Wren on your computer → <b>Phone</b> → <b>Show pairing code</b>, and scan the code with the camera.</p>
      <details class="paste">
        <summary>Or paste the pairing link</summary>
        <p class="muted">Use this in an app added to the Home Screen: copy the link with “Copy link” in Wren and paste it here.</p>
        <input bind:value={pasted} placeholder="https://…/#p=…" autocomplete="off" autocapitalize="off" spellcheck="false" />
        <button class="primary" onclick={tryPaste} disabled={!pasted.trim()}>Continue</button>
        {#if pasteError}<p class="err">{pasteError}</p>{/if}
      </details>
    </div>
  {:else if link.stage === 'offer'}
    <div class="center intro">
      <Logo size={56} color="var(--accent-fill)" hole="var(--bg)" />
      <h1>Pair with Wren</h1>
      {#if mustAddFirst}
        <div class="tip">
          <b>Want notifications?</b> Add Wren to your Home Screen <i>before</i> pairing: tap Share → <b>Add to Home Screen</b>, then open Wren from the Home Screen. (An iPhone keeps Home Screen apps separate from Safari.)
        </div>
      {/if}
      <label class="field">
        <span>Name of this phone (shown in Wren)</span>
        <input bind:value={name} maxlength="60" />
      </label>
      <button class="primary big" onclick={() => pair(name)}>Pair{mustAddFirst ? ' in Safari' : ''}</button>
      <p class="muted">Messages between this phone and your computer are end-to-end encrypted.</p>
    </div>
  {:else if link.stage === 'waiting'}
    <div class="center intro">
      <Logo size={56} color="var(--accent-fill)" hole="var(--bg)" />
      <h1>Allow on your computer</h1>
      <p>Wren is asking on your computer whether to allow <b>{name}</b>. Click <b>Allow</b> there.</p>
    </div>
  {:else if link.stage === 'connecting' && !link.everReady}
    <div class="center intro">
      <Logo size={56} color="var(--accent-fill)" hole="var(--bg)" />
      <p>Connecting to Wren…</p>
    </div>
  {:else}
    {#if link.stage !== 'ready'}
      <div class="offline" role="status">
        {#if link.stage === 'denied'}This phone is no longer allowed. <button class="linklike" onclick={unpair}>Pair again</button>
        {:else}{link.why === 'Wren is offline' ? `Wren isn't running on ${link.host || 'the computer'}.` : 'Reconnecting to Wren…'} <button class="linklike" onclick={reconnectNow}>Try now</button>{/if}
      </div>
    {/if}

    {#if view.kind === 'pane'}
      <Chat slot={view.slot} onback={() => go({ kind: 'home' })} />
    {:else if view.kind === 'new'}
      <NewTask onback={() => go({ kind: 'home' })} onstarted={(slot) => go({ kind: 'pane', slot })} />
    {:else if view.kind === 'settings'}
      <Settings onback={() => go({ kind: 'home' })} />
    {:else}
      <header class="top">
        <Logo size={26} color="var(--accent-fill)" hole="var(--bg)" />
        <div class="who">
          <b>wren</b>
          <small><span class="dot" class:on={link.stage === 'ready'}></span>{link.host || 'Your computer'}</small>
        </div>
        <button class="ghost icon" onclick={() => go({ kind: 'settings' })} aria-label="Settings">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></svg>
        </button>
      </header>

      <div class="tabs" role="tablist">
        <button role="tab" aria-selected={view.kind === 'home'} class:on={view.kind === 'home'} onclick={() => go({ kind: 'home' })}>Open now</button>
        <button role="tab" aria-selected={view.kind === 'chats'} class:on={view.kind === 'chats'} onclick={() => go({ kind: 'chats' })}>Saved chats</button>
      </div>

      {#if error}<p class="err pad" role="alert">{error}</p>{/if}

      {#if view.kind === 'home'}
        <ul class="list">
          {#each panes as p (p.slot)}
            {@const st = paneState(p)}
            <li>
              <button class="card" onclick={() => go({ kind: 'pane', slot: p.slot })}>
                <span class="row1">
                  <b class="ai" style={`--c: var(--series-${p.provider})`}>{PROVIDERS[p.provider].label}</b>
                  <span class={`state ${st.tone}`}>{st.text}</span>
                </span>
                <span class="title">{p.title || 'New conversation'}</span>
                <span class="sub">{folderOf(p.cwd) || 'No folder'}{p.accountName ? ` · ${p.accountName}` : ''}</span>
              </button>
            </li>
          {:else}
            <li class="muted pad">Nothing open on the computer yet.</li>
          {/each}
        </ul>
        <div class="pad">
          <button class="primary big" disabled={link.stage !== 'ready' || !link.app?.canAdd} onclick={() => go({ kind: 'new' })}>+ New task</button>
          {#if link.app && !link.app.canAdd}<p class="muted small">All 4 panes are in use on the computer. Open one and start a new chat there, or close a pane.</p>{/if}
        </div>
      {:else}
        <ul class="list">
          {#each link.app?.history ?? [] as h (h.id)}
            <li>
              <button class="card" disabled={busy} onclick={() => openChat(h.id)}>
                <span class="row1">
                  <b class="ai" style={`--c: var(--series-${h.provider})`}>{PROVIDERS[h.provider].label}</b>
                  <span class="muted small">{ago(h.updatedAt)}</span>
                </span>
                <span class="title">{h.title || 'Untitled'}</span>
                <span class="sub">{folderOf(h.cwd)}</span>
              </button>
            </li>
          {:else}
            <li class="muted pad">No saved chats yet.</li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/if}
</div>
