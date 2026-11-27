<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { saveSettings, settings, type OnLimit } from './settings.svelte';
  import { checkForUpdate, updater } from './updater.svelte';
  import { notifyState, testNotification } from './notify.svelte';
  import { PROVIDERS, type Provider } from './types';

  let { open = $bindable(false), installed }: { open: boolean; installed: Partial<Record<Provider, string | null>> } = $props();
  let dlg: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) dlg.showModal();
    if (!open && dlg.open) dlg.close();
  });

  const OPTIONS: { value: OnLimit; label: string; hint: string }[] = [
    {
      value: 'switch',
      label: 'Continue in another AI',
      hint: 'The pane switches by itself to the next AI below and carries a recap of the conversation.',
    },
    {
      value: 'wait',
      label: 'Wait for the reset, then continue',
      hint: 'Wren reads the reset time, waits, then re-sends your last message on the same account. Keep Wren open.',
    },
    { value: 'off', label: 'Do nothing', hint: 'Show a message and let me choose.' },
  ];

  const NOTIFY_KINDS = [
    { key: 'approval', label: 'Needs your approval', hint: 'Allow / Deny, a plan ready to start, an action Antigravity refused' },
    { key: 'question', label: 'Asks you a question', hint: 'A question form is waiting for your choices' },
    { key: 'done', label: 'Task done', hint: 'With the first line of the answer' },
    { key: 'problems', label: 'Problems', hint: 'A task stopped with an error or ran out of usage' },
  ] as const;

  const SYSTEM_SETTINGS = navigator.userAgent.includes('Mac')
    ? 'System Settings → Notifications → Wren'
    : navigator.userAgent.includes('Windows')
      ? 'Settings → System → Notifications → Wren'
      : "your desktop's notification settings";

  let testState = $state<'idle' | 'sent' | 'failed'>('idle');
  async function sendTest() {
    testState = (await testNotification()) ? 'sent' : 'failed';
  }

  // ---- Reset Wren ----
  let confirmReset = $state(false);
  let resetting = $state(false);
  let resetError = $state<string | null>(null);

  async function resetWren() {
    resetting = true;
    resetError = null;
    try {
      // Wren's own saved settings in this window (layout, theme, limits, ...).
      localStorage.clear();
      sessionStorage.clear();
      await invoke('reset_wren'); // deletes Wren's data and restarts Wren
    } catch (e) {
      resetError = String(e);
      resetting = false;
    }
  }

  function move(i: number, d: number) {
    const o = settings.fallbackOrder;
    const j = i + d;
    if (j < 0 || j >= o.length) return;
    [o[i], o[j]] = [o[j], o[i]];
    saveSettings();
  }
</script>

