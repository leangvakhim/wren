<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import LoginSheet from './LoginSheet.svelte';
  import LimitBars from './LimitBars.svelte';
  import { planLimits, refreshLimits, sourceText } from './planLimits.svelte';
  import { accounts, accountsFor, checkAccount, loadAccounts, STATUS_LABEL } from './accounts.svelte';
  import { checkGithub, github, installHint, loadGithub } from './github.svelte';
  import { PROVIDERS, type Account, type GithubAccount, type Provider } from './types';

  let { open = $bindable(false), installed }: { open: boolean; installed: Partial<Record<Provider, string | null>> } = $props();

  let dlg: HTMLDialogElement | undefined = $state();
  let signingIn = $state<Account | GithubAccount | null>(null);
  let newGhName = $state('');
  const gh = installHint();
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

  /** Adds the CLI's login that already exists on this computer (no new sign-in). */
  async function addExisting(p: Provider) {
    error = null;
    try {
      const a = await invoke<Account>('add_account', { provider: p, name: newName[p].trim() || 'This computer', existing: true });
      newName[p] = '';
      await loadAccounts();
      if (p !== 'antigravity' && installed[p]) await check(a);
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

  // ---- GitHub (through gh) ----

  async function addGithub(existing: boolean) {
    error = null;
    try {
      const name = existing ? newGhName.trim() || 'This computer' : newGhName;
      const a = await invoke<GithubAccount>('add_github', { name, existing });
      newGhName = '';
      await loadGithub();
      if (existing) await checkGh(a);
      else signingIn = a; // start sign-in right away; Cancel keeps it for later
    } catch (e) {
      error = String(e);
    }
  }

  async function checkGh(a: GithubAccount) {
    busyId = a.id;
    error = null;
    try {
      await checkGithub(a.id);
    } catch (e) {
      error = String(e);
    }
    busyId = null;
  }

  async function removeGh(a: GithubAccount) {
    busyId = a.id;
    error = null;
    try {
      await invoke('remove_github', { id: a.id });
      await loadGithub();
    } catch (e) {
      error = String(e);
    }
    busyId = null;
  }

  function ghStatus(a: GithubAccount): { text: string; tone: string } {
    if (a.status === 'signed_in') return { text: a.login ? `Signed in as @${a.login}` : 'Signed in', tone: 'ok' };
    if (a.status === 'unknown') return { text: 'Not checked yet', tone: 'muted' };
    return { text: a.status === 'pending' ? 'Not signed in yet' : 'Signed out', tone: 'warn' };
  }

  async function saveRename(a: Account | GithubAccount) {
    error = null;
    try {
      const isAI = 'provider' in a;
      await invoke(isAI ? 'rename_account' : 'rename_github', { id: a.id, name: renameText });
      renaming = null;
      await (isAI ? loadAccounts() : loadGithub());
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
    <h2 id="acc-title">{signingIn ? `Sign in · ${'provider' in signingIn ? PROVIDERS[signingIn.provider].label : 'GitHub'} · ${signingIn.name}` : agyUsage ? 'Antigravity quota' : 'Accounts'}</h2>
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
        {#if !accountsFor(p).length}
          <p class="note">No {PROVIDERS[p].label} accounts in Wren yet.</p>
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
                  {#if a.is_default}
                    <button class="ghost danger" disabled={busyId === a.id} onclick={() => remove(a)} title={`Takes it off Wren's list. Your ${PROVIDERS[p].bin} login on this computer stays as it is.`}>Remove</button>
                  {:else}
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
          {#if !accountsFor(p).some((a) => a.is_default)}
            <button
              class="ghost"
              type="button"
              disabled={installed[p] === null}
              onclick={() => addExisting(p)}
              title={`Adds the ${PROVIDERS[p].bin} login you already use in Terminal on this computer (no new sign-in)`}
            >Use this computer's login</button>
          {/if}
        </form>
      </section>
    {/each}

    <section>
      <h3>GitHub{github.gh === null ? ' (GitHub CLI not installed)' : ''}</h3>
      <p class="note">
        Pick a GitHub account per pane (menu in the pane's header). The AI's <code>gh</code> commands and <code>git push</code> /
        <code>pull</code> in that pane then use it. Sign-in happens on github.com through the GitHub CLI; Wren never sees your token,
        and your git settings on this computer aren't changed.
      </p>
      {#if github.gh === null}
        <p class="note warnText">
          Install the GitHub CLI first:
          {#if gh.command}<code>{gh.command}</code> in Terminal, or download it from{:else}download it from{/if}
          <code>{gh.url}</code>. Then reopen Wren.
        </p>
      {/if}
      {#if !github.list.length}
        <p class="note">No GitHub accounts in Wren yet.</p>
      {/if}
      <ul>
        {#each github.list as a (a.id)}
          {@const st = ghStatus(a)}
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
              <span class="sub">{st.text}{a.is_default ? " · this computer's gh login" : ''}</span>
            </div>
            <div class="btns">
              {#if renaming === a.id}
                <button class="ghost" onclick={() => saveRename(a)}>Save</button>
                <button class="ghost" onclick={() => (renaming = null)}>Cancel</button>
              {:else}
                <button class="ghost" disabled={!github.gh} onclick={() => (signingIn = a)}>Sign in</button>
                <button class="ghost" disabled={busyId === a.id || !github.gh} onclick={() => checkGh(a)}>Check</button>
                <button class="ghost" onclick={() => { renaming = a.id; renameText = a.name; }}>Rename</button>
                <button
                  class="ghost danger"
                  disabled={busyId === a.id}
                  onclick={() => removeGh(a)}
                  title={a.is_default ? "Takes it off Wren's list. Your gh login on this computer stays as it is." : 'Signs this account out of gh and removes it'}
                >Remove</button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
      <form class="add" onsubmit={(e) => { e.preventDefault(); addGithub(false); }}>
        <label class="sr" for="add-github">New GitHub account name</label>
        <input id="add-github" placeholder="New account name, e.g. Work" bind:value={newGhName} maxlength="40" />
        <button class="ghost" type="submit" disabled={!newGhName.trim() || !github.gh}>Add account</button>
        {#if !github.list.some((a) => a.is_default)}
          <button class="ghost" type="button" disabled={!github.gh} onclick={() => addGithub(true)} title="Adds the gh login you already use in Terminal on this computer (no new sign-in)">Use this computer's login</button>
        {/if}
      </form>
    </section>
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
