<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import LoginSheet from './LoginSheet.svelte';
  import { accounts, findAccount } from './accounts.svelte';
  import { settings } from './settings.svelte';
  import type { Account, Provider } from './types';

  let { open = $bindable(false), installed }: { open: boolean; installed: Partial<Record<Provider, string | null>> } = $props();

  let dlg: HTMLDialogElement | undefined = $state();
  let dataFolder = $state('');
  let agySettings = $state<Account | null>(null);

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) {
      dlg.showModal();
      invoke<string>('data_folder').then((d) => (dataFolder = d)).catch(() => {});
    }
    if (!open && dlg.open) dlg.close();
  });

  function openAgySettings() {
    agySettings = findAccount(accounts.activeAgy ?? '') ?? findAccount('default-agy') ?? null;
  }
</script>

<dialog bind:this={dlg} onclose={() => { open = false; agySettings = null; }} aria-labelledby="priv-title">
  <div class="head">
    <h2 id="priv-title">{agySettings ? 'Antigravity settings' : 'Privacy'}</h2>
    <button class="ghost" onclick={() => (agySettings ? (agySettings = null) : (open = false))}>{agySettings ? 'Back' : 'Close'}</button>
  </div>

  {#if agySettings}
    <LoginSheet account={agySettings} purpose="settings" onclose={() => (agySettings = null)} />
  {:else}
    <section>
      <h3>What stays on your computer</h3>
      <ul>
        <li>Wren has no server and sends nothing to its own makers. If you turn on the phone link, messages to your phone go through the relay you set up, end-to-end encrypted (the relay can't read them).</li>
        <li>Folder icons come with Wren. Only if you click <i>Download</i> for the extra Tabler icons does Wren fetch them once from cdn.jsdelivr.net (a plain download: no account, nothing about your work).</li>
        <li>Your conversations, settings, accounts list and usage numbers are saved only here:
          <code>{dataFolder || 'Wren app data folder'}</code></li>
        <li>Logins are kept by each AI's own CLI on this computer. Wren never reads them.</li>
        <li>Your prompts and files you ask about still go to the AI company you chose, because that's where the model runs.</li>
      </ul>
    </section>

    <section>
      <h3>Turned off automatically by Wren</h3>
      <ul>
        <li><b>Claude Code:</b> telemetry, error reporting and feedback surveys.</li>
        <li>
          <b>Codex:</b> client analytics and feedback uploads{#if settings.codexPrivate}, and its own chat history: Codex
            chats are kept only in Wren, not in Codex or the Codex app (Settings → Privacy){/if}.
        </li>
        <li>
          <b>Antigravity:</b> needs a one-time switch inside agy. Open its settings, type <code>/config</code>, go to
          <b>Account</b> and turn <b>Enable Telemetry</b> off. Google says this is also the setting that stops your
          interactions being used to improve models.
          <div><button class="ghost" disabled={installed.antigravity === null} onclick={openAgySettings}>Open Antigravity settings</button></div>
        </li>
      </ul>
    </section>

    <section>
      <h3>Model training: set this on each AI's website (once per account)</h3>
      <ul>
        <li><b>Claude</b> (claude.ai): Settings → Privacy → turn off <i>Help improve Claude</i>.</li>
        <li><b>ChatGPT / Codex</b> (chatgpt.com): Settings → Data controls → turn off <i>Improve the model for everyone</i>.</li>
        <li><b>Google / Antigravity</b>: the <i>Enable Telemetry</i> switch above.</li>
      </ul>
      <p class="note">Do this for every account you add. Menu names can change; look for the data or privacy controls.</p>
    </section>
  {/if}
</dialog>

<style>
  dialog {
    width: min(680px, calc(100vw - 40px));
    max-height: calc(100vh - 40px);
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    padding: 18px 20px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.45); }
  .head { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 18px; }
  section { border-top: 1px solid var(--border); padding: 10px 0; margin-top: 8px; }
  h3 { margin: 0 0 6px; font-size: 14px; }
  ul { margin: 0; padding-left: 18px; display: flex; flex-direction: column; gap: 6px; font-size: 13px; line-height: 1.5; }
  li div { margin-top: 6px; }
  code { font-size: 12px; background: var(--surface-2); padding: 1px 5px; border-radius: 4px; word-break: break-all; }
  .note { color: var(--muted); font-size: 12px; margin: 8px 0 0; }
</style>