<dialog bind:this={dlg} onclose={() => (open = false)} aria-labelledby="set-title">
  <div class="head">
    <h2 id="set-title">Settings</h2>
    <button class="ghost" onclick={() => (open = false)}>Close</button>
  </div>

  <section>
    <h3>When an account runs out of usage</h3>
    <div class="radios" role="radiogroup" aria-label="When an account runs out of usage">
      {#each OPTIONS as o}
        <label class="radio" class:on={settings.onLimit === o.value}>
          <input type="radio" name="onlimit" value={o.value} bind:group={settings.onLimit} onchange={saveSettings} />
          <span><b>{o.label}</b><br /><small>{o.hint}</small></span>
        </label>
      {/each}
    </div>

    {#if settings.onLimit === 'switch'}
      <h4>Order to try</h4>
      <ol class="order">
        {#each settings.fallbackOrder as p, i (p)}
          <li>
            <span>{PROVIDERS[p].label}{installed[p] === null ? ' (not installed, skipped)' : ''}</span>
            <button class="ghost" aria-label={`Move ${PROVIDERS[p].label} up`} disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
            <button class="ghost" aria-label={`Move ${PROVIDERS[p].label} down`} disabled={i === settings.fallbackOrder.length - 1} onclick={() => move(i, 1)}>↓</button>
          </li>
        {/each}
      </ol>
      <label class="check">
        <input type="checkbox" bind:checked={settings.waitIfNoOther} onchange={saveSettings} />
        If no other AI is available, wait for the reset and continue
      </label>
    {/if}

    <p class="note">
      Wren never switches between accounts of the <i>same</i> AI by itself, because the AI companies treat that as
      getting around their limits. You can still pick another account manually in the pane.
    </p>
  </section>

  <section>
    <h3>Notifications</h3>
    <label class="check first">
      <input type="checkbox" bind:checked={settings.notify.enabled} onchange={saveSettings} />
      <b>Allow notifications</b>
    </label>
    <div class="kinds" class:off={!settings.notify.enabled}>
      {#each NOTIFY_KINDS as k}
        <label class="check">
          <input type="checkbox" bind:checked={settings.notify[k.key]} disabled={!settings.notify.enabled} onchange={saveSettings} />
          <span>{k.label} <small>· {k.hint}</small></span>
        </label>
      {/each}
      <label class="check">
        <input type="checkbox" bind:checked={settings.notify.sound} disabled={!settings.notify.enabled} onchange={saveSettings} />
        Play a sound
      </label>
    </div>
    <div class="row center gap">
      <button class="ghost" disabled={!settings.notify.enabled} onclick={sendTest}>Send a test notification</button>
      <span class="ver" role="status">
        {#if testState === 'sent'}Sent. If nothing appeared, check {SYSTEM_SETTINGS}.{/if}
      </span>
    </div>
    {#if notifyState.blocked}
      <p class="err" role="alert">Notifications are blocked for Wren. To turn them back on, open {SYSTEM_SETTINGS}.</p>
    {:else if notifyState.error}
      <p class="err" role="alert">Couldn't show a notification: {notifyState.error}</p>
    {/if}
    <p class="note">
      Shown only while Wren isn't the window in front. Coming back to Wren makes the pane that notified you the active
      one. This setting is saved on this computer only, so everyone on your team chooses their own.
    </p>
  </section>

  <section>
    <h3>Updates</h3>
    <div class="row center">
      <span class="ver">Wren {updater.current || '…'}</span>
      <button class="ghost" disabled={updater.phase === 'checking' || updater.phase === 'downloading' || updater.phase === 'installing'} onclick={() => checkForUpdate(false)}>
        {updater.phase === 'checking' ? 'Checking…' : 'Check for updates'}
      </button>
      <span class="ver" role="status">
        {#if updater.available}Version {updater.available.version} is ready: use "Update available" at the top.
        {:else if updater.phase === 'latest'}You have the latest version.
        {:else if updater.phase === 'error'}Couldn't check: {updater.error}{/if}
      </span>
    </div>
    <p class="note">Wren checks for a new version when it starts and every 6 hours.</p>
  </section>

  <section>
    <h3>Reset Wren</h3>
    <p class="note first">
      Start over as a new user on this computer: deletes Wren's accounts list, conversations, attachments and settings,
      then restarts Wren.
    </p>
    {#if !confirmReset}
      <button class="ghost danger" onclick={() => (confirmReset = true)}>Reset Wren…</button>
    {:else}
      <div class="confirm" role="alertdialog" aria-label="Reset Wren?">
        <p><b>Delete everything Wren keeps on this computer?</b> This can't be undone.</p>
        <ul>
          <li>Deleted: all accounts in Wren, all conversations, attachments, usage history and settings.</li>
          <li>Accounts you added in Wren are signed out first.</li>
          <li>Kept: the <code>claude</code>, <code>codex</code> and <code>agy</code> logins you use in Terminal, and your project files (including research reports).</li>
        </ul>
        {#if resetError}<p class="err" role="alert">{resetError}</p>{/if}
        <div class="row">
          <button class="danger solid" disabled={resetting} onclick={resetWren}>{resetting ? 'Resetting…' : 'Delete everything and restart'}</button>
          <button class="ghost" disabled={resetting} onclick={() => (confirmReset = false)}>Cancel</button>
        </div>
      </div>
    {/if}
  </section>
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 40px));
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    padding: 18px 20px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.45); }
  .head { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 18px; }
  section { border-top: 1px solid var(--border); margin-top: 10px; padding-top: 10px; }
  h3 { margin: 0 0 8px; font-size: 14px; }
  h4 { margin: 12px 0 6px; font-size: 13px; }
  .radios { display: flex; flex-direction: column; gap: 6px; }
  .radio { display: flex; gap: 10px; align-items: flex-start; padding: 8px 10px; border: 1px solid var(--border); border-radius: 10px; cursor: pointer; }
  .radio.on { border-color: var(--accent); background: var(--surface-2); }
  .radio small { color: var(--muted); }
  .radio input { margin-top: 3px; }
  .order { margin: 0; padding-left: 22px; display: flex; flex-direction: column; gap: 4px; }
  .order li span { display: inline-block; min-width: 230px; }
  .check { display: flex; gap: 8px; align-items: center; font-size: 13px; margin-top: 10px; }
  .check.first { margin-top: 0; }
  .check small { color: var(--muted); }
  .kinds { padding-left: 22px; }
  .kinds .check { margin-top: 6px; }
  .kinds.off { opacity: 0.55; }
  .gap { margin-top: 10px; }
  .note { color: var(--muted); font-size: 12px; margin: 12px 0 0; line-height: 1.5; }
  .note.first { margin: 0 0 8px; }
  .danger { color: var(--danger); }
  .danger.solid { background: var(--danger); color: var(--bg); border: 1px solid var(--danger); border-radius: 8px; padding: 6px 12px; font: inherit; cursor: pointer; }
  .danger.solid:disabled { opacity: 0.6; cursor: default; }
  .confirm { border: 1px solid var(--danger); border-radius: 10px; padding: 10px 12px; font-size: 13px; }
  .confirm p { margin: 0 0 6px; }
  .confirm ul { margin: 0 0 10px; padding-left: 18px; color: var(--muted); font-size: 12px; line-height: 1.5; }
  .row { display: flex; gap: 8px; }
  .row.center { align-items: center; flex-wrap: wrap; }
  .ver { font-size: 13px; color: var(--muted); }
  .err { color: var(--danger); font-size: 12px; }
</style>
