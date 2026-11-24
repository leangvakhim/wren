<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import LoginSheet from './LoginSheet.svelte';
  import LimitBars from './LimitBars.svelte';
  import { planLimits, refreshLimits, sourceText } from './planLimits.svelte';
  import { accounts, accountsFor, checkAccount, loadAccounts, STATUS_LABEL } from './accounts.svelte';
  import { PROVIDERS, type Account, type Provider } from './types';

  let { open = $bindable(false), installed }: { open: boolean; installed: Partial<Record<Provider, string | null>> } = $props();

  let dlg: HTMLDialogElement | undefined = $state();
  let signingIn = $state<Account | null>(null);
  /** Antigravity: its own /usage screen in the terminal sheet. */
  let agyUsage = $state<Account | null>(null);
  let now = $state(Date.now());
  let newName = $state<Record<Provider, string>>({ claude: '', codex: '', antigravity: '' });
  let renaming = $state<string | null>(null);
  let renameText = $state('');
  let busyId = $state<string | null>(null);
  let error = $state<string | null>(null);

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) {
      dlg.showModal();
      refreshSignedIn();
    }
    if (!open && dlg.open) dlg.close();
  });

  // keep "resets in …" current while open
  $effect(() => {
    if (!open) return;
    const t = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(t);
  });

  /** Numbers older than 2 minutes are read again from each CLI. */
  function refreshSignedIn() {
    for (const a of accounts.list) {
      if (a.provider === 'antigravity' || installed[a.provider] === null) continue;
      if (a.status === 'signed_in' || (a.is_default && a.status === 'unknown')) refreshLimits(a.id, a.provider, 120_000);
    }
  }

  function openAgyUsage() {
    agyUsage = accounts.list.find((x) => x.id === accounts.activeAgy) ?? accounts.list.find((x) => x.id === 'default-agy') ?? null;
  }

  const providers = Object.keys(PROVIDERS) as Provider[];

  async function add(p: Provider) {
    error = null;
    try {
      const a = await invoke<Account>('add_account', { provider: p, name: newName[p] });
      newName[p] = '';
      await loadAccounts();
      signingIn = a; // start sign-in right away; Cancel keeps it for later
    } catch (e) {
      error = String(e);
    }
  }

  async function check(a: Account) {
    busyId = a.id;
    error = null;
    try {
      await checkAccount(a.id);
    } catch (e) {
      error = String(e);
    }
    busyId = null;
  }

  async function remove(a: Account) {
    busyId = a.id;
    error = null;
    try {
      await invoke('remove_account', { id: a.id });
      await loadAccounts();
    } catch (e) {
      error = String(e);
    }
    busyId = null;
  }

  async function saveRename(a: Account) {
    error = null;
    try {
      await invoke('rename_account', { id: a.id, name: renameText });
      renaming = null;
      await loadAccounts();
    } catch (e) {
      error = String(e);
    }
  }

  function statusOf(a: Account): { text: string; tone: string } {
    if (a.provider === 'antigravity') {
      if (a.is_default) return { text: 'Uses the current agy login', tone: 'muted' };
      return accounts.activeAgy === a.id ? { text: 'Active (signed in)', tone: 'ok' } : { text: 'Not active', tone: 'muted' };
    }
    const tone = a.status === 'signed_in' ? 'ok' : a.status === 'unknown' ? 'muted' : 'warn';
    return { text: STATUS_LABEL[a.status], tone };
  }
</script>

