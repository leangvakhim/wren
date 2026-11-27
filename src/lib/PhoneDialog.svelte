<script lang="ts">
  import { onDestroy } from 'svelte';
  import { cancelPairing, configureRemote, pairPhone, remote, removePhone, renamePhone } from './remote.svelte';
  import { copyText } from './markdown';

  let { open = $bindable(false) }: { open: boolean } = $props();
  let dlg: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) {
      dlg.showModal();
      loadForm();
    }
    if (!open && dlg.open) dlg.close();
  });

  const view = $derived(remote.view);
  let relay = $state('');
  let key = $state('');
  let keepAwake = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);

  function loadForm() {
    relay = view?.relay ?? '';
    key = '';
    keepAwake = view?.keep_awake ?? true;
    error = null;
  }

  async function save(enabled: boolean) {
    saving = true;
    error = null;
    try {
      await configureRemote(enabled, relay, key.trim() ? key.trim() : null, keepAwake);
      key = '';
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  // ---- QR code ----
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(timer));
  const left = $derived(remote.code ? Math.max(0, Math.round((remote.code.expires_at - now) / 1000)) : 0);
  $effect(() => {
    if (remote.code && left === 0) remote.code = null;
  });
  let copied = $state(false);

  async function pair() {
    error = null;
    try {
      await pairPhone();
    } catch (e) {
      error = String(e);
    }
  }

  async function copyLink() {
    if (remote.code && (await copyText(remote.code.url))) {
      copied = true;
      setTimeout(() => (copied = false), 2000);
    }
  }

  // ---- paired phones ----
  let renaming = $state<string | null>(null);
  let newName = $state('');
  let removing = $state<string | null>(null);

  async function saveName(id: string) {
    try {
      await renamePhone(id, newName);
      renaming = null;
    } catch (e) {
      error = String(e);
    }
  }

  function seen(ms: number): string {
    if (!ms) return 'never connected';
    const d = new Date(ms);
    const sameDay = d.toDateString() === new Date().toDateString();
    return `last seen ${sameDay ? d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }) : d.toLocaleDateString([], { month: 'short', day: 'numeric' })}`;
  }

  const STATUS: Record<string, string> = {
    off: 'Off',
    connecting: 'Connecting to the relay…',
    online: 'On: phones can reach this computer',
    offline: 'Relay not reachable, trying again',
    error: "Can't connect",
  };
</script>

<dialog bind:this={dlg} onclose={() => { open = false; cancelPairing(); }} aria-labelledby="phone-title">
  <div class="head">
    <h2 id="phone-title">Phone</h2>
    <button class="ghost" onclick={() => (open = false)}>Close</button>
  </div>
  <p class="note first">
    Use Wren from your phone: answer approvals and questions, send messages, start tasks and open saved chats.
    Everything between this computer and the phone is end-to-end encrypted; the relay only passes it along and can't read it.
    Keep Wren open on this computer.
  </p>

  {#if view}
    <section>
      <div class="status" role="status">
        <span class="dot {view.status}" aria-hidden="true"></span>
        <b>{STATUS[view.status]}</b>
        {#if view.error}<span class="err">{view.error}</span>{/if}
      </div>

      <label class="field">
        <span>Relay address</span>
        <input type="url" bind:value={relay} placeholder="https://remote.example.com" spellcheck="false" autocomplete="off" />
      </label>
      <label class="field">
        <span>Access key {#if view.has_key}<small>(saved; type a new one to replace it)</small>{/if}</span>
        <input type="password" bind:value={key} placeholder={view.has_key ? '••••••••••••' : 'wren_…'} spellcheck="false" autocomplete="off" />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={keepAwake} />
        <span>Keep this computer awake while the phone link is on <small>(closing a laptop's lid still makes it sleep)</small></span>
      </label>
      <div class="row gap">
        {#if view.enabled}
          <button class="primary" disabled={saving} onclick={() => save(true)}>Save</button>
          <button class="ghost" disabled={saving} onclick={() => save(false)}>Turn off</button>
        {:else}
          <button class="primary" disabled={saving} onclick={() => save(true)}>Turn on</button>
        {/if}
      </div>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <p class="note">
        The relay runs on your own server; only your Wren copies with its access key can use it.
        To make a key, run <code>npm run key</code> in Wren's <code>relay</code> folder.
      </p>
    </section>

    {#if view.enabled}
      <section>
        <h3>Pair a phone</h3>
        {#if remote.code}
          <div class="qr">
            <!-- Made by Wren from its own link (no outside content). -->
            <div class="code" aria-label="QR code to pair a phone">{@html remote.code.svg}</div>
            <div class="steps">
              <ol>
                <li>Open the phone's camera and point it at the code.</li>
                <li>Tap the link, then <b>Pair</b>.</li>
                <li>Allow the phone here when Wren asks.</li>
                <li>iPhone: tap Share → <b>Add to Home Screen</b> to get notifications.</li>
              </ol>
              <p class="note">The code works once, for {Math.floor(left / 60)}:{String(left % 60).padStart(2, '0')} more. Don't share it.</p>
              <div class="row">
                <button class="ghost" onclick={copyLink}>{copied ? '✓ Copied' : 'Copy link'}</button>
                <button class="ghost" onclick={cancelPairing}>Cancel</button>
              </div>
            </div>
          </div>
        {:else}
          <button class="primary" disabled={view.status !== 'online'} onclick={pair}>Show pairing code</button>
          {#if view.status !== 'online'}<p class="note">Available once Wren is connected to the relay.</p>{/if}
        {/if}
      </section>
    {/if}

    {#if view.devices.length}
      <section>
        <h3>Paired phones</h3>
        <ul class="devices">
          {#each view.devices as d (d.id)}
            <li>
              {#if renaming === d.id}
                <input
                  class="rename"
                  bind:value={newName}
                  maxlength="60"
                  aria-label="Phone name"
                  onkeydown={(e) => {
                    if (e.key === 'Enter') saveName(d.id);
                    if (e.key === 'Escape') renaming = null;
                  }}
                />
                <button class="ghost" onclick={() => saveName(d.id)}>Save</button>
                <button class="ghost" onclick={() => (renaming = null)}>Cancel</button>
              {:else}
                <span class="dname">
                  <b>{d.name}</b>
                  <small>{d.connected ? '● connected' : seen(d.last_seen)}{d.push ? ' · notifications on' : ''}</small>
                </span>
                {#if removing === d.id}
                  <span class="confirm">Remove? It can't reach Wren any more.</span>
                  <button class="danger solid" onclick={() => { removePhone(d.id); removing = null; }}>Remove</button>
                  <button class="ghost" onclick={() => (removing = null)}>Keep</button>
                {:else}
                  <button class="ghost" onclick={() => { renaming = d.id; newName = d.name; }}>Rename</button>
                  <button class="ghost" onclick={() => (removing = d.id)}>Remove</button>
                {/if}
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  {/if}
</dialog>

<style>
  dialog {
    width: min(600px, calc(100vw - 40px));
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    padding: 18px 20px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.45); }
  .head { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 18px; }
  h3 { margin: 0 0 8px; font-size: 14px; }
  section { border-top: 1px solid var(--border); margin-top: 10px; padding-top: 10px; }
  .note { color: var(--muted); font-size: 12px; margin: 10px 0 0; line-height: 1.5; }
  .note.first { margin: 8px 0 0; }
  .status { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; margin-bottom: 10px; }
  .dot { width: 9px; height: 9px; border-radius: 50%; background: var(--muted); flex: none; }
  .dot.online { background: var(--ok); }
  .dot.connecting { background: var(--status-warning); }
  .dot.offline, .dot.error { background: var(--status-critical); }
  .field { display: flex; flex-direction: column; gap: 4px; font-size: 13px; margin-top: 8px; }
  .field small { color: var(--muted); font-weight: normal; }
  .field input, .rename {
    font: inherit; color: var(--text); background: var(--surface-2);
    border: 1px solid var(--border); border-radius: 8px; padding: 6px 8px;
  }
  .check { display: flex; gap: 8px; align-items: center; font-size: 13px; margin-top: 10px; }
  .check small { color: var(--muted); }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  .gap { margin-top: 12px; }
  .err { color: var(--danger); font-size: 12px; }
  .qr { display: flex; gap: 16px; align-items: flex-start; flex-wrap: wrap; }
  .code { background: #fff; border-radius: 10px; padding: 6px; line-height: 0; }
  .code :global(svg) { width: 220px; height: 220px; }
  .steps { flex: 1; min-width: 220px; font-size: 13px; }
  .steps ol { margin: 0; padding-left: 18px; line-height: 1.6; }
  .devices { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .devices li { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding: 8px 10px; border: 1px solid var(--border); border-radius: 10px; }
  .dname { flex: 1; display: flex; flex-direction: column; min-width: 160px; }
  .dname small { color: var(--muted); font-size: 12px; }
  .confirm { font-size: 12px; color: var(--danger); }
  .danger.solid { background: var(--danger); color: var(--bg); border: 1px solid var(--danger); border-radius: 8px; padding: 4px 10px; font: inherit; cursor: pointer; }
  code { font-family: var(--mono); font-size: 12px; }
</style>
