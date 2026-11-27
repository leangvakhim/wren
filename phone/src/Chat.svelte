<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import PeckingWren from '../../src/lib/PeckingWren.svelte';
  import QuestionCard from '../../src/lib/QuestionCard.svelte';
  import { copyText, renderMarkdown } from '../../src/lib/markdown';
  import { BIRD_WORDS, cardLabel, fmtDuration, groupTurns, stepLabel, type Item } from '../../src/lib/chatView';
  import { PROVIDERS } from '../../src/lib/types';
  import type { PhoneMsg, Uploaded } from '../../src/lib/remoteTypes';
  import { link, request, upload } from './link.svelte';
  import PaneSettings from './PaneSettings.svelte';
  import { fmtSize, folderOf } from './util';

  let { slot, onback }: { slot: number; onback: () => void } = $props();

  const ps = $derived(link.panes[slot]);
  const meta = $derived(ps?.meta ?? link.app?.panes.find((p) => p.slot === slot));
  const messages = $derived(ps ? ps.ids.map((id) => ps.byId[id]).filter((m): m is PhoneMsg => !!m) : []);
  const items = $derived(groupTurns(messages, !!meta?.busy));
  const label = $derived(meta ? PROVIDERS[meta.provider].label : '');

  let input = $state('');
  let files = $state<{ name: string; size: number; progress: number; done: Uploaded | null; error: string }[]>([]);
  let error = $state('');
  let sending = $state(false);
  let showSettings = $state(false);
  let menu = $state(false);
  let openTurns = $state<Record<number, boolean>>({});

  // "Working… 0:42"
  let now = $state(Date.now());
  const timer = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => clearInterval(timer));
  const elapsed = $derived.by(() => {
    const s = meta?.startedAt ? Math.max(0, Math.floor((now - meta.startedAt) / 1000)) : 0;
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  });
  const verb = $derived.by(() => {
    if (meta?.waitingFor === 'question') return 'Waiting for your answer';
    if (meta?.waitingFor === 'approval') return 'Waiting for your approval';
    if (meta?.thinking) return 'Thinking';
    const s = meta?.startedAt ? Math.floor((now - meta.startedAt) / 1000) : 0;
    return BIRD_WORDS[Math.floor(s / 3) % BIRD_WORDS.length];
  });

  // Follow new output while you're at the bottom.
  let listEl: HTMLDivElement | undefined = $state();
  let atBottom = true;
  function onScroll() {
    if (!listEl) return;
    atBottom = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 80;
  }
  $effect(() => {
    void messages.length;
    void messages.at(-1)?.text.length;
    if (atBottom) tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
  });

  async function act(r: Parameters<typeof request>[0]) {
    error = '';
    try {
      await request(r);
    } catch (e) {
      error = (e as Error).message;
    }
  }

  async function pickFiles(e: Event) {
    const list = [...((e.target as HTMLInputElement).files ?? [])];
    (e.target as HTMLInputElement).value = '';
    for (const f of list) {
      if (files.length >= 10) break;
      files.push({ name: f.name || 'photo.jpg', size: f.size, progress: 0, done: null, error: '' });
      const item = files[files.length - 1];
      upload(f, (p) => (item.progress = p))
        .then((u) => (item.done = u))
        .catch((err) => (item.error = (err as Error).message));
    }
  }

  const uploading = $derived(files.some((f) => !f.done && !f.error));

  async function send() {
    const text = input.trim();
    const ready = files.filter((f) => f.done).map((f) => f.done!);
    if ((!text && !ready.length) || sending || uploading) return;
    sending = true;
    error = '';
    try {
      await request({ op: 'send', slot, text, files: ready });
      input = '';
      files = [];
      atBottom = true;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      sending = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    // Enter = new line on a phone keyboard; the button sends. Cmd/Ctrl+Enter sends too.
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      send();
    }
  }

  function grow(node: HTMLTextAreaElement) {
    const fit = () => {
      node.style.height = 'auto';
      node.style.height = `${Math.min(node.scrollHeight, 180)}px`;
    };
    node.addEventListener('input', fit);
    $effect(() => {
      void input;
      tick().then(fit);
    });
    return { destroy: () => node.removeEventListener('input', fit) };
  }

  /** The copy button under an answer copies its Markdown text. */
  let copiedAnswer = $state<number | null>(null);
  async function copyAnswer(m: PhoneMsg) {
    if (!(await copyText(m.text.trim()))) return;
    copiedAnswer = m.id;
    setTimeout(() => {
      if (copiedAnswer === m.id) copiedAnswer = null;
    }, 1500);
  }
  /** While the pane works, its last answer may still be streaming. */
  const lastMsg = $derived(messages[messages.length - 1]);

  // Links in answers open in a new tab; copy buttons copy.
  async function onClick(e: MouseEvent) {
    const t = e.target as HTMLElement;
    const btn = t.closest<HTMLButtonElement>('button.copybtn');
    if (btn) {
      const pre = btn.parentElement?.querySelector('pre');
      if (pre && (await copyText(pre.innerText.replace(/\n$/, '')))) {
        btn.textContent = '✓ Copied';
        setTimeout(() => (btn.innerHTML = '⧉'), 1500);
      }
      return;
    }
    const a = t.closest('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href') ?? '';
    if (/^https?:\/\//i.test(href)) window.open(href, '_blank', 'noopener,noreferrer');
  }

  const others = $derived((link.app?.accounts ?? []).filter((a) => meta && a.provider === meta.provider && a.id !== meta.accountId && a.usable));
  let switchTo = $state('');
</script>

{#if !meta}
  <div class="center"><p class="muted">Loading…</p></div>
{:else}
  <header class="top chat">
    <button class="ghost icon" onclick={onback} aria-label="Back">
      <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
    </button>
    <div class="who">
      <b class="ellipsis">{meta.title || 'New conversation'}</b>
      <small class="ellipsis">{label}{meta.modelLabel ? ` · ${meta.modelLabel}` : ''} · {folderOf(meta.cwd) || 'no folder'}</small>
    </div>
    {#if meta.busy}
      <button class="ghost" onclick={() => act({ op: 'stop', slot })}>Stop</button>
    {/if}
    <button class="ghost icon" onclick={() => (menu = !menu)} aria-label="More" aria-expanded={menu}>
      <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="2" /><circle cx="12" cy="12" r="2" /><circle cx="19" cy="12" r="2" /></svg>
    </button>
  </header>
  {#if menu}
    <div class="menu" role="menu">
      <button role="menuitem" onclick={() => { menu = false; showSettings = true; }}>AI, account, model, permission, folder…</button>
      <button role="menuitem" disabled={meta.busy} onclick={() => { menu = false; act({ op: 'new_chat', slot }); }}>New chat in this pane</button>
      {#if (link.app?.panes.length ?? 0) > 1}
        <button role="menuitem" class="danger" onclick={() => { menu = false; act({ op: 'close', slot }); }}>{meta.busy ? 'Stop & close pane' : 'Close pane'}</button>
      {/if}
    </div>
  {/if}

  <button class="chips" onclick={() => (showSettings = true)}>
    <span class="chip" style={`--c: var(--series-${meta.provider})`}>{meta.accountName || 'No account'}</span>
    <span class="chip" class:danger={meta.permission === 'full'}>{({ manual: 'Manual', auto: 'Auto', plan: 'Plan', full: 'Full access' })[meta.permission]}</span>
    {#if meta.mode}<span class="chip">{({ study: 'Study', research: 'Deep research', visualize: 'Visualize' })[meta.mode]}</span>{/if}
    <span class="chip muted">Change…</span>
  </button>

  {#if meta.problem}<p class="banner">{meta.problem}</p>{/if}
  {#if meta.waitingLimit}
    <div class="banner">
      Usage limit reached. Wren continues by itself {meta.waitingLimit.known ? `at ${new Date(meta.waitingLimit.until).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}` : 'later'}.
      <div class="btns">
        <button class="ghost" onclick={() => act({ op: 'limit', slot, action: 'continue' })}>Continue now</button>
        <button class="ghost" onclick={() => act({ op: 'limit', slot, action: 'cancel' })}>Cancel</button>
      </div>
    </div>
  {/if}
  {#if meta.limitHit}
    <div class="banner">
      This account hit its usage limit.
      {#if others.length}
        <div class="btns">
          <select bind:value={switchTo} aria-label="Switch to account">
            <option value="" disabled>Pick an account…</option>
            {#each others as a (a.id)}<option value={a.id}>{a.name}</option>{/each}
          </select>
          <button class="primary" disabled={!switchTo} onclick={() => act({ op: 'limit', slot, action: 'switch', accountId: switchTo })}>Switch and continue</button>
        </div>
      {/if}
      <div class="btns"><button class="ghost" onclick={() => act({ op: 'limit', slot, action: 'dismiss' })}>Dismiss</button></div>
    </div>
  {/if}

  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="messages" bind:this={listEl} onscroll={onScroll} onclick={onClick}>
    {#if !messages.length}
      <p class="muted center-text">Ask {label} anything about {folderOf(meta.cwd) || 'your project'}.</p>
    {/if}
    {#each items as it (it.kind === 'group' ? `g${it.key}` : it.m.id)}
      {@render row(it)}
    {/each}
  </div>

  {#if error}<p class="err pad" role="alert">{error}</p>{/if}

  <footer class="composer">
    {#if files.length}
      <div class="atts">
        {#each files as f, i (i)}
          <span class="fchip" class:bad={!!f.error} title={f.error || f.name}>
            <span class="fname">{f.name}</span>
            <small>{f.error ? 'failed' : f.done ? fmtSize(f.size) : `${Math.round(f.progress * 100)}%`}</small>
            <button class="x" aria-label={`Remove ${f.name}`} onclick={() => files.splice(i, 1)}>×</button>
          </span>
        {/each}
      </div>
    {/if}
    <div class="inrow">
      <label class="attach" aria-label="Attach photos or files">
        <input type="file" multiple onchange={pickFiles} />
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </label>
      <textarea
        rows="1"
        bind:value={input}
        use:grow
        onkeydown={onKey}
        placeholder={meta.busy ? 'Working…' : `Message ${label}`}
        disabled={meta.busy}
        aria-label="Message"
      ></textarea>
      <button class="send" aria-label="Send" disabled={meta.busy || sending || uploading || (!input.trim() && !files.some((f) => f.done))} onclick={send}>
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
      </button>
    </div>
  </footer>

  {#if showSettings}
    <PaneSettings {slot} {meta} onclose={() => (showSettings = false)} />
  {/if}
{/if}

{#snippet row(it: Item<PhoneMsg>)}
  {#if it.kind === 'group'}
    {@const open = openTurns[it.key] ?? it.live}
    <div class="steps" class:live={it.live}>
      <button class="shead" aria-expanded={open} onclick={() => (openTurns[it.key] = !open)}>
        {#if it.live}
          <PeckingWren size={18} hole="var(--bg)" />
          <span>{verb}</span><span class="muted">{elapsed}</span>
        {:else}
          <span>{it.durationMs !== undefined ? `Worked for ${fmtDuration(it.durationMs)}` : `${it.steps.length} step${it.steps.length === 1 ? '' : 's'}`}</span>
        {/if}
        <span class="chev" class:open>›</span>
      </button>
      {#if open && it.steps.length}
        <div class="slist">
          {#each it.steps as m (m.id)}
            {#if m.role === 'tool'}
              {@const sl = stepLabel(m)}
              <details class="srow" class:bad={m.isError}>
                <summary><b>{sl.label}</b> <span class="muted">{sl.subject}</span>{#if m.result === undefined && it.live}<span class="muted"> · running</span>{:else if m.isError}<span class="bad"> · failed</span>{/if}</summary>
                {#if m.detail}<pre>{m.detail}</pre>{/if}
                {#if m.result !== undefined}<pre class="out">{m.result || '(no output)'}</pre>{/if}
              </details>
            {:else if m.role === 'approval' || m.role === 'question'}
              {@const cl = cardLabel(m)}
              <details class="srow" class:bad={cl.bad}>
                <summary><b>{cl.label}</b> <span class="muted">{cl.subject}</span></summary>
                {#if m.role === 'approval'}{@render approval(m)}{:else}{@render question(m)}{/if}
              </details>
            {:else if m.role === 'info'}
              <p class="snote">{m.text}</p>
            {:else}
              <div class="snote md">{@html renderMarkdown(m.text)}</div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {:else}
    {@const m = it.m}
    {#if m.role === 'user'}
      {#if m.files?.length}
        <div class="ufiles">{#each m.files as f, i (i)}<span class="fchip sent">{f.kind === 'image' ? '🖼 ' : ''}{f.name}</span>{/each}</div>
      {/if}
      {#if m.text}<div class="msg user">{m.text}</div>{/if}
    {:else if m.role === 'assistant'}
      <div class="msg assistant md">{@html renderMarkdown(m.text)}</div>
      {#if m.text.trim() && !(meta?.busy && m === lastMsg)}
        <button class="anscopy" onclick={() => copyAnswer(m)} aria-label="Copy response">{copiedAnswer === m.id ? '✓ Copied' : '⧉ Copy'}</button>
      {/if}
    {:else if m.role === 'error'}
      <div class="msg error" role="alert">{m.text}</div>
    {:else if m.role === 'approval'}
      {@render approval(m)}
    {:else if m.role === 'question'}
      {@render question(m)}
    {:else if m.role === 'visual'}
      <div class="msg info">Made a page: <b>{m.text}</b> (open it on the computer)</div>
    {:else if m.role === 'divider'}
      <div class="divider"><span>{m.text}</span></div>
    {:else if m.role === 'notice'}
      <div class="msg notice">{m.text}</div>
    {:else if m.role !== 'tool'}
      <div class="msg info">{m.text}</div>
    {/if}
  {/if}
{/snippet}

{#snippet approval(m: PhoneMsg)}
  <div class="card approval" class:waiting={m.approvalState === 'waiting' || m.approvalState === 'refused'}>
    <p class="ahead">
      {#if m.approvalState === 'refused' || m.approvalState === 'retried' || m.approvalState === 'kept'}<b>{m.tool}</b> refused an action that needs your permission
      {:else if m.tool === 'ExitPlanMode'}<b>Plan ready</b>: {m.text}
      {:else}<b>{m.tool}</b> wants to:{/if}
    </p>
    {#if m.tool !== 'ExitPlanMode'}<pre>{m.text}</pre>{/if}
    {#if m.detail}
      {#if m.tool === 'ExitPlanMode'}<div class="md plan">{@html renderMarkdown(m.detail)}</div>{:else}<p class="muted small">{m.detail}</p>{/if}
    {/if}
    {#if m.approvalState === 'waiting'}
      <div class="btns">
        <button class="primary" onclick={() => act({ op: 'approve', slot, msg: m.id, decision: 'allow' })}>{m.tool === 'ExitPlanMode' ? 'Start' : 'Allow'}</button>
        {#if m.allowSession && m.tool !== 'ExitPlanMode'}
          <button class="ghost" onclick={() => act({ op: 'approve', slot, msg: m.id, decision: 'allow_session' })}>Allow for this chat</button>
        {/if}
        <button class="ghost" onclick={() => act({ op: 'approve', slot, msg: m.id, decision: 'deny' })}>{m.tool === 'ExitPlanMode' ? 'Keep planning' : 'Deny'}</button>
      </div>
    {:else if m.approvalState === 'refused'}
      <div class="btns">
        <button class="primary" disabled={meta?.busy} onclick={() => act({ op: 'approve', slot, msg: m.id, decision: 'retry' })}>Allow &amp; retry</button>
        <button class="ghost" onclick={() => act({ op: 'approve', slot, msg: m.id, decision: 'keep' })}>Keep refused</button>
      </div>
    {:else}
      <p class="muted small">
        {#if m.approvalState === 'allowed'}✓ Allowed{:else if m.approvalState === 'allowed_session'}✓ Allowed for this chat{:else if m.approvalState === 'auto_allowed'}✓ Allowed by {m.autoBy ?? 'the new permission'}{:else if m.approvalState === 'denied'}✗ Denied{:else if m.approvalState === 'retried'}↻ Retried with Auto{:else if m.approvalState === 'kept'}✗ Kept refused{:else}No longer waiting{/if}
      </p>
    {/if}
  </div>
{/snippet}

{#snippet question(m: PhoneMsg)}
  <QuestionCard
    who={m.tool ?? label}
    questions={m.questions ?? []}
    status={m.questionState ?? 'expired'}
    answers={m.answers}
    onsubmit={(a) => act({ op: 'answer', slot, msg: m.id, answers: a })}
  />
{/snippet}