<dialog bind:this={dlg} onclose={() => (open = false)} aria-labelledby="acc-title">
  <div class="head">
    <h2 id="acc-title">{signingIn ? `Sign in · ${PROVIDERS[signingIn.provider].label} · ${signingIn.name}` : agyUsage ? 'Antigravity quota' : 'Accounts'}</h2>
    <button class="ghost" onclick={() => (signingIn ? (signingIn = null) : agyUsage ? (agyUsage = null) : (open = false))}>{signingIn || agyUsage ? 'Back' : 'Close'}</button>
  </div>

  {#if agyUsage}
    {#key agyUsage.id}
      <LoginSheet account={agyUsage} purpose="usage" onclose={() => (agyUsage = null)} />
    {/key}
  {:else if signingIn}
    {#key signingIn.id}
      <LoginSheet account={signingIn} onclose={() => (signingIn = null)} />
    {/key}
  {:else}
    <p class="intro">
      Add as many accounts per AI as you like. Each one signs in on the AI's own page; Wren never sees your password or
      tokens. Pick the account per conversation in its pane.
    </p>
    {#if error}<p class="err" role="alert">{error}</p>{/if}

    {#each providers as p}
      <section>
        <h3>{PROVIDERS[p].label}{installed[p] === null ? ' (not installed)' : ''}</h3>
        {#if p === 'antigravity'}
          <p class="note">Antigravity keeps one Google account signed in per computer, so only one account is active at a time.</p>
          <p class="note">
            Its 5-hour and weekly quota can only be seen in agy's own <code>/usage</code> screen.
            <button class="link" disabled={installed[p] === null} onclick={openAgyUsage}>Open /usage</button>.
            Wren's own count per day is in the Usage window.
          </p>
        {/if}
        <ul>
          {#each accountsFor(p) as a (a.id)}
            {@const st = statusOf(a)}
            <li>
              <span class={`dot ${st.tone}`} aria-hidden="true"></span>
              <div class="who">
                {#if renaming === a.id}
                  <label class="sr" for={`rn-${a.id}`}>New name</label>
                  <!-- svelte-ignore a11y_autofocus -->
                  <input id={`rn-${a.id}`} bind:value={renameText} maxlength="40" autofocus onkeydown={(e) => { if (e.key === 'Enter') saveRename(a); if (e.key === 'Escape') { e.preventDefault(); renaming = null; } }} />
                {:else}
                  <span class="name">{a.name}</span>
                {/if}
                <span class="sub">{a.email ? a.email + ' · ' : ''}{st.text}</span>
              </div>
              <div class="btns">
                {#if renaming === a.id}
                  <button class="ghost" onclick={() => saveRename(a)}>Save</button>
                  <button class="ghost" onclick={() => (renaming = null)}>Cancel</button>
                {:else}
                  <button class="ghost" disabled={installed[p] === null} onclick={() => (signingIn = a)}>Sign in</button>
                  {#if p !== 'antigravity'}
                    <button class="ghost" disabled={busyId === a.id || installed[p] === null} onclick={() => check(a)}>Check</button>
                  {/if}
                  <button class="ghost" onclick={() => { renaming = a.id; renameText = a.name; }}>Rename</button>
                  {#if !a.is_default}
                    <button class="ghost danger" disabled={busyId === a.id} onclick={() => remove(a)} title="Signs this account out and removes it">Remove</button>
                  {/if}
                {/if}
              </div>
              {#if p !== 'antigravity'}
                {@const lim = planLimits.byAccount[a.id]}
                <div class="limits">
                  {#if lim && lim.windows.length}
                    <LimitBars windows={lim.windows} provider={p} {now} />
                  {/if}
                  <div class="limmeta">
                    {#if planLimits.busy[a.id]}
                      <span>Reading plan limits…</span>
                    {:else if lim?.error && lim.windows.length}
                      <span>{lim.plan ? `${lim.plan[0].toUpperCase()}${lim.plan.slice(1)} plan · ` : ''}{sourceText(lim, now)}.</span>
                      <span class="warnText">Latest read failed: {lim.error}</span>
                    {:else if lim?.error}
                      <span class="warnText">{lim.error}</span>
                    {:else if lim}
                      <span>{lim.plan ? `${lim.plan[0].toUpperCase()}${lim.plan.slice(1)} plan · ` : ''}{lim.from === 'reply' ? sourceText(lim, now) : `from ${PROVIDERS[p].label}, ${sourceText(lim, now)}`}</span>
                    {:else}
                      <span>Plan limits not read yet.</span>
                    {/if}
                    <button class="link" disabled={planLimits.busy[a.id] || installed[p] === null} onclick={() => refreshLimits(a.id, p)}>Refresh</button>
                  </div>
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        <form class="add" onsubmit={(e) => { e.preventDefault(); add(p); }}>
          <label class="sr" for={`add-${p}`}>New {PROVIDERS[p].label} account name</label>
          <input id={`add-${p}`} placeholder="New account name, e.g. Work" bind:value={newName[p]} maxlength="40" />
          <button class="ghost" type="submit" disabled={!newName[p].trim()}>Add account</button>
        </form>
      </section>
    {/each}
  {/if}
</dialog>

<style>
  dialog {
    width: min(720px, calc(100vw - 40px));
    max-height: calc(100vh - 40px);
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    padding: 18px 20px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.45); }
  .head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 6px; }
  h2 { margin: 0; font-size: 18px; }
  .intro { color: var(--muted); font-size: 13px; margin: 0 0 8px; }
  section { border-top: 1px solid var(--border); padding: 12px 0; }
  h3 { margin: 0 0 6px; font-size: 14px; }
  .note { margin: 0 0 6px; font-size: 12px; color: var(--muted); }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  li { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 8px; background: var(--surface-2); flex-wrap: wrap; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--muted); flex-shrink: 0; }
  .dot.ok { background: var(--ok); }
  .dot.warn { background: var(--accent-fill); }
  .who { display: flex; flex-direction: column; flex: 1 1 200px; min-width: 0; }
  .name { font-weight: 600; }
  .sub { font-size: 12px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .btns { display: flex; gap: 6px; flex-wrap: wrap; }
  .add { display: flex; gap: 8px; margin-top: 8px; }
  input {
    flex: 1;
    min-width: 0;
    font: inherit;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
  }
  .danger { color: var(--danger); }
  .limits { flex-basis: 100%; padding: 4px 0 2px 19px; display: flex; flex-direction: column; gap: 6px; }
  .limmeta { display: flex; gap: 8px; align-items: baseline; font-size: 12px; color: var(--muted); flex-wrap: wrap; }
  .warnText { color: var(--text); }
  .link { background: none; border: 0; padding: 0; color: var(--accent); font: inherit; font-size: 12px; cursor: pointer; text-decoration: underline; }
  .link:disabled { color: var(--muted); cursor: default; }
  .err { color: var(--danger); font-size: 13px; }
</style>
