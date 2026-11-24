<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { onMount, onDestroy, untrack } from 'svelte';
  import { Terminal } from '@xterm/xterm';
  import { FitAddon } from '@xterm/addon-fit';
  import '@xterm/xterm/css/xterm.css';
  import { checkAccount, loadAccounts } from './accounts.svelte';
  import type { Account, LoginEvent } from './types';

  let {
    account,
    onclose,
    purpose = 'signin',
  }: { account: Account; onclose: () => void; purpose?: 'signin' | 'settings' | 'usage' } = $props();

  // The sheet is re-created for each account (see {#key} in AccountsDialog).
  const loginId = untrack(() => `login-${account.id}`);
  let termEl: HTMLDivElement | undefined = $state();
  let url = $state<string | null>(null);
  let copied = $state(false);
  let running = $state(false);
  let deviceCode = $state(false);
  let result = $state<string | null>(null);
  let error = $state<string | null>(null);

  let term: Terminal | null = null;
  let fit: FitAddon | null = null;
  let ro: ResizeObserver | null = null;

  const p = untrack(() => account.provider);
  const settingsOnly = untrack(() => purpose !== 'signin');
  const usageOnly = untrack(() => purpose === 'usage');

  function cssVar(name: string) {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }

  function decode(b64: string): Uint8Array {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  async function start() {
    error = null;
    result = null;
    url = null;
    term?.reset();
    const ch = new Channel<LoginEvent>();
    ch.onmessage = async (ev) => {
      if (ev.kind === 'output') term?.write(decode(ev.data));
      else if (ev.kind === 'url') url = ev.url;
      else if (ev.kind === 'exited') {
        running = false;
        await verify();
      }
    };
    try {
      fit?.fit();
      await invoke('start_login', {
        loginId,
        accountId: account.id,
        deviceCode,
        cols: term?.cols ?? 80,
        rows: term?.rows ?? 12,
        onEvent: ch,
      });
      running = true;
      term?.focus();
    } catch (e) {
      error = String(e);
    }
  }

  async function verify() {
    if (p === 'antigravity' || settingsOnly) return;
    try {
      const a = await checkAccount(account.id);
      result = a.status === 'signed_in' ? `Signed in${a.email ? ' as ' + a.email : ''}.` : 'Not signed in yet.';
    } catch (e) {
      error = String(e);
    }
  }

  async function copy() {
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
    } catch {
      const input = document.getElementById(`${loginId}-url`) as HTMLInputElement | null;
      input?.select();
      document.execCommand('copy');
    }
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  async function done() {
    await invoke('cancel_login', { loginId }).catch(() => {});
    if (settingsOnly) {
      onclose();
      return;
    }
    if (p === 'antigravity') {
      await invoke('set_active_antigravity', { id: account.id }).catch((e) => (error = String(e)));
    } else {
      await verify();
    }
    await loadAccounts();
    onclose();
  }

  async function cancel() {
    await invoke('cancel_login', { loginId }).catch(() => {});
    onclose();
  }

  async function restartWithDeviceCode() {
    await invoke('cancel_login', { loginId }).catch(() => {});
    await start();
  }

  onMount(() => {
    term = new Terminal({
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      fontSize: 12,
      rows: 12,
      cursorBlink: true,
      convertEol: false,
      theme: { background: cssVar('--surface-2'), foreground: cssVar('--text'), cursor: cssVar('--accent') },
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.open(termEl!);
    fit.fit();
    term.onData((d) => invoke('login_input', { loginId, data: d }).catch(() => {}));
    ro = new ResizeObserver(() => {
      fit?.fit();
      if (term) invoke('login_resize', { loginId, cols: term.cols, rows: term.rows }).catch(() => {});
    });
    ro.observe(termEl!);
    start();
  });

  onDestroy(() => {
    ro?.disconnect();
    invoke('cancel_login', { loginId }).catch(() => {});
    term?.dispose();
  });
</script>

<div class="sheet">

  <ol class="steps">
    {#if usageOnly}
      <li>When the agy prompt appears below, type <code>/usage</code> and press Enter (or click <b>Type /usage</b>).</li>
      <li>It shows Antigravity's own quota for the signed-in Google account. Click <b>Done</b> when you're finished.</li>
    {:else if settingsOnly}
      <li>In the terminal below, type <code>/config</code> and press Enter.</li>
      <li>Go to <b>Account</b> and turn <b>Enable Telemetry</b> off.</li>
      <li>Close the settings in agy, then click <b>Done</b>.</li>
    {:else if p === 'claude'}
      <li>Copy the link and open it in the browser where you're signed in to the Claude account you want.</li>
      <li>Approve access. If the page shows a code, paste it in the terminal below and press Enter.</li>
    {:else if p === 'codex'}
      <li>Copy the link and open it in the browser where you're signed in to the ChatGPT account you want.</li>
      <li>{deviceCode ? 'Enter the one-time code shown in the terminal below.' : 'Approve access. The terminal below finishes by itself.'}</li>
    {:else}
      <li>Antigravity keeps one Google account signed in per computer. If a different one is signed in, type <code>/logout</code> in the terminal below and press Enter, then click <b>Restart</b>.</li>
      <li>Antigravity may open your default browser directly. If a link appears, copy it into the browser with the Google account you want.</li>
      <li>When you see the agy prompt, you're signed in. Click <b>Done</b>.</li>
    {/if}
  </ol>

  {#if p === 'codex' && !settingsOnly}
    <label class="check">
      <input type="checkbox" bind:checked={deviceCode} onchange={restartWithDeviceCode} />
      Use a one-time code (works in any browser, even on another device)
    </label>
  {/if}

  <div class="link" hidden={settingsOnly}>
    {#if url}
      <label class="sr" for={`${loginId}-url`}>Sign-in link</label>
      <input id={`${loginId}-url`} readonly value={url} onfocus={(e) => (e.currentTarget as HTMLInputElement).select()} />
      <button class="primary" onclick={copy}>{copied ? 'Copied' : 'Copy link'}</button>
    {:else}
      <span class="muted">{running ? 'Waiting for the sign-in link…' : 'Starting…'}</span>
    {/if}
  </div>

  <div class="term" bind:this={termEl}></div>

  {#if error}<p class="err" role="alert">{error}</p>{/if}
  {#if result}<p class="ok" aria-live="polite">{result}</p>{/if}

  <div class="actions">
    <button class="ghost" onclick={cancel}>Cancel</button>
    <button class="ghost" onclick={restartWithDeviceCode}>Restart</button>
    {#if usageOnly}
      <button class="ghost" disabled={!running} onclick={() => { invoke('login_input', { loginId, data: '/usage\r' }).catch(() => {}); term?.focus(); }}>Type /usage</button>
    {/if}
    <span class="spacer"></span>
    <button class="primary" onclick={done}>Done</button>
  </div>
</div>

<style>
  .sheet { display: flex; flex-direction: column; gap: 10px; }
  .steps { margin: 0; padding-left: 20px; color: var(--muted); font-size: 13px; line-height: 1.5; }
  .check { font-size: 13px; display: flex; gap: 8px; align-items: center; }
  .link { display: flex; gap: 8px; align-items: center; min-height: 36px; }
  .link input {
    flex: 1;
    min-width: 0;
    font-family: var(--mono);
    font-size: 12px;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg);
    color: var(--text);
  }
  .term { height: 220px; border: 1px solid var(--border); border-radius: 8px; padding: 6px; background: var(--surface-2); }
  .actions { display: flex; gap: 8px; align-items: center; }
  .spacer { flex: 1; }
  .muted { color: var(--muted); font-size: 13px; }
  .err { color: var(--danger); margin: 0; font-size: 13px; }
  .ok { color: var(--ok); margin: 0; font-size: 13px; font-weight: 600; }
</style>
