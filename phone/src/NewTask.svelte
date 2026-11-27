<script lang="ts">
  import type { Mode, Permission, Provider } from '../../src/lib/types';
  import type { Uploaded } from '../../src/lib/remoteTypes';
  import { link, request, upload } from './link.svelte';
  import Setup from './Setup.svelte';
  import { fmtSize } from './util';

  let { onback, onstarted }: { onback: () => void; onstarted: (slot: number) => void } = $props();

  const firstAI = (): Provider => {
    const inst = link.app?.installed;
    return (['claude', 'codex', 'antigravity'] as Provider[]).find((p) => inst?.[p]) ?? 'claude';
  };
  const accs = (p: Provider) => (link.app?.accounts ?? []).filter((a) => a.provider === p);
  const startAI = firstAI();
  let provider = $state<Provider>(startAI);
  let accountId = $state((accs(startAI).find((a) => a.usable) ?? accs(startAI)[0])?.id ?? '');
  let model = $state('');
  let effort = $state('');
  let permission = $state<Permission>('manual');
  let mode = $state<Mode | null>(null);
  let cwd = $state(link.app?.folders[0] ?? link.app?.defaultFolder ?? '');
  let text = $state('');
  let files = $state<{ name: string; size: number; progress: number; done: Uploaded | null; error: string }[]>([]);
  let starting = $state(false);
  let error = $state('');

  function pickFiles(e: Event) {
    const list = [...((e.target as HTMLInputElement).files ?? [])];
    (e.target as HTMLInputElement).value = '';
    for (const f of list.slice(0, 10 - files.length)) {
      files.push({ name: f.name || 'photo.jpg', size: f.size, progress: 0, done: null, error: '' });
      const item = files[files.length - 1];
      upload(f, (p) => (item.progress = p))
        .then((u) => (item.done = u))
        .catch((err) => (item.error = (err as Error).message));
    }
  }
  const uploading = $derived(files.some((f) => !f.done && !f.error));

  async function start() {
    if (!cwd.trim()) return (error = 'Choose a project folder.');
    if (!text.trim() && !files.some((f) => f.done)) return (error = 'Write what to do.');
    starting = true;
    error = '';
    try {
      const r = await request<{ slot: number }>(
        {
          op: 'new_task',
          config: { provider, accountId, permission, model, effort, mode, cwd: cwd.trim() },
          text: text.trim(),
          files: files.filter((f) => f.done).map((f) => f.done!),
        },
        120_000,
      );
      onstarted(r.slot);
    } catch (e) {
      error = (e as Error).message;
    } finally {
      starting = false;
    }
  }
</script>

<header class="top chat">
  <button class="ghost icon" onclick={onback} aria-label="Back">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
  </button>
  <div class="who"><b>New task</b><small>Opens a pane on {link.host || 'the computer'}</small></div>
</header>

<div class="pad">
  <label class="field">
    <span>What should it do?</span>
    <textarea bind:value={text} rows="5" placeholder="e.g. Fix the failing login test and explain the cause"></textarea>
  </label>
  <div class="atts">
    {#each files as f, i (i)}
      <span class="fchip" class:bad={!!f.error} title={f.error || f.name}>
        <span class="fname">{f.name}</span>
        <small>{f.error ? 'failed' : f.done ? fmtSize(f.size) : `${Math.round(f.progress * 100)}%`}</small>
        <button class="x" aria-label={`Remove ${f.name}`} onclick={() => files.splice(i, 1)}>×</button>
      </span>
    {/each}
    <label class="ghost addfile">
      <input type="file" multiple onchange={pickFiles} />
      + Photos or files
    </label>
  </div>

  <Setup bind:provider bind:accountId bind:model bind:effort bind:permission bind:mode bind:cwd />

  {#if error}<p class="err" role="alert">{error}</p>{/if}
  <button class="primary big" disabled={starting || uploading || link.stage !== 'ready'} onclick={start}>{starting ? 'Starting…' : 'Start'}</button>
</div>
