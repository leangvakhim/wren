<script lang="ts">
  import { isIOS, link, pushSupported, setPush, standalone, unpair } from './link.svelte';

  let { onback }: { onback: () => void } = $props();

  const KINDS = [
    { key: 'approval', label: 'Needs your approval' },
    { key: 'question', label: 'Asks you a question' },
    { key: 'done', label: 'Task done' },
    { key: 'problems', label: 'Problems (errors, out of usage)' },
  ];
  let kinds = $state<string[]>(link.pushKinds.length ? [...link.pushKinds] : KINDS.map((k) => k.key));
  let on = $state(link.pushKinds.length > 0);
  let saving = $state(false);
  let error = $state('');
  let saved = $state(false);
  let confirmForget = $state(false);

  async function save(enable: boolean) {
    saving = true;
    error = '';
    saved = false;
    try {
      await setPush(enable ? kinds : []);
      on = enable;
      saved = true;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      saving = false;
    }
  }

  const needsHomeScreen = $derived(isIOS() && !standalone());
</script>

<header class="top chat">
  <button class="ghost icon" onclick={onback} aria-label="Back">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </button>
  <div class="who"><b>Settings</b><small>{link.host}{link.version ? ` · Wren ${link.version}` : ''}</small></div>
</header>

<div class="pad">
  <h3>Notifications on this phone</h3>
  {#if needsHomeScreen}
    <p class="tip">On iPhone, notifications only work in the Home Screen app. Tap Share → <b>Add to Home Screen</b>, open Wren from there, and pair it (Wren → Phone → Copy link, then paste it).</p>
  {:else if !pushSupported()}
    <p class="muted">This browser can't receive notifications from a web page.</p>
  {:else}
    <p class="muted small">Sent by your computer when a pane needs you, while this page isn't open.</p>
    {#each KINDS as k}
      <label class="check">
        <input type="checkbox" value={k.key} bind:group={kinds} />
        {k.label}
      </label>
    {/each}
    <div class="btns">
      <button class="primary" disabled={saving || !kinds.length} onclick={() => save(true)}>{on ? 'Save' : 'Turn on'}</button>
      {#if on}<button class="ghost" disabled={saving} onclick={() => save(false)}>Turn off</button>{/if}
    </div>
    {#if saved}<p class="ok small">✓ Saved</p>{/if}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}

  <h3>This phone</h3>
  <p class="muted small">Paired with {link.host || 'your computer'}. Messages are end-to-end encrypted; the relay can't read them.</p>
  {#if confirmForget}
    <p>Unpair this phone? You'll need a new pairing code from Wren. (Also remove it in Wren → Phone.)</p>
    <div class="btns">
      <button class="danger solid" onclick={unpair}>Unpair</button>
      <button class="ghost" onclick={() => (confirmForget = false)}>Keep</button>
    </div>
  {:else}
    <button class="ghost" onclick={() => (confirmForget = true)}>Unpair this phone…</button>
  {/if}
</div>
