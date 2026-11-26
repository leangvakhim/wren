<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import Logo from './Logo.svelte';
  import PeckingWren from './PeckingWren.svelte';
  import ModelPicker from './ModelPicker.svelte';
  import ModeMenu from './ModeMenu.svelte';
  import QuestionCard from './QuestionCard.svelte';
  import BrowserPanel from './BrowserPanel.svelte';
  import { applyLimitEvent, dismissedWarnings, levelOf, planLimits, refreshLimits, resetText, usedNow } from './planLimits.svelte';
  import { estimateCost } from './pricing';
  import { getConversation, saveConversation } from './history.svelte';
  import { modelLists, modelName, modelsKey } from './models.svelte';
  import { settings } from './settings.svelte';
  import { buildRecap, formatWait, isLimitMessage, isRecapBubble, parseResetTime } from './limits';
  import { CHECK_ICON, COPY_ICON, copyText, renderMarkdown } from './markdown';
  import { accountLabel, accountsFor, bestAccountId, findAccount, isUsable } from './accounts.svelte';
  import { findGithub, github, githubLabel } from './github.svelte';
  import { MODES, PROVIDERS, PERMISSIONS, normalizePermission, type Attachment, type SentFile, type Conversation, type Msg, type PaneConfig, type Provider, type Totals, type UiEvent, type UsageRecord } from './types';

  let {
    paneId,
    index,
    config = $bindable(),
    installed,
    active,
    defaultFolder,
    onactivate,
    onopenaccounts,
    onconv,
    closable = false,
    onclose,
  }: {
    paneId: string;
    index: number;
    config: PaneConfig;
    installed: Partial<Record<Provider, string | null>>;
    active: boolean;
    defaultFolder: string;
    onactivate: () => void;
    onopenaccounts: () => void;
    /** Tells the window which saved conversation this pane shows. */
    onconv?: (id: string | null) => void;
    /** Show the × button (hidden when this is the only pane). */
    closable?: boolean;
    /** Removes this pane from the window. */
    onclose?: () => void;
  } = $props();

  let messages = $state<Msg[]>([]);
  let busy = $state(false);
  let input = $state('');
  /** Model name the CLI reported (e.g. Antigravity's display name). */
  let reportedModel = $state<string | null>(null);
  const emptyTotals = (): Totals => ({ input: 0, output: 0, cost: 0, hasCost: false, unpriced: false });
  let totals = $state<Totals>(emptyTotals());

  // The saved conversation this pane is showing (local history).
  let convId = $state<string | null>(null);
  /** One record per answered turn (for the Usage window). */
  let usageLog: UsageRecord[] = [];
  /** Last thing the user asked (re-sent after a wait, or carried to another AI). */
  let lastUserText = '';
  /** Files sent with the last message (re-sent by Continue / Allow & retry). */
  let lastUserFiles: SentFile[] = [];

  // ---- files attached to the next message ----
  const MAX_FILES = 10;
  const MAX_FILE_MB = 100;
  let attachments = $state<Attachment[]>([]);
  let dragOver = $state(false);
  /** Image thumbnails by path (also for images in saved conversations). */
  let previews = $state<Record<string, string>>({});
  let viewing = $state<string | null>(null);

  function remember(a: Attachment) {
    if (a.preview) previews[a.path] = a.preview;
  }

  function roomFor(n: number): boolean {
    if (attachments.length + n > MAX_FILES) {
      push({ role: 'error', text: `At most ${MAX_FILES} files per message.` });
      return false;
    }
    return true;
  }

  /** Pasted or dropped files: the raw bytes go to Wren, which keeps a private copy. */
  async function addBlobs(files: File[]) {
    if (!files.length || !roomFor(files.length)) return;
    for (const f of files) {
      const name = f.name || (f.type.startsWith('image/') ? `pasted-image.${f.type.split('/')[1] || 'png'}` : 'pasted-file');
      if (f.size > MAX_FILE_MB * 1024 * 1024) {
        push({ role: 'error', text: `"${name}" is larger than ${MAX_FILE_MB} MB.` });
        continue;
      }
      try {
        const bytes = new Uint8Array(await f.arrayBuffer());
        const a = await invoke<Attachment>('attach_bytes', bytes, { headers: { 'x-file-name': encodeURIComponent(name) } });
        remember(a);
        attachments.push(a);
      } catch (e) {
        push({ role: 'error', text: String(e) });
      }
    }
  }

  /** "Add files…" in the + menu: the system file picker (any file). */
  async function pickFiles() {
    const picked = await open({ multiple: true, directory: false, title: 'Attach files' });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (!paths.length || !roomFor(paths.length)) return;
    for (const p of paths) {
      try {
        const a = await invoke<Attachment>('attach_file', { path: p });
        remember(a);
        attachments.push(a);
      } catch (e) {
        push({ role: 'error', text: String(e) });
      }
    }
  }

  function onPaste(e: ClipboardEvent) {
    const files = [...(e.clipboardData?.files ?? [])];
    if (!files.length) return; // plain text pastes as usual
    e.preventDefault();
    addBlobs(files);
  }

  function onDrop(e: DragEvent) {
    dragOver = false;
    const files = [...(e.dataTransfer?.files ?? [])];
    if (!files.length) return;
    e.preventDefault();
    addBlobs(files);
  }

  const fmtSize = (n: number) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${Math.ceil(n / 1024)} KB` : `${n} B`);
  /** Short type label on a file chip: "CSV", "XLSX", "PDF", "ZIP"… */
  const fileTag = (name: string) => (name.includes('.') ? name.split('.').pop()!.slice(0, 5).toUpperCase() : 'FILE');

  /** Files of a sent message (older saves only have image paths). */
  const sentFiles = (m: Msg): SentFile[] =>
    m.files ?? (m.images ?? []).map((p) => ({ path: p, name: p.split(/[\\/]/).pop() ?? 'image', size: 0, mime: 'image/*', kind: 'image' as const }));

  /** Thumbnails of saved conversations are read back from Wren's folder. */
  $effect(() => {
    for (const m of messages) {
      for (const f of sentFiles(m)) {
        const p = f.path;
        if (f.kind !== 'image') continue;
        if (previews[p] === undefined) {
          previews[p] = '';
          invoke<string>('attachment_preview', { path: p })
            .then((url) => (previews[p] = url))
            .catch(() => (previews[p] = 'missing'));
        }
      }
    }
  });
  /** A usage-limit message seen during this turn, handled when the turn ends. */
  let pendingLimit: string | null = null;
  /** Waiting for a usage reset before continuing automatically. */
  let waiting = $state<{ until: number; known: boolean } | null>(null);
  let waitTimer: ReturnType<typeof setTimeout> | undefined;
  let clock = $state(Date.now());
  let clockTimer: ReturnType<typeof setInterval> | undefined;
  let convCreated = 0;
  let convTitle = '';
  let sessionId: string | null = null;
  let listEl: HTMLDivElement | undefined = $state();
  let opened = $state(false);
  /** The account the CLI process was last started with (the pane's menu can change later, e.g. when the first account is added). */
  let appliedAccount: string | null = null;
  /** The AI the CLI process was last started with. */
  let appliedProvider: Provider = config.provider;
  /** The conversation moved to another AI that hasn't been told about it yet:
   *  the next message carries a recap (label of the AI it came from). */
  let recapFrom: string | null = null;

  function markApplied() {
    appliedAccount = config.accountId;
    appliedProvider = config.provider;
  }
  let limitHit = $state(false);
  let switchTo = $state('');

  /** Index in `messages` where the current turn's answer starts (for Deep research reports). */
  let turnStart = 0;
  /** When the current turn was sent (ms), to find the pages it made. */
  let turnSentAt = 0;

  // ---- the browser panel next to the chat ----
  let browserOpen = $state(false);
  let browser: BrowserPanel | undefined = $state();
  /** Share of the pane's width the browser takes (drag the divider). */
  let browserShare = $state(0.5);
  let bodyEl: HTMLDivElement | undefined = $state();

  /** Open an address in this pane's browser panel. */
  async function openInBrowser(url: string) {
    browserOpen = true;
    await tick();
    browser?.go(url);
  }

  /** Open a page from the project (e.g. visuals/quicksort.html) in the panel. */
  async function openVisual(path: string) {
    try {
      openInBrowser(await invoke<string>('visual_url', { cwd: config.cwd, path }));
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  /** Same page twice (relative and full path) counts once. */
  const fullPath = (p: string) => (/^([a-zA-Z]:)?[\\/]/.test(p) ? p : `${config.cwd.replace(/[\\/]+$/, '')}/${p.replace(/^\.\//, '')}`);

  /**
   * After a turn: pages the AI wrote (any .html in its tool calls, or new
   * files in visuals/) get a card. In Visualize mode the newest opens by itself.
   */
  async function showNewVisuals() {
    if (!config.cwd) return;
    // Newest pages in visuals/ first, then any other .html the AI wrote.
    const paths = new Set<string>();
    try {
      for (const p of await invoke<string[]>('recent_visuals', { cwd: config.cwd, since: Math.max(0, turnSentAt - 2000) })) paths.add(fullPath(p));
    } catch {}
    for (const m of messages.slice(turnStart)) {
      if (m.role !== 'tool' || m.isError || !m.detail) continue;
      if (!/^(write|edit|multiedit|write_to_file|create_file|replace_file_content|apply_patch)$/i.test(m.text)) continue;
      for (const hit of m.detail.matchAll(/([^\s,'"]+\.html?)\b/gi)) paths.add(fullPath(hit[1]));
    }
    let first: string | null = null;
    for (const full of paths) {
      push({ role: 'visual', text: full.split(/[\\/]/).pop() ?? full, detail: full });
      first ??= full;
    }
    if (first) persist();
    if (first && config.mode === 'visualize') openVisual(first);
  }

  /** Drag the divider between the chat and the browser. */
  function dragDivider(e: PointerEvent) {
    const el = bodyEl;
    if (!el) return;
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const r = el.getBoundingClientRect();
    const move = (ev: PointerEvent) => {
      const share = 1 - (ev.clientX - r.left) / r.width;
      browserShare = Math.min(0.8, Math.max(0.2, share));
    };
    const up = () => {
      target.removeEventListener('pointermove', move);
      target.removeEventListener('pointerup', up);
    };
    target.addEventListener('pointermove', move);
    target.addEventListener('pointerup', up);
  }

  function nudgeDivider(e: KeyboardEvent) {
    if (e.key === 'ArrowLeft') browserShare = Math.min(0.8, browserShare + 0.05);
    else if (e.key === 'ArrowRight') browserShare = Math.max(0.2, browserShare - 0.05);
  }

  /** "Browser for the AI" switched: the CLI restarts with (or without) the tool; the conversation continues. */
  async function changeAiBrowser() {
    try {
      if (opened) await invoke('set_ai_browser', { paneId, on: !!config.aiBrowser });
      push({
        role: 'info',
        text: config.aiBrowser
          ? `Browser for the AI is on. ${providerLabel} can open pages in its own Chrome window${config.permission === 'manual' ? ' (it asks first)' : ''}. The first start downloads the browser tool.`
          : 'Browser for the AI is off.',
      });
      scrollToEnd(true);
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }
  /** Claude sent fresh limit numbers during this turn. */
  let sawLimitEvent = false;
  /** Ticks every minute so "resets in …" and reset windows stay current. */
  let now = $state(Date.now());
  /** The model is thinking (Claude/Codex report it); cleared when text or a tool call arrives. */
  let thinking = $state(false);
  /** When the current turn started, for the "Working… 0:42" timer. */
  let turnStartedAt = $state(0);
  let tickNow = $state(Date.now());
  let tickTimer: ReturnType<typeof setInterval> | undefined;
  /** The turn ended (answered, failed, stopped): stop the timer and remember how long it worked. */
  function endTurn() {
    clearInterval(tickTimer);
    thinking = false;
    if (!turnStartedAt) return;
    const u = [...messages].reverse().find((m) => m.role === 'user');
    if (u && u.durationMs === undefined) u.durationMs = Date.now() - turnStartedAt;
    turnStartedAt = 0;
  }

  // ---- Cowork-style step groups: one collapsible group per turn ----

  /** Steps open/closed by the user, per turn (the user message id). */
  let openTurns = $state<Record<number, boolean>>({});

  type Item =
    | { kind: 'msg'; m: Msg }
    | { kind: 'group'; key: number; steps: Msg[]; live: boolean; durationMs: number | undefined };

  /**
   * Each turn = the user's message, one group with the work (tool calls and the
   * AI's notes between them), then everything else in order (the answer,
   * approval cards, errors, notes).
   */
  const items = $derived.by(() => {
    const out: Item[] = [];
    const segs: { user: Msg | null; rest: Msg[] }[] = [{ user: null, rest: [] }];
    for (const m of messages) {
      if (m.role === 'user') segs.push({ user: m, rest: [] });
      else segs[segs.length - 1].rest.push(m);
    }
    segs.forEach((seg, i) => {
      const last = i === segs.length - 1;
      if (seg.user) out.push({ kind: 'msg', m: seg.user });
      // An AI message is a step if more tool work follows it in this turn.
      const lastTool = seg.rest.map((m) => m.role).lastIndexOf('tool');
      const steps = seg.rest.filter((m, j) => m.role === 'tool' || (m.role === 'assistant' && j < lastTool));
      const live = last && busy && !!seg.user;
      if (seg.user && (steps.length || live)) {
        out.push({ kind: 'group', key: seg.user.id, steps, live, durationMs: seg.user.durationMs });
      }
      for (const m of seg.rest) if (!steps.includes(m) && m.role !== 'details') out.push({ kind: 'msg', m });
    });
    return out;
  });

  /** The latest turn (from the last message you sent) is shown on its own, at least one screen tall, so it can sit at the top like in Claude Desktop. */
  const itemKey = (it: Item) => (it.kind === 'group' ? `g${it.key}` : it.m.id);
  const lastUserAt = $derived.by(() => {
    for (let i = items.length - 1; i >= 0; i--) {
      const it = items[i];
      if (it.kind === 'msg' && it.m.role === 'user') return i;
    }
    return -1;
  });
  const older = $derived(lastUserAt < 0 ? items : items.slice(0, lastUserAt));
  const latest = $derived(lastUserAt < 0 ? [] : items.slice(lastUserAt));
  /** Height of the chat area minus room for the bottom padding and a small gap above the turn. */
  let viewH = $state(0);
  /** Keep scrolling down with new output (only after you scroll to the end of a long answer yourself). */
  let followTail = false;
  let turnEl: HTMLDivElement | undefined = $state();

  function onListScroll() {
    if (!listEl) return;
    const nearBottom = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 40;
    followTail = nearBottom && !!turnEl && turnEl.offsetHeight > viewH + 4;
  }

  /** After sending: your message rolls up to the top; the progress and the answer come in under it. */
  function scrollToTurn() {
    followTail = false;
    tick().then(() => {
      if (listEl && turnEl) listEl.scrollTo({ top: turnEl.offsetTop - 8, behavior: 'smooth' });
    });
  }

  // ---- long messages you sent: first lines only, with Show more ----
  let expanded = $state<Record<number, boolean>>({});
  let tall = $state<Record<number, boolean>>({});
  function measureTall(node: HTMLElement, id: number) {
    const check = () => {
      if (!expanded[id]) tall[id] = node.scrollHeight > node.clientHeight + 2;
    };
    const ro = new ResizeObserver(check);
    ro.observe(node);
    check();
    return { destroy: () => ro.disconnect() };
  }

  // ---- the message box grows with its text, up to half the pane ----
  let paneEl: HTMLElement | undefined = $state();
  let inputEl: HTMLTextAreaElement | undefined = $state();
  function autosize() {
    if (!inputEl) return;
    const max = Math.max(80, Math.floor((paneEl?.clientHeight ?? 600) / 2));
    inputEl.style.height = 'auto';
    const h = Math.min(inputEl.scrollHeight, max);
    inputEl.style.height = `${h}px`;
    inputEl.style.overflowY = inputEl.scrollHeight > max ? 'auto' : 'hidden';
  }
  $effect(() => {
    void input;
    tick().then(autosize);
  });

  $effect(() => {
    const el = listEl;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      viewH = Math.max(0, el.clientHeight - 24);
      autosize();
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  /** Copy buttons on code blocks (added by renderMarkdown). */
  async function copyCode(btn: HTMLButtonElement) {
    const pre = btn.parentElement?.querySelector('pre');
    if (!pre) return;
    const ok = await copyText(pre.innerText.replace(/\n$/, ''));
    btn.innerHTML = ok ? `${CHECK_ICON}<span>Copied</span>` : 'Copy failed';
    btn.classList.add('copied');
    setTimeout(() => {
      btn.innerHTML = COPY_ICON;
      btn.classList.remove('copied');
    }, 2000);
  }

  const BIRD_WORDS = ['Pecking', 'Foraging', 'Figuring', 'Nesting', 'Preening', 'Scouting', 'Hopping', 'Fluttering', 'Gathering twigs'];
  const verb = $derived.by(() => {
    if (waitingQuestion) return 'Waiting for your answer';
    if (waitingApproval) return 'Waiting for your approval';
    if (thinking) return 'Thinking';
    const sec = Math.floor((tickNow - turnStartedAt) / 1000);
    return BIRD_WORDS[Math.floor(sec / 3) % BIRD_WORDS.length];
  });

  /** "Ran a command", "Read a file", … for a tool call. */
  function stepLabel(m: Msg): { label: string; subject: string } {
    const name = m.text;
    const d = m.detail ?? '';
    const file = d.split(/[\\/]/).filter(Boolean).pop() ?? d;
    const n = name.toLowerCase();
    if (['bash', 'shell', 'run_command', 'powershell', 'exec_command'].includes(n)) return { label: 'Ran a command', subject: '' };
    if (['read', 'view_file', 'read_file', 'notebookread'].includes(n)) return { label: 'Read a file', subject: file };
    if (['write', 'write_to_file', 'create_file'].includes(n)) return { label: 'Wrote a file', subject: file };
    if (['edit', 'multiedit', 'notebookedit', 'replace_file_content', 'replace', 'apply_patch'].includes(n))
      return { label: n === 'edit' && d.includes(',') ? 'Edited files' : 'Edited a file', subject: n === 'edit' && d.includes(',') ? '' : file };
    if (['grep', 'glob', 'grep_search', 'find_by_name', 'list_dir', 'ls'].includes(n)) return { label: 'Searched files', subject: d };
    if (['websearch', 'web_search', 'search_web'].includes(n)) return { label: 'Searched the web', subject: d };
    if (['webfetch', 'read_url_content', 'fetch'].includes(n)) return { label: 'Read a web page', subject: d };
    if (['agent', 'task'].includes(n)) return { label: 'Ran a sub-agent', subject: d };
    if (['todowrite', 'taskcreate', 'taskupdate'].includes(n)) return { label: 'Updated tasks', subject: '' };
    return { label: `Used ${name}`, subject: d };
  }

  function fmtDuration(ms: number | undefined): string {
    if (ms === undefined) return '';
    const s = Math.max(1, Math.round(ms / 1000));
    return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
  }

  /** Approval cards still waiting for the user. */
  const waitingApproval = $derived(messages.some((m) => (m.role === 'approval' && m.approvalState === 'waiting') || (m.role === 'question' && m.questionState === 'waiting')));
  const waitingQuestion = $derived(messages.some((m) => m.role === 'question' && m.questionState === 'waiting'));
  /** Antigravity "Allow & retry": the permission to go back to after the retried turn. */
  let revertPermission: string | null = null;
  /** One "refused" card per turn is enough. */
  let refusedThisTurn = false;
  let lastToolSummary = '';

  /** Antigravity can't ask in headless mode; these texts mean it refused something. */
  const REFUSAL = /cannot prompt|can't prompt|auto-?denied|needs? (your )?approval|requires? (your )?approval|permission (was )?denied|not allowed to run|blocked by (the )?permission/i;

  function checkRefusal(text: string | null | undefined) {
    if (!text || refusedThisTurn || config.provider !== 'antigravity' || config.permission !== 'manual') return;
    if (!REFUSAL.test(text)) return;
    refusedThisTurn = true;
    push({ role: 'approval', tool: 'Antigravity', text: lastToolSummary || 'An action that needs your permission', detail: text.slice(0, 600), approvalState: 'refused' });
  }

  /** Approval cards and question forms the CLI can no longer receive an answer for. */
  function expireApprovals() {
    for (const m of messages) {
      if (m.role === 'approval' && m.approvalState === 'waiting') m.approvalState = 'expired';
      if (m.role === 'question' && m.questionState === 'waiting') m.questionState = 'expired';
    }
  }

  /** Saved/reopened conversations: nothing can still be waiting. */
  function expireSaved(m: Msg): Msg {
    if (m.approvalState === 'waiting') return { ...m, approvalState: 'expired' };
    if (m.questionState === 'waiting') return { ...m, questionState: 'expired' };
    return m;
  }

  async function answerQuestion(m: Msg, answers: Record<string, string[]> | null) {
    if (!m.approvalId) return;
    try {
      await invoke('answer_question', { paneId, id: m.approvalId, answers });
      // Secret answers are never kept in the saved conversation.
      const secret = new Set((m.questions ?? []).filter((q) => q.secret).map((q) => q.id));
      m.answers = answers ? Object.fromEntries(Object.entries(answers).map(([k, v]) => [k, secret.has(k) ? ['(hidden)'] : v])) : undefined;
      m.questionState = answers ? 'answered' : 'skipped';
    } catch (e) {
      m.questionState = 'expired';
      push({ role: 'error', text: String(e) });
    }
    persist();
  }

  async function answerApproval(m: Msg, decision: 'allow' | 'allow_session' | 'deny') {
    if (!m.approvalId) return;
    try {
      await invoke('answer_approval', { paneId, id: m.approvalId, decision });
      m.approvalState = decision === 'allow' ? 'allowed' : decision === 'allow_session' ? 'allowed_session' : 'denied';
    } catch (e) {
      m.approvalState = 'expired';
      push({ role: 'error', text: String(e) });
    }
    persist();
  }

  /** Antigravity: re-send the last message once with Auto, then go back. */
  async function allowAndRetry(m: Msg) {
    if (busy || !lastUserText) return;
    m.approvalState = 'retried';
    try {
      revertPermission = config.permission;
      await invoke('set_permission', { paneId, permission: 'auto' });
      push({ role: 'info', text: 'Retrying your last message with Auto for this one turn. The pane goes back to Manual afterwards.' });
      await sendText(lastUserText, lastUserText, lastUserFiles);
    } catch (e) {
      revertPermission = null;
      push({ role: 'error', text: String(e) });
    }
  }

  /** After a retried turn, the pane goes back to its own permission (same conversation). */
  function revertAfterRetry() {
    if (!revertPermission) return;
    const p = revertPermission;
    revertPermission = null;
    invoke('set_permission', { paneId, permission: p }).catch(() => {});
  }

  /** Changing the permission keeps the conversation. */
  async function changePermission() {
    const label = PERMISSIONS.find((p) => p.value === config.permission)?.label;
    if (!opened) return;
    try {
      await invoke('set_permission', { paneId, permission: config.permission });
      push({ role: 'info', text: `Permission: ${label}. The conversation continues.` });
      persist();
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  const elapsed = $derived.by(() => {
    const s = Math.max(0, Math.floor((tickNow - turnStartedAt) / 1000));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  });

  let logs: string[] = [];
  let streamingId = $state<number | null>(null);
  let nextId = 0;

  const providerLabel = $derived(PROVIDERS[config.provider].label);
  const missing = $derived(installed[config.provider] === null);
  const folderName = $derived(config.cwd ? config.cwd.split(/[\\/]/).filter(Boolean).pop() : '');
  const permHint = $derived(PERMISSIONS.find((p) => p.value === config.permission)?.hint ?? '');
  const account = $derived(findAccount(config.accountId));
  const accountOk = $derived(isUsable(account));
  const otherAccounts = $derived(accountsFor(config.provider).filter((a) => a.id !== config.accountId && isUsable(a)));

  // ---- plan limits of this pane's account ----
  const limits = $derived(planLimits.byAccount[config.accountId]);
  /** The 5-hour and weekly windows for the footer meter. */
  const mainWindows = $derived((limits?.windows ?? []).filter((w) => w.key === 'five_hour' || w.key === 'seven_day'));
  /** Windows at 80% or more that the user hasn't dismissed (task: warnings at 80 / 95%). */
  const warnings = $derived(
    (limits?.windows ?? [])
      .map((w) => ({ w, pct: usedNow(w, now) }))
      .map((x) => ({ ...x, level: levelOf(x.pct) }))
      .filter((x) => x.level !== 'ok' && !dismissedWarnings[warnKey(x.w.key, x.level, x.w.resets_at)]),
  );
  const warnKey = (key: string, level: string, resetsAt: number | null) => `${config.accountId}|${key}|${level}|${resetsAt ?? ''}`;

  // Read the account's limits when the pane opens or the account changes (numbers newer than 5 min are reused).
  $effect(() => {
    const id = config.accountId;
    const p = config.provider;
    if (opened && installed[p] && accountOk) refreshLimits(id, p, 300_000);
  });

  const modelList = $derived(modelLists[modelsKey(config.provider, config.accountId)]);
  const defaultModel = $derived(modelList?.find((m) => m.is_default) ?? modelList?.[0]);

  /** The model to price this pane's usage with. */
  const pricedModel = $derived(
    (config.model && (modelList?.find((m) => m.id === config.model)?.resolved ?? config.model)) ||
      reportedModel ||
      defaultModel?.resolved ||
      defaultModel?.id ||
      '',
  );

  function persist() {
    if (!convId) return;
    const conv: Conversation = {
      id: convId,
      title: convTitle,
      provider: config.provider,
      accountId: config.accountId,
      cwd: config.cwd,
      permission: config.permission,
      model: config.model ?? '',
      effort: config.effort ?? '',
      mode: config.mode ?? null,
      sessionId,
      createdAt: convCreated,
      updatedAt: Date.now(),
      messages: $state.snapshot(messages)
        .filter((m) => m.role !== 'details')
        .map(expireSaved),
      totals: $state.snapshot(totals),
      usage: usageLog,
    };
    saveConversation(conv);
  }

  function startConversation(firstText: string) {
    convId = crypto.randomUUID();
    convCreated = Date.now();
    convTitle = firstText.replace(/\s+/g, ' ').slice(0, 60);
    sessionId = null;
    onconv?.(convId);
  }

  function forgetConversation() {
    convId = null;
    usageLog = [];
    sessionId = null;
    onconv?.(null);
  }

  /** Show the manual account picker (used when automatic handling is off or impossible). */
  function showLimitBanner() {
    limitHit = true;
    switchTo = otherAccounts[0]?.id ?? '';
  }

  /** This pane's account ran out of usage. Decide what to do by itself (task 3). */
  async function handleLimit(text: string) {
    if (settings.onLimit === 'switch') {
      if (await switchToOtherAI()) return;
      if (settings.waitIfNoOther) return startWaiting(text);
      return showLimitBanner();
    }
    if (settings.onLimit === 'wait') return startWaiting(text);
    showLimitBanner();
  }

  /** Continue in the next AI that is installed and has a usable account (same conversation). */
  async function switchToOtherAI(): Promise<boolean> {
    const from = config.provider;
    for (const p of settings.fallbackOrder) {
      if (p === from || !installed[p]) continue;
      const list = accountsFor(p).filter((a) => isUsable(a));
      const acc = list.find((a) => a.is_default) ?? list[0];
      if (!acc) continue;
      config.provider = p;
      config.accountId = acc.id;
      config.model = '';
      config.effort = '';
      return continueWith('limit');
    }
    return false;
  }

  /** The chat as the recap sees it: question forms count as the user's answers. */
  function recapSource(): { role: string; text: string }[] {
    return messages.map((m) => {
      if (m.role !== 'question' || !m.answers) return m;
      const parts = (m.questions ?? [])
        .filter((q) => !q.secret && m.answers?.[q.id]?.length)
        .map((q) => `${q.header || q.question}: ${m.answers![q.id].join(', ')}`);
      return { role: 'user', text: `(Answered the AI's questions) ${parts.join('; ')}` };
    });
  }

  /** "Claude Code · Work" for the pane's current AI and account. */
  const targetLabel = () => `${PROVIDERS[config.provider].label}${account ? ` · ${account.name}` : ''}`;

  /**
   * The user picked another account or AI (or the limit handling did): keep
   * this conversation and go on with the new one.
   * - Same AI: the CLI's own saved conversation is copied to the new account
   *   and resumed, so nothing is lost.
   * - Another AI (or no saved copy): the new AI gets a recap of the chat.
   * `why` = 'limit': the last request didn't finish, so it's sent again.
   */
  async function continueWith(why: 'switch' | 'limit'): Promise<boolean> {
    const fromProvider = appliedProvider;
    const fromAccount = appliedAccount;
    const fromLabel = PROVIDERS[fromProvider].label;
    if (!opened || !convId || !messages.some((m) => m.role === 'user')) {
      // Nothing to carry over yet.
      await applyConfig(config.provider === fromProvider ? `Account: ${account?.name ?? 'none'}` : `Switched to ${PROVIDERS[config.provider].label}`);
      return true;
    }
    limitHit = false;
    pendingLimit = null;
    stopWaiting();
    expireApprovals();
    const sameAI = config.provider === fromProvider;
    if (sameAI && sessionId && fromAccount && config.accountId && config.provider !== 'antigravity') {
      try {
        const carried = await invoke<boolean>('carry_session', { fromAccount, toAccount: config.accountId, sessionId });
        if (carried) {
          await invoke('resume_conversation', { paneId, config: $state.snapshot(config), sessionId });
          markApplied();
          recapFrom = null;
          push({ role: 'divider', text: `Continued with ${targetLabel()}` });
          persist();
          if (why === 'limit' && lastUserText) await sendText(lastUserText, lastUserText, lastUserFiles);
          return true;
        }
      } catch (e) {
        push({ role: 'notice', text: `Couldn't move the saved conversation (${e}). Sending a recap instead.` });
      }
    }
    try {
      await invoke('update_pane', { paneId, config: $state.snapshot(config) });
      markApplied();
    } catch (e) {
      push({ role: 'error', text: String(e) });
      return false;
    }
    sessionId = null;
    reportedModel = null;
    push({
      role: 'divider',
      text: why === 'limit' ? `${fromLabel} ran out of usage. Continued with ${targetLabel()}` : `Continued with ${targetLabel()}`,
    });
    // A recap still waiting from an earlier move covers this one too.
    recapFrom = why === 'limit' ? fromLabel : (recapFrom ?? fromLabel);
    persist();
    // Without a usable account the recap waits for the next message.
    if (!account || !isUsable(account) || !installed[config.provider]) {
      push({ role: 'info', text: `Your next message will include a recap of this conversation for ${PROVIDERS[config.provider].label}.` });
      return true;
    }
    const recap = buildRecap(recapSource(), why === 'limit' ? lastUserText : '', recapFrom, 8000, why);
    recapFrom = null;
    await sendText(
      recap,
      why === 'limit' ? `(Recap of the conversation with ${fromLabel})\n\n${lastUserText}` : `(Recap of the conversation so far, sent to ${targetLabel()})`,
      why === 'limit' ? lastUserFiles : [],
    );
    return true;
  }

  /** Wait until the reset time, then re-send the last message on the same account. */
  function startWaiting(text: string) {
    const reset = parseResetTime(text);
    const until = (reset ? reset.getTime() : Date.now() + 30 * 60_000) + 60_000;
    waiting = { until, known: !!reset };
    clearTimeout(waitTimer);
    waitTimer = setTimeout(continueNow, Math.max(5_000, until - Date.now()));
    clearInterval(clockTimer);
    clockTimer = setInterval(() => (clock = Date.now()), 20_000);
    clock = Date.now();
  }

  function stopWaiting() {
    clearTimeout(waitTimer);
    clearInterval(clockTimer);
    waiting = null;
  }

  async function continueNow() {
    stopWaiting();
    if (!lastUserText || busy) return;
    push({ role: 'info', text: 'Usage limit should have reset. Continuing automatically.' });
    await sendText(lastUserText, lastUserText, lastUserFiles);
  }

  function push(m: Omit<Msg, 'id'>): number {
    const id = nextId++;
    messages.push({ ...m, id });
    return id;
  }
  const byId = (id: number) => messages.find((m) => m.id === id);

  function scrollToEnd(force = false) {
    if (!listEl) return;
    if (force || followTail) {
      tick().then(() => listEl && (listEl.scrollTop = listEl.scrollHeight));
    }
  }

  function handle(ev: UiEvent) {
    switch (ev.kind) {
      case 'plan_limits':
        sawLimitEvent = true;
        applyLimitEvent(config.accountId, ev.limits);
        return;
      case 'thinking':
        thinking = true;
        return;
      case 'approval':
        thinking = false;
        streamingId = null;
        push({ role: 'approval', approvalId: ev.id, tool: ev.tool, text: ev.title, detail: ev.detail ?? undefined, approvalState: 'waiting', allowSession: ev.allow_session });
        scrollToEnd(true);
        return;
      case 'question':
        thinking = false;
        streamingId = null;
        push({ role: 'question', approvalId: ev.id, text: '', tool: PROVIDERS[config.provider].label, questions: ev.questions, questionState: 'waiting' });
        scrollToEnd(true);
        return;
      case 'notice':
        push({ role: 'notice', text: ev.text });
        break;
      case 'session_started':
        reportedModel = ev.model;
        sessionId = ev.session_id;
        persist();
        break;
      case 'text_delta': {
        thinking = false;
        if (streamingId === null) streamingId = push({ role: 'assistant', text: '' });
        const m = byId(streamingId);
        if (m) m.text += ev.text;
        break;
      }
      case 'text_end':
        if (streamingId !== null) {
          const m = byId(streamingId);
          if (m && ev.full != null) m.text = ev.full;
          streamingId = null;
        } else if (ev.full) {
          push({ role: 'assistant', text: ev.full });
        }
        break;
      case 'tool_use':
        thinking = false;
        streamingId = null;
        lastToolSummary = ev.summary ? `${ev.name}: ${ev.summary}` : ev.name;
        push({ role: 'tool', text: ev.name, detail: ev.summary });
        break;
      case 'tool_result': {
        const last = [...messages].reverse().find((m) => m.role === 'tool' && m.result === undefined);
        if (last) {
          last.result = ev.output;
          last.isError = ev.is_error;
        }
        if (ev.is_error) checkRefusal(ev.output);
        break;
      }
      case 'turn_done':
        busy = false;
        streamingId = null;
        checkRefusal(ev.error);
        expireApprovals();
        endTurn();
        revertAfterRetry();
        usageLog.push({
          t: Date.now(),
          provider: config.provider,
          accountId: config.accountId,
          model: pricedModel,
          input: ev.usage.input_tokens + ev.usage.cache_write_tokens,
          cached: ev.usage.cache_read_tokens,
          output: ev.usage.output_tokens,
          cost: ev.cost_usd ?? estimateCost(pricedModel, ev.usage),
        });
        totals.input += ev.usage.input_tokens + ev.usage.cache_read_tokens + ev.usage.cache_write_tokens;
        totals.output += ev.usage.output_tokens;
        {
          // Claude Code reports cost itself; others are estimated from API list prices.
          const cost = ev.cost_usd ?? estimateCost(pricedModel, ev.usage);
          if (cost != null) {
            totals.cost += cost;
            totals.hasCost = true;
          } else if (ev.usage.input_tokens + ev.usage.output_tokens > 0) {
            totals.unpriced = true;
          }
        }
        if (ev.error) {
          push({ role: 'error', text: ev.error });
          if (isLimitMessage(ev.error)) pendingLimit = ev.error;
        } else if (config.mode === 'research') {
          saveResearchReport();
        }
        if (!ev.error) showNewVisuals();
        // Codex doesn't report limits during a turn, so ask it (at most once a minute).
        // Claude normally sends them during the turn; ask only if it didn't.
        if (config.provider === 'codex') refreshLimits(config.accountId, 'codex', 60_000);
        else if (config.provider === 'claude' && !sawLimitEvent) refreshLimits(config.accountId, 'claude', 300_000);
        persist();
        if (pendingLimit) {
          const text = pendingLimit;
          pendingLimit = null;
          handleLimit(text);
        }
        break;
      case 'error':
        checkRefusal(ev.message);
        push({ role: 'error', text: ev.message });
        if (isLimitMessage(ev.message)) pendingLimit = ev.message;
        break;
      case 'log':
        if (busy) checkRefusal(ev.line);
        logs.push(ev.line);
        if (logs.length > 60) logs = logs.slice(-60);
        break;
      case 'exited':
        streamingId = null;
        expireApprovals();
        if (busy) endTurn();
        if (pendingLimit) {
          const text = pendingLimit;
          pendingLimit = null;
          busy = false;
          handleLimit(text);
          break;
        }
        if (busy) {
          busy = false;
          if (ev.code !== 0 && ev.code !== null) {
            const tail = logs.slice(-8).join('\n');
            push({ role: 'error', text: `${providerLabel} stopped (exit code ${ev.code}).${tail ? '\n\n' + tail : ''}` });
          }
        }
        break;
    }
    scrollToEnd();
  }

  /** Deep research: Wren saves the turn's final answer as a Markdown file in the project folder. */
  async function saveResearchReport() {
    const answers = messages.slice(turnStart).filter((m) => m.role === 'assistant' && m.text.trim());
    if (!answers.length) return;
    // The report is the last answer; if that is short, the AI split it into several parts.
    const last = answers[answers.length - 1].text.trim();
    const text = last.length >= 400 ? last : answers.map((m) => m.text.trim()).join('\n\n');
    if (text.length < 200) return;
    const d = new Date();
    const p2 = (n: number) => String(n).padStart(2, '0');
    const stamp = `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}-${p2(d.getHours())}${p2(d.getMinutes())}`;
    try {
      const path = await invoke<string>('save_report', { cwd: config.cwd, stamp, text });
      const rel = config.cwd && path.startsWith(config.cwd) ? path.slice(config.cwd.length).replace(/^[\\/]/, '') : path;
      push({ role: 'info', text: `Report saved to ${rel} in ${folderName}.` });
    } catch (e) {
      push({ role: 'error', text: `Could not save the report: ${e}` });
    }
    persist();
  }

  /** Study / Deep research turned on or off: the conversation continues. */
  async function changeMode() {
    try {
      if (opened) await invoke('set_mode', { paneId, mode: config.mode ?? null });
      push({
        role: 'info',
        text: config.mode
          ? `${MODES[config.mode].label} is on.${config.mode === 'research' ? ' The final report is also saved in the project folder under research/.' : ''}${config.mode === 'visualize' ? ' Ask about an idea or algorithm: the animation is saved under visuals/ and opens in the browser panel.' : ''}`
          : 'Back to normal mode.',
      });
      persist();
      scrollToEnd(true);
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  async function pickFolder() {
    const dir = await open({
      directory: true,
      multiple: false,
      title: 'Choose a project folder',
      defaultPath: config.cwd || defaultFolder || undefined,
    });
    if (typeof dir === 'string' && dir !== config.cwd) {
      config.cwd = dir;
      await applyConfig(`Folder set to ${dir}`);
    }
  }

  async function applyConfig(note: string) {
    if (!opened) return;
    try {
      await invoke('update_pane', { paneId, config: $state.snapshot(config) });
      markApplied();
      resetView();
      push({ role: 'info', text: `${note}. New conversation.` });
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  function resetView() {
    forgetConversation();
    recapFrom = null;
    messages = [];
    busy = false;
    reportedModel = null;
    streamingId = null;
    limitHit = false;
    pendingLimit = null;
    stopWaiting();
    logs = [];
    thinking = false;
    clearInterval(tickTimer);
    totals = emptyTotals();
  }

  async function newConversation() {
    try {
      await invoke('new_conversation', { paneId });
      resetView();
      push({ role: 'info', text: 'New conversation started.' });
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  // ---- closing this pane (the × button) ----
  /** Shown when the pane is still working, so closing never cuts off a reply by surprise. */
  let confirmClose = $state(false);
  const working = $derived(busy || !!waiting);

  // The task finished while the question was open: nothing left to confirm.
  $effect(() => {
    if (!working) confirmClose = false;
  });

  function requestClose() {
    if (working) confirmClose = true;
    else close();
  }

  async function stopAndClose() {
    if (busy) await stop();
    close();
  }

  function close() {
    confirmClose = false;
    stopWaiting();
    // The conversation stays in the sidebar's local history so it can be reopened.
    persist();
    onclose?.();
  }

  async function stop() {
    try {
      await invoke('stop_pane', { paneId });
      busy = false;
      streamingId = null;
      expireApprovals();
      endTurn();
      revertAfterRetry();
      push({ role: 'info', text: 'Stopped. Your next message continues this conversation.' });
      persist();
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  function changeProvider() {
    config.accountId = bestAccountId(config.provider);
    config.model = '';
    config.effort = '';
    continueWith('switch');
  }

  /** Model/effort changed: the conversation continues with the new setting. */
  async function changeModel() {
    try {
      if (opened) await invoke('set_model', { paneId, model: config.model || null, effort: config.effort || null });
      persist();
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  /** Open a saved conversation here and continue it (sidebar). */
  export async function openConversation(id: string) {
    try {
      if (busy) await invoke('stop_pane', { paneId });
      const conv = await getConversation(id);
      config.provider = conv.provider;
      config.accountId = conv.accountId;
      config.cwd = conv.cwd;
      config.permission = normalizePermission(conv.permission);
      config.model = conv.model ?? '';
      config.effort = conv.effort ?? '';
      config.mode = conv.mode ?? null;
      await invoke('resume_conversation', { paneId, config: $state.snapshot(config), sessionId: conv.sessionId });
      markApplied();
      busy = false;
      streamingId = null;
      limitHit = false;
      logs = [];
      reportedModel = null;
      stopWaiting();
      pendingLimit = null;
      usageLog = conv.usage ?? [];
      const lastUser = [...(conv.messages ?? [])].reverse().find((m) => m.role === 'user');
      lastUserText = lastUser?.text ?? '';
      lastUserFiles = lastUser ? sentFiles(lastUser) : [];
      messages = (conv.messages ?? []).map(expireSaved);
      nextId = messages.reduce((mx, m) => Math.max(mx, m.id + 1), 0);
      totals = { ...emptyTotals(), ...(conv.totals ?? {}) };
      convId = conv.id;
      convCreated = conv.createdAt;
      convTitle = conv.title;
      sessionId = conv.sessionId;
      onconv?.(conv.id);
      scrollToEnd(true);
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  /** Start a fresh conversation here (sidebar "New"). */
  export function startNew() {
    newConversation();
  }

  function changeAccount() {
    continueWith('switch');
  }

  /** Another GitHub account (or None): the CLI restarts with it, the conversation continues. */
  async function changeGithub() {
    try {
      if (opened) await invoke('set_github', { paneId, config: $state.snapshot(config) });
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  }

  // A GitHub account removed in Accounts: this pane goes back to None.
  $effect(() => {
    if (github.loaded && config.githubAccountId && !findGithub(config.githubAccountId)) {
      untrack(() => {
        config.githubAccountId = '';
        changeGithub();
      });
    }
  });

  /** The user picked another account after hitting a limit. Never automatic. */
  function switchAccount() {
    if (!switchTo) return;
    config.accountId = switchTo;
    continueWith('limit');
  }

  async function send() {
    const text = input.trim();
    if ((!text && !attachments.length) || busy) return;
    if (text === '\\new') {
      input = '';
      await newConversation();
      return;
    }
    if (!config.cwd) {
      push({ role: 'error', text: 'Choose a project folder for this pane first.' });
      return;
    }
    // The pane got an account since its CLI started (e.g. the first one was just added).
    if (opened && config.accountId !== appliedAccount) {
      try {
        await invoke('update_pane', { paneId, config: $state.snapshot(config) });
        // The CLI starts fresh: the conversation so far goes along as a recap.
        if (messages.some((m) => m.role === 'user') && !recapFrom) recapFrom = PROVIDERS[appliedProvider].label;
        markApplied();
      } catch {}
    }
    input = '';
    const files: SentFile[] = attachments.map(({ preview: _p, ...f }) => f);
    attachments = [];
    stopWaiting();
    limitHit = false;
    if (recapFrom) {
      const recap = buildRecap(recapSource(), text, recapFrom, 8000, 'switch');
      recapFrom = null;
      await sendText(recap, text, files, text);
      return;
    }
    await sendText(text, text, files);
  }

  /** Send `cliText` to the CLI; show `shownText` in the chat. `request` = what the user asked (re-sent by Continue / Allow & retry). */
  async function sendText(cliText: string, shownText: string, files: SentFile[] = [], request = cliText) {
    if (!convId) startConversation(shownText || files[0]?.name || 'Attachment');
    if (!isRecapBubble(shownText)) {
      lastUserText = request;
      lastUserFiles = files;
    }
    pendingLimit = null;
    sawLimitEvent = false;
    push({ role: 'user', text: shownText, files: files.length ? files : undefined });
    turnStart = messages.length;
    turnSentAt = Date.now();
    persist();
    busy = true;
    thinking = false;
    refusedThisTurn = false;
    lastToolSummary = '';
    turnStartedAt = tickNow = Date.now();
    clearInterval(tickTimer);
    tickTimer = setInterval(() => (tickNow = Date.now()), 1000);
    scrollToTurn();
    try {
      await invoke('send_message', { paneId, text: cliText, files: files.map((f) => f.path) });
    } catch (e) {
      busy = false;
      endTurn();
      push({ role: 'error', text: String(e) });
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      send();
    }
  }

  // Links inside answers never navigate the app window: web links and the
  // project's pages open in the browser panel.
  function blockLinks(e: MouseEvent) {
    const t = e.target as HTMLElement;
    const copy = t.closest<HTMLButtonElement>('button.copybtn');
    if (copy) return void copyCode(copy);
    const a = t.closest('a');
    if (!a) return;
    e.preventDefault();
    const href = a.getAttribute('href') ?? '';
    if (/^https?:\/\//i.test(href)) openInBrowser(href);
    else if (/\.(html?|svg)([?#].*)?$/i.test(href) && !/^[a-z][a-z0-9+.-]*:/i.test(href)) openVisual(decodeURIComponent(href.replace(/^file:\/\//, '').replace(/[?#].*$/, '')));
  }

  const fmt = (n: number) => (n >= 1_000_000 ? (n / 1_000_000).toFixed(1) + 'M' : n >= 1000 ? (n / 1000).toFixed(1) + 'k' : String(n));

  onMount(async () => {
    const channel = new Channel<UiEvent>();
    channel.onmessage = handle;
    try {
      await invoke('open_pane', { paneId, config: $state.snapshot(config), onEvent: channel });
      markApplied();
      opened = true;
      nowTimer = setInterval(() => (now = Date.now()), 60_000);
    } catch (e) {
      push({ role: 'error', text: String(e) });
    }
  });

  let nowTimer: ReturnType<typeof setInterval> | undefined;

  onDestroy(() => {
    clearInterval(nowTimer);
    clearInterval(tickTimer);
    stopWaiting();
    invoke('close_pane', { paneId }).catch(() => {});
  });
</script>

<section class="pane" bind:this={paneEl} class:active onpointerdown={onactivate} aria-label={`Conversation ${index + 1}`}>
  <header class="bar">
    <label class="sr" for={`${paneId}-provider`}>AI</label>
    <select
      id={`${paneId}-provider`}
      bind:value={config.provider}
      disabled={busy}
      onchange={changeProvider}
    >
      {#each Object.entries(PROVIDERS) as [value, p]}
        <option {value}>{p.label}{installed[value as Provider] === null ? ' (not installed)' : ''}</option>
      {/each}
    </select>

    <label class="sr" for={`${paneId}-account`}>Account</label>
    <select id={`${paneId}-account`} class="account" bind:value={config.accountId} disabled={busy} onchange={changeAccount} title="Which account this conversation uses">
      {#each accountsFor(config.provider) as a (a.id)}
        <option value={a.id}>{accountLabel(a)}</option>
      {:else}
        <option value="">No account yet</option>
      {/each}
    </select>

    {#if github.list.length || config.githubAccountId}
      <label class="sr" for={`${paneId}-github`}>GitHub account</label>
      <select
        id={`${paneId}-github`}
        class="github"
        bind:value={config.githubAccountId}
        disabled={busy}
        onchange={changeGithub}
        title="GitHub account that gh and git push / pull use in this pane. None = your computer's normal setup."
      >
        <option value="">GitHub: None</option>
        {#each github.list as g (g.id)}
          <option value={g.id}>GitHub: {githubLabel(g)}</option>
        {/each}
      </select>
    {/if}

    <label class="sr" for={`${paneId}-perm`}>Permission</label>
    <select
      id={`${paneId}-perm`}
      bind:value={config.permission}
      disabled={busy}
      title={permHint}
      class:danger={config.permission === 'full'}
      onchange={changePermission}
    >
      {#each PERMISSIONS as p}
        <option value={p.value}>{p.label}</option>
      {/each}
    </select>

    <span class="spacer"></span>
    {#if reportedModel}<span class="model" title={`Model reported by the CLI: ${reportedModel}`}>{modelName(modelList, reportedModel)}</span>{/if}
    {#if busy}
      <button class="ghost" onclick={stop} title="Stop the current task">Stop</button>
    {/if}
    <button
      class="ghost browserbtn"
      class:on={browserOpen}
      onclick={() => (browserOpen = !browserOpen)}
      aria-pressed={browserOpen}
      title={browserOpen ? 'Close the browser panel' : 'Open a browser next to this chat'}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
      <span>Browser</span>
    </button>
    <button class="ghost" onclick={newConversation} title="New conversation (or type \new)">New</button>
    {#if closable}
      <button class="ghost close" onclick={requestClose} aria-label={`Close conversation ${index + 1}`} title="Close this pane">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    {/if}
  </header>

  {#if confirmClose}
    <div class="banner limit" role="alertdialog" aria-label="Close this pane?">
      <span>
        {busy ? (waitingApproval ? 'This conversation is waiting for your approval.' : 'This conversation is still working.') : 'Wren is waiting to continue this conversation.'}
        Stop it and close the pane? It stays in the sidebar.
      </span>
      <button class="ghost" onclick={stopAndClose}>Stop &amp; close</button>
      <button class="ghost" onclick={() => (confirmClose = false)}>Cancel</button>
    </div>
  {/if}

  {#if missing}
    <p class="banner">
      The <code>{PROVIDERS[config.provider].bin}</code> CLI wasn't found. Install it, then restart Wren.
    </p>
  {:else if !account}
    <p class="banner">
      There's no {providerLabel} account in Wren yet.
      <button class="linklike" onclick={onopenaccounts}>Open Accounts to add one</button>
    </p>
  {:else if account && !accountOk}
    <p class="banner">
      {#if config.provider === 'antigravity'}
        "{account.name}" isn't the active Antigravity account.
      {:else}
        "{account.name}" isn't signed in.
      {/if}
      <button class="linklike" onclick={onopenaccounts}>Open Accounts to sign in</button>
    </p>
  {/if}
  {#if waiting}
    <div class="banner limit" role="status">
      <span>
        Usage limit reached. Wren will continue automatically
        {waiting.known ? `at ${new Date(waiting.until).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}` : 'after trying again'}
        (in {formatWait(waiting.until - clock)}). Keep Wren open.
      </span>
      <button class="ghost" onclick={continueNow}>Continue now</button>
      <button class="ghost" onclick={stopWaiting}>Cancel</button>
    </div>
  {/if}
  {#if limitHit}
    <div class="banner limit" role="alert">
      <span>This account hit its usage limit.</span>
      {#if otherAccounts.length}
        <label class="sr" for={`${paneId}-switch`}>Switch to account</label>
        <select id={`${paneId}-switch`} bind:value={switchTo}>
          {#each otherAccounts as a (a.id)}<option value={a.id}>{accountLabel(a)}</option>{/each}
        </select>
        <button class="ghost" onclick={switchAccount}>Switch and continue</button>
      {:else}
        <button class="linklike" onclick={onopenaccounts}>Add another account</button>
      {/if}
      <button class="ghost" onclick={() => (limitHit = false)} aria-label="Dismiss">Dismiss</button>
    </div>
  {/if}

  {#each warnings as x (x.w.key)}
    <div class="banner limit" class:critical={x.level === 'critical'} role="status">
      <span>
        <b>{x.level === 'critical' ? '⛔ Almost out:' : '⚠ Near the limit:'}</b>
        the {x.w.label} limit of "{account?.name ?? 'this account'}" is {Math.round(x.pct)}% used{x.w.resets_at ? ` (${resetText(x.w.resets_at, now)})` : ''}.
      </span>
      {#if x.level === 'critical'}<button class="linklike" onclick={onopenaccounts}>Accounts</button>{/if}
      <button class="ghost" onclick={() => (dismissedWarnings[warnKey(x.w.key, x.level, x.w.resets_at)] = true)}>Dismiss</button>
    </div>
  {/each}

  <div class="body" bind:this={bodyEl}>
  <div class="chat">
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="messages" bind:this={listEl} onclick={blockLinks} onscroll={onListScroll}>
    {#if messages.length === 0}
      <div class="empty">
        <Logo size={56} color="var(--muted)" hole="var(--surface)" />
        <p>{config.cwd ? `Ask ${providerLabel} anything about ${folderName}.` : 'Choose a project folder to start.'}</p>
        <p class="hint">Change the folder with the chip in the message box.</p>
        <p class="hint">Enter to send · Shift+Enter for a new line · <code>\new</code> starts over</p>
      </div>
    {/if}
    <div class="col">
    {#each older as it (itemKey(it))}{@render row(it)}{/each}
    {#if latest.length}
      <div class="turn" bind:this={turnEl} style:min-height={`${viewH}px`}>
        {#each latest as it (itemKey(it))}{@render row(it)}{/each}
      </div>
    {/if}
    </div>
  </div>

  {#snippet row(it: Item)}
      {#if it.kind === 'group'}
        {@const open = openTurns[it.key] ?? it.live}
        <div class="steps" class:live={it.live}>
          <button class="shead" aria-expanded={open} onclick={() => (openTurns[it.key] = !open)}>
            {#if it.live}
              <PeckingWren size={18} />
              <span class="verb">{verb}</span>
              <span class="stime">{elapsed}</span>
            {:else}
              <span class="verb done">{it.durationMs !== undefined ? `Worked for ${fmtDuration(it.durationMs)}` : `${it.steps.length} step${it.steps.length === 1 ? '' : 's'}`}</span>
            {/if}
            <svg class="chev" class:open width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          </button>
          {#if open && it.steps.length}
            <div class="slist">
              {#each it.steps as m (m.id)}
                {#if m.role === 'tool'}
                  {@const sl = stepLabel(m)}
                  <details class="srow" class:err={m.isError}>
                    <summary>
                      <span class="slabel">{sl.label}</span>
                      {#if sl.subject}<span class="ssubject">{sl.subject}</span>{/if}
                      {#if m.result === undefined && it.live}<span class="sstate">running</span>{:else if m.isError}<span class="sstate bad">failed</span>{/if}
                      <svg class="schev" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
                    </summary>
                    <div class="sbody">
                      {#if m.detail}<pre class="scmd">{m.detail}</pre>{/if}
                      {#if m.result !== undefined}<pre class="sout">{m.result || '(no output)'}</pre>{:else}<p class="snone">{it.live ? 'Still running…' : 'No result.'}</p>{/if}
                    </div>
                  </details>
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
          {#if sentFiles(m).length}
            <div class="uimgs">
              {#each sentFiles(m) as f (f.path)}
                {#if f.kind === 'image'}
                  {#if previews[f.path] && previews[f.path] !== 'missing'}
                    <button class="uthumb" onclick={() => (viewing = previews[f.path])} aria-label={`View ${f.name}`}><img src={previews[f.path]} alt={f.name} /></button>
                  {:else}
                    <span class="uthumb missing" title="This image is no longer on this computer">{previews[f.path] === 'missing' ? 'Image removed' : '…'}</span>
                  {/if}
                {:else}
                  <span class="fchip sent" title={f.name}>
                    <span class="ftag">{fileTag(f.name)}</span>
                    <span class="fmeta"><span class="fname">{f.name}</span><span class="fsize">{fmtSize(f.size)}</span></span>
                  </span>
                {/if}
              {/each}
            </div>
          {/if}
          {#if m.text}
            <div class="msg user" class:clamped={!expanded[m.id]} class:fade={tall[m.id] && !expanded[m.id]} use:measureTall={m.id}>{m.text}</div>
            {#if tall[m.id] || expanded[m.id]}
              <button class="more" aria-expanded={!!expanded[m.id]} onclick={() => (expanded[m.id] = !expanded[m.id])}>{expanded[m.id] ? 'Show less' : 'Show more'}</button>
            {/if}
          {/if}
        {:else if m.role === 'assistant'}
          <div class="msg assistant md">{@html renderMarkdown(m.text)}</div>
        {:else if m.role === 'error'}
          <div class="msg error" role="alert">{m.text}</div>
        {:else if m.role === 'approval'}
        <div class="msg approval" class:waiting={m.approvalState === 'waiting' || m.approvalState === 'refused'} role={m.approvalState === 'waiting' ? 'alertdialog' : 'group'} aria-label={`${m.tool} approval`}>
          <div class="ahead">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="11" width="16" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg>
            {#if m.approvalState === 'refused' || m.approvalState === 'retried' || m.approvalState === 'kept'}
              <span><b>{m.tool}</b> refused an action that needs your permission (it can't ask in this mode)</span>
            {:else if m.tool === 'ExitPlanMode'}
              <span><b>Plan ready</b>: {m.text}</span>
            {:else}
              <span><b>{m.tool}</b> wants to:</span>
            {/if}
          </div>
          {#if m.tool !== 'ExitPlanMode'}<pre class="acmd">{m.text}</pre>{/if}
          {#if m.detail}
            {#if m.tool === 'ExitPlanMode'}
              <div class="aplan md">{@html renderMarkdown(m.detail)}</div>
            {:else}
              <p class="adetail">{m.detail}</p>
            {/if}
          {/if}
          {#if m.approvalState === 'waiting'}
            <div class="abtns">
              <button class="primary" onclick={() => answerApproval(m, 'allow')}>{m.tool === 'ExitPlanMode' ? 'Start' : 'Allow'}</button>
              {#if m.allowSession && m.tool !== 'ExitPlanMode'}
                <button class="ghost" onclick={() => answerApproval(m, 'allow_session')} title="Allow this kind of action for the rest of this conversation">Allow for this chat</button>
              {/if}
              <button class="ghost" onclick={() => answerApproval(m, 'deny')}>{m.tool === 'ExitPlanMode' ? 'Keep planning' : 'Deny'}</button>
            </div>
          {:else if m.approvalState === 'refused'}
            <div class="abtns">
              <button class="primary" disabled={busy} onclick={() => allowAndRetry(m)} title="Send your last message again with Auto for one turn">Allow &amp; retry</button>
              <button class="ghost" onclick={() => { m.approvalState = 'kept'; persist(); }}>Keep refused</button>
            </div>
          {:else}
            <p class={`astate ${m.approvalState}`}>
              {#if m.approvalState === 'allowed'}✓ Allowed
              {:else if m.approvalState === 'allowed_session'}✓ Allowed for this chat
              {:else if m.approvalState === 'denied'}✗ Denied
              {:else if m.approvalState === 'retried'}↻ Retried with Auto for one turn
              {:else if m.approvalState === 'kept'}✗ Kept refused
              {:else}No longer waiting (the task ended or was stopped){/if}
            </p>
          {/if}
        </div>
        {:else if m.role === 'question'}
          <QuestionCard
            who={m.tool ?? PROVIDERS[config.provider].label}
            questions={m.questions ?? []}
            status={m.questionState ?? 'expired'}
            answers={m.answers}
            onsubmit={(a) => answerQuestion(m, a)}
          />
        {:else if m.role === 'visual'}
          <div class="visual">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="14" rx="2" /><path d="m10 8.5 4.5 2.5-4.5 2.5z" /><path d="M8 21h8" /></svg>
            <span class="vtext"><b>{m.text}</b><small title={m.detail}>{m.detail && config.cwd && m.detail.startsWith(config.cwd) ? m.detail.slice(config.cwd.length).replace(/^[\\/]/, '') : m.detail}</small></span>
            <button class="primary" onclick={() => m.detail && openVisual(m.detail)}>Open</button>
          </div>
        {:else if m.role === 'divider'}
          <div class="divider" role="separator" aria-label={m.text}><span>{m.text}</span></div>
        {:else if m.role === 'notice'}
          <div class="msg notice" role="status">{m.text}</div>
        {:else if m.role !== 'tool'}
          <div class="msg info">{m.text}</div>
        {/if}
      {/if}
  {/snippet}

  <footer class="composer">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="inputbox"
      class:dragover={dragOver}
      ondragover={(e) => { if (e.dataTransfer?.types.includes('Files')) { e.preventDefault(); dragOver = true; } }}
      ondragleave={() => (dragOver = false)}
      ondrop={onDrop}
    >
      {#if attachments.length}
        <div class="atts" aria-label="Attached files">
          {#each attachments as a, i (a.path)}
            <div class="att">
              {#if a.kind === 'image' && a.preview}
                <button class="attimg" onclick={() => (viewing = a.preview)} aria-label={`View ${a.name}`}><img src={a.preview} alt={a.name} /></button>
              {:else}
                <span class="fchip" title={a.name}>
                  <span class="ftag">{fileTag(a.name)}</span>
                  <span class="fmeta"><span class="fname">{a.name}</span><span class="fsize">{fmtSize(a.size)}</span></span>
                </span>
              {/if}
              <button class="attx" onclick={() => attachments.splice(i, 1)} aria-label={`Remove ${a.name}`} title="Remove">
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
              </button>
            </div>
          {/each}
        </div>
      {/if}
      <label class="sr" for={`${paneId}-input`}>Message</label>
      <textarea
        id={`${paneId}-input`}
        bind:this={inputEl}
        rows="2"
        bind:value={input}
        onkeydown={onKey}
        onpaste={onPaste}
        placeholder={busy ? 'Working…' : config.mode === 'research' ? `Research question for ${providerLabel}` : config.mode === 'study' ? `What do you want to learn? (${providerLabel})` : `Message ${providerLabel}`}
        disabled={missing}
      ></textarea>
      <div class="tools">
        <ModeMenu bind:mode={config.mode} bind:aiBrowser={config.aiBrowser} provider={config.provider} disabled={busy || missing} onchange={changeMode} onaddfiles={pickFiles} onbrowser={changeAiBrowser} />
        <button class="chip" onclick={pickFolder} disabled={busy} title={config.cwd ? `Project folder: ${config.cwd}` : 'Choose a project folder'}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
          <span>{folderName || 'Choose folder'}</span>
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
        </button>
        <ModelPicker
          provider={config.provider}
          accountId={config.accountId}
          bind:model={config.model}
          bind:effort={config.effort}
          disabled={busy || missing}
          onchange={changeModel}
        />
        <span class="spacer"></span>
        <button class="send" onclick={send} disabled={busy || (!input.trim() && !attachments.length) || missing} aria-label="Send">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 19V5M6 11l6-6 6 6" /></svg>
        </button>
      </div>
    </div>
  </footer>
  <div class="usage">
    {fmt(totals.input)} in · {fmt(totals.output)} out
    {#if totals.hasCost}· ≈ ${totals.cost < 0.01 && totals.cost > 0 ? totals.cost.toFixed(4) : totals.cost.toFixed(2)}
      <span class="muted">(API-price estimate{totals.unpriced ? '; some turns had no known price' : ''})</span>
    {:else if totals.unpriced}· <span class="muted">cost unknown for {pricedModel || 'this model'}</span>{/if}
    {#if config.provider !== 'antigravity' && mainWindows.length}
      <button
        class="limmeter"
        onclick={() => refreshLimits(config.accountId, config.provider)}
        title={`Plan limits of "${account?.name ?? ''}" from ${providerLabel}. Click to refresh.\n` + mainWindows.map((w) => `${w.label}: ${Math.round(usedNow(w, now))}% used${w.resets_at ? ', ' + resetText(w.resets_at, now) : ''}`).join('\n')}
      >
        {#each mainWindows as w (w.key)}
          {@const pct = usedNow(w, now)}
          {@const lv = levelOf(pct)}
          <span class="lm">
            {w.key === 'five_hour' ? '5h' : 'week'}
            <span class={`mini ${lv}`} style={`--fill: var(--series-${config.provider})`}><span style={`width: ${Math.min(100, pct)}%`}></span></span>
            {Math.round(pct)}%{lv === 'critical' ? ' ⛔' : lv === 'warn' ? ' ⚠' : ''}
          </span>
        {/each}
      </button>
    {/if}
  </div>
  </div>
  {#if browserOpen}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="vsplit"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the browser"
      aria-valuenow={Math.round(browserShare * 100)}
      tabindex="0"
      onpointerdown={dragDivider}
      onkeydown={nudgeDivider}
    ></div>
    <div class="bpane" style:flex-basis={`${browserShare * 100}%`}>
      <BrowserPanel bind:this={browser} {paneId} onclose={() => (browserOpen = false)} />
    </div>
  {/if}
  </div>
  {#if viewing}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="lightbox" onclick={() => (viewing = null)}>
      <img src={viewing} alt="Attached" />
      <button class="ghost" onclick={() => (viewing = null)}>Close</button>
    </div>
  {/if}
</section>

<svelte:window onkeydown={(e) => { if (e.key === 'Escape' && viewing) viewing = null; }} />

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }
  .pane.active {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  .bar {
    display: flex;
    flex-wrap: nowrap;
    min-width: 0;
    align-items: center;
    gap: 6px;
    padding: 8px;
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
  }
  .spacer { flex: 1; }
  .bar select { min-width: 64px; flex: 0 1 auto; text-overflow: ellipsis; }
  .bar button { flex-shrink: 0; }
  .bar .close { padding: 4px 6px; display: inline-flex; align-items: center; justify-content: center; color: var(--muted); }
  .bar .close:hover { color: var(--text); }
  select.danger { color: var(--danger); border-color: var(--danger); }
  .model { font-size: 12px; color: var(--muted); font-family: var(--mono); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .banner { margin: 0; padding: 8px 12px; background: var(--warn-bg); color: var(--text); font-size: 13px; }
  /* The chat, and the browser panel next to it when open. */
  .body { flex: 1; min-height: 0; display: flex; }
  .chat { flex: 1 1 0; min-width: 240px; display: flex; flex-direction: column; min-height: 0; }
  .bpane { flex: 0 0 50%; min-width: 240px; display: flex; min-height: 0; border-left: 1px solid var(--border); }
  .vsplit { flex: 0 0 6px; margin: 0 -3px; cursor: col-resize; z-index: 1; position: relative; }
  .vsplit:hover, .vsplit:focus-visible { background: color-mix(in oklab, var(--accent-fill) 40%, transparent); outline: none; }
  .browserbtn { display: inline-flex; align-items: center; gap: 5px; }
  .browserbtn.on { color: var(--accent); border-color: var(--accent); }
  .visual { display: flex; align-items: center; gap: 10px; border: 1px solid var(--border); border-radius: 10px; padding: 8px 10px 8px 12px; background: var(--surface-2); font-size: 13px; }
  .visual svg { color: var(--accent); flex-shrink: 0; }
  .vtext { display: flex; flex-direction: column; min-width: 0; flex: 1; }
  .vtext b { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .vtext small { color: var(--muted); font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .messages {
    position: relative;
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    display: flex;
    flex-direction: column;
  }
  /* The conversation stays in a centered column (max 5xl = 64rem), like Claude Desktop. */
  .col { width: 100%; max-width: 64rem; margin: 0 auto; display: flex; flex-direction: column; gap: 10px; }
  .turn { display: flex; flex-direction: column; gap: 10px; }
  .user.clamped { max-height: calc(1.5em * 8 + 16px); overflow: hidden; }
  .user.fade { mask-image: linear-gradient(to bottom, #000 calc(100% - 2.5em), transparent); -webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - 2.5em), transparent); }
  .more { align-self: flex-end; margin-top: -4px; border: 0; background: none; color: var(--muted); font: inherit; font-size: 12px; padding: 2px 4px; cursor: pointer; text-decoration: underline; text-underline-offset: 2px; }
  .more:hover { color: var(--text); }
  .md :global(.codeblock) { position: relative; }
  .md :global(.codeblock pre) { padding-right: 44px; }
  .md :global(.copybtn) {
    position: absolute; top: 6px; right: 6px;
    display: inline-flex; align-items: center; gap: 4px;
    min-width: 28px; min-height: 28px; padding: 0 7px; justify-content: center;
    border: 1px solid var(--border); border-radius: 6px;
    background: var(--bg); color: var(--muted); font: inherit; font-size: 12px; cursor: pointer;
    opacity: 0.85;
  }
  .md :global(.copybtn:hover), .md :global(.copybtn:focus-visible) { color: var(--text); opacity: 1; }
  .md :global(.copybtn.copied) { color: var(--ok); opacity: 1; }

  /* Cowork-style step group: one per turn. */
  .steps { align-self: stretch; display: flex; flex-direction: column; gap: 6px; }
  .shead {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    align-self: flex-start;
    background: none;
    border: 0;
    padding: 2px 0;
    color: var(--muted);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .shead:hover { color: var(--text); }
  .shead .verb { color: var(--text); font-weight: 500; }
  .shead .verb.done { color: var(--muted); font-weight: 400; }
  .shead:hover .verb.done { color: var(--text); }
  .stime { font-variant-numeric: tabular-nums; font-size: 12.5px; }
  .chev { transition: transform 0.15s ease; }
  .chev.open { transform: rotate(90deg); }
  .slist { border: 1px solid var(--border); border-radius: 10px; overflow: hidden; margin-left: 2px; }
  .srow + .srow, .srow + .snote, .snote + .srow, .snote + .snote { border-top: 1px solid var(--border); }
  .srow summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 12px;
    cursor: pointer;
    list-style: none;
    font-size: 13px;
    color: var(--muted);
    min-width: 0;
  }
  .srow summary::-webkit-details-marker { display: none; }
  .srow summary:hover { background: var(--surface-2); }
  .slabel { color: var(--muted); white-space: nowrap; }
  .ssubject { color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .sstate { font-size: 11.5px; color: var(--muted); white-space: nowrap; }
  .sstate.bad { color: var(--danger); }
  .srow.err .slabel { color: var(--danger); }
  .schev { flex-shrink: 0; transition: transform 0.15s ease; }
  .srow[open] .schev { transform: rotate(90deg); }
  .sbody { padding: 0 12px 10px; display: flex; flex-direction: column; gap: 6px; }
  .scmd, .sout { margin: 0; font-family: var(--mono); font-size: 12px; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 8px 10px; white-space: pre-wrap; word-break: break-word; max-height: 240px; overflow: auto; }
  .scmd { color: var(--text); }
  .sout { color: var(--muted); }
  .snone { margin: 0; color: var(--muted); font-size: 12px; }
  .snote { padding: 9px 12px; font-size: 13px; color: var(--muted); }
  .empty {
    margin: auto;
    text-align: center;
    color: var(--muted);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .empty p { margin: 0; }
  .hint { font-size: 12px; }
  .msg { max-width: 100%; overflow-wrap: anywhere; line-height: 1.5; }
  .user {
    align-self: flex-end;
    max-width: 85%;
    background: var(--user-bg);
    color: var(--user-text);
    padding: 8px 12px;
    border-radius: 14px 14px 4px 14px;
    white-space: pre-wrap;
  }
  .assistant { align-self: stretch; }
  .error { color: var(--danger); background: var(--danger-bg); padding: 8px 12px; border-radius: 8px; white-space: pre-wrap; font-size: 13px; }
  .info { color: var(--muted); font-size: 12px; text-align: center; }
  .approval { border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; background: var(--surface-2); display: flex; flex-direction: column; gap: 8px; font-size: 13px; }
  .approval.waiting { border-color: var(--accent-fill); box-shadow: 0 0 0 1px var(--accent-fill); }
  .ahead { display: flex; align-items: center; gap: 8px; color: var(--text); }
  .ahead svg { color: var(--accent); flex-shrink: 0; }
  .acmd { margin: 0; font-family: var(--mono); font-size: 12px; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 8px 10px; white-space: pre-wrap; word-break: break-word; max-height: 180px; overflow: auto; }
  .adetail { margin: 0; color: var(--muted); font-size: 12.5px; white-space: pre-wrap; }
  .aplan { max-height: 280px; overflow: auto; background: var(--bg); border: 1px solid var(--border); border-radius: 8px; padding: 8px 12px; }
  .abtns { display: flex; gap: 8px; flex-wrap: wrap; }
  .abtns button { min-height: 32px; }
  .astate { margin: 0; font-size: 12.5px; font-weight: 600; color: var(--muted); }
  .astate.allowed, .astate.allowed_session { color: var(--ok); }
  .astate.denied, .astate.kept { color: var(--danger); }
  .divider { display: flex; align-items: center; gap: 10px; color: var(--muted); font-size: 12px; margin: 4px 0; }
  .divider::before, .divider::after { content: ''; flex: 1; border-top: 1px solid var(--border); }
  .divider span { max-width: 80%; text-align: center; overflow-wrap: anywhere; }
  .notice { background: var(--warn-bg); color: var(--text); padding: 8px 12px; border-radius: 8px; font-size: 13px; }
  .composer { padding: 8px 8px 4px; border-top: 1px solid var(--border); }
  .composer .inputbox { max-width: 64rem; margin: 0 auto; }
  .inputbox.dragover { border-color: var(--accent); background: color-mix(in oklab, var(--accent-fill) 8%, var(--bg)); }
  .atts { display: flex; flex-wrap: wrap; gap: 8px; padding: 2px 0 4px; }
  .att { position: relative; }
  .attimg, .uthumb { padding: 0; border: 1px solid var(--border); border-radius: 8px; overflow: hidden; background: var(--surface-2); cursor: zoom-in; display: block; }
  .attimg img { width: 56px; height: 56px; object-fit: cover; display: block; }
  .attx {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    display: grid;
    place-items: center;
    padding: 0;
    cursor: pointer;
  }
  .uimgs { align-self: flex-end; display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 6px; max-width: 85%; }
  .uthumb img { width: 120px; height: 120px; object-fit: cover; display: block; }
  .fchip { display: inline-flex; align-items: center; gap: 8px; height: 56px; max-width: 220px; padding: 0 10px 0 6px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface-2); }
  .fchip.sent { height: 48px; background: var(--surface); }
  .ftag { flex-shrink: 0; min-width: 38px; height: 38px; padding: 0 4px; border-radius: 6px; background: var(--accent-fill); color: #17181c; font-size: 10px; font-weight: 700; display: grid; place-items: center; letter-spacing: 0.02em; }
  .fmeta { display: flex; flex-direction: column; min-width: 0; }
  .fname { font-size: 12.5px; color: var(--text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .fsize { font-size: 11px; color: var(--muted); }
  .uthumb.missing { width: 120px; height: 40px; display: grid; place-items: center; font-size: 11px; color: var(--muted); cursor: default; }
  .lightbox { position: fixed; inset: 0; z-index: 50; background: rgb(0 0 0 / 0.75); display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 12px; padding: 24px; }
  .lightbox img { max-width: 100%; max-height: calc(100% - 60px); border-radius: 8px; box-shadow: 0 10px 40px rgb(0 0 0 / 0.5); }
  .inputbox {
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--bg);
    padding: 8px 8px 6px 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .inputbox:focus-within { border-color: var(--accent); }
  textarea {
    resize: none;
    font: inherit;
    color: var(--text);
    background: transparent;
    border: 0;
    outline: none;
    padding: 2px 0;
    min-height: 40px;
  }
  .tools { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 70%;
    min-height: 30px;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 12.5px;
    cursor: pointer;
  }
  .chip span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip:hover:not(:disabled) { border-color: var(--accent); }
  .send {
    width: 34px;
    height: 34px;
    border: 0;
    border-radius: 50%;
    background: var(--accent-fill);
    color: #17181c;
    display: grid;
    place-items: center;
    cursor: pointer;
  }
  .send:disabled { opacity: 0.4; cursor: default; }
  .linklike { background: none; border: 0; color: var(--accent); text-decoration: underline; cursor: pointer; padding: 0 4px; font: inherit; }
  .banner.limit { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  select.account { max-width: 220px; }
  select.github { max-width: 200px; }
  .usage { width: 100%; max-width: calc(64rem + 24px); margin: 0 auto; padding: 0 12px 8px; font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; display: flex; flex-wrap: wrap; align-items: center; gap: 0 4px; }
  .limmeter { margin-left: auto; display: inline-flex; gap: 10px; background: none; border: 0; padding: 0; color: var(--muted); font: inherit; font-size: 11px; cursor: pointer; }
  .limmeter:hover { color: var(--text); }
  .lm { display: inline-flex; align-items: center; gap: 4px; }
  .mini { --c: var(--fill); width: 36px; height: 6px; border-radius: 3px; background: color-mix(in oklab, var(--c) 22%, var(--surface)); overflow: hidden; display: inline-block; }
  .mini.warn { --c: var(--status-warning); }
  .mini.critical { --c: var(--status-critical); }
  .mini > span { display: block; height: 100%; border-radius: 3px; background: var(--c); }
  .banner.critical { background: var(--danger-bg); }
  .muted { color: var(--muted); }
  .md :global(pre) { background: var(--surface-2); border: 1px solid var(--border); border-radius: 8px; padding: 10px; overflow-x: auto; }
  .md :global(code) { font-family: var(--mono); font-size: 0.9em; }
  .md :global(:not(pre) > code) { background: var(--surface-2); padding: 1px 4px; border-radius: 4px; }
  .md :global(p) { margin: 0 0 0.6em; }
  .md :global(p:last-child) { margin-bottom: 0; }
  .md :global(a) { color: var(--accent); cursor: default; }
  .md :global(table) { border-collapse: collapse; display: block; overflow-x: auto; }
  .md :global(th), .md :global(td) { border: 1px solid var(--border); padding: 4px 8px; }
</style>
