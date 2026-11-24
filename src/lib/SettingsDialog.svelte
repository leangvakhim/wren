<script lang="ts">
  import { saveSettings, settings, type OnLimit } from './settings.svelte';
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
  .note { color: var(--muted); font-size: 12px; margin: 12px 0 0; line-height: 1.5; }
</style>
