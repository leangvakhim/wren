import { invoke, Channel } from '@tauri-apps/api/core';
import type { ModelInfo, Msg, SentFile } from './types';
import type { NewTask, PanePatch, PhoneApp, PhoneMsg, PhonePaneMeta, PhoneRequest, Uploaded } from './remoteTypes';

/** Phone link state from Rust (src-tauri/src/remote/mod.rs). */
export interface RemoteDevice {
  id: string;
  name: string;
  added_at: number;
  last_seen: number;
  connected: boolean;
  push: boolean;
}

export interface RemoteView {
  enabled: boolean;
  relay: string;
  has_key: boolean;
  keep_awake: boolean;
  status: 'off' | 'connecting' | 'online' | 'offline' | 'error';
  error: string | null;
  host_name: string;
  devices: RemoteDevice[];
  pairing_until: number | null;
}

export interface PairCode {
  url: string;
  svg: string;
  expires_at: number;
}

type RemoteEvent =
  | { kind: 'status'; view: RemoteView }
  | { kind: 'connected'; conn: number; device: string; name: string }
  | { kind: 'disconnected'; conn: number }
  | { kind: 'request'; conn: number; device: string; msg: { t?: string; id?: number } & PhoneRequest }
  | { kind: 'pair_request'; conn: number; name: string };

export const remote = $state<{
  view: RemoteView | null;
  code: PairCode | null;
  /** New phones waiting for "Allow" on this computer. */
  asks: { conn: number; name: string }[];
  /** Phones connected right now. */
  phones: number;
}>({ view: null, code: null, asks: [], phones: 0 });

/** What a pane offers the phone link (registered by Pane.svelte). */
export interface PaneRemote {
  slot: number;
  meta(): Omit<PhonePaneMeta, 'pos'>;
  messages(): Msg[];
  isEmpty(): boolean;
  opened(): Promise<void>;
  send(text: string, files: SentFile[]): Promise<void>;
  stop(): Promise<void>;
  approve(msgId: number, decision: 'allow' | 'allow_session' | 'deny' | 'retry' | 'keep'): Promise<void>;
  answer(msgId: number, answers: Record<string, string[]> | null): Promise<void>;
  newChat(): Promise<void>;
  config(patch: PanePatch): Promise<void>;
  limit(action: 'switch' | 'continue' | 'cancel' | 'dismiss', accountId?: string): Promise<void>;
  openConversation(id: string): Promise<void>;
  /** Start fresh with this AI / account / folder / settings. */
  prepare(cfg: NewTask): Promise<void>;
  close(): Promise<void>;
}

/** What the window offers (registered by +page.svelte). */
export interface WindowRemote {
  app(): Omit<PhoneApp, 't' | 'panes' | 'canAdd'>;
  /** On-screen position of a slot (-1 = not shown). */
  position(slot: number): number;
  /** Slots on screen, in order. */
  visible(): number[];
  /** Show one more pane (throws when all 4 are open); returns its slot. */
  addPane(): Promise<number>;
}

const panes = new Map<number, PaneRemote>();
let win: WindowRemote | null = null;

export function registerPane(h: PaneRemote) {
  panes.set(h.slot, h);
  paneChanged(h.slot);
}

export function unregisterPane(slot: number) {
  panes.delete(slot);
  scheduleApp();
}

interface Conn {
  device: string;
  /** The pane the phone has open. */
  watch: number | null;
  watchConv: string | null;
  /** Last JSON sent per message id of the watched pane. */
  sent: Map<number, string>;
  lastApp: string;
}

const conns = new Map<number, Conn>();

const MAX_TEXT = 60_000;
const MAX_TOOL = 4_000;
const MAX_MSGS = 150;
const BATCH = 250_000;

const cut = (s: string | undefined, n: number) => (s === undefined ? undefined : s.length > n ? s.slice(0, n) + '\n… (shortened on the phone)' : s);

function toPhone(m: Msg): PhoneMsg {
  const files = (m.files ?? (m.images ?? []).map((p) => ({ path: p, name: p.split(/[\\/]/).pop() ?? 'image', size: 0, kind: 'image' as const }))).map((f) => ({
    name: f.name,
    size: f.size,
    kind: f.kind,
  }));
  const out: PhoneMsg = { id: m.id, role: m.role, text: cut(m.text, MAX_TEXT) ?? '' };
  if (m.detail !== undefined) out.detail = cut(m.detail, m.role === 'approval' ? MAX_TEXT : MAX_TOOL);
  if (m.result !== undefined) out.result = cut(m.result, MAX_TOOL);
  if (m.isError) out.isError = true;
  if (files.length) out.files = files;
  for (const k of ['durationMs', 'step', 'tool', 'approvalState', 'autoBy', 'allowSession', 'questions', 'answers', 'questionState'] as const) {
    if (m[k] !== undefined) (out as unknown as Record<string, unknown>)[k] = $state.snapshot(m[k]);
  }
  return out;
}

function sendTo(conn: number | null, msg: unknown) {
  return invoke('remote_send', { conn, msg }).catch(() => {});
}

function paneMeta(slot: number): PhonePaneMeta | null {
  const h = panes.get(slot);
  if (!h || !win) return null;
  const pos = win.position(slot);
  if (pos < 0) return null;
  return { ...h.meta(), pos };
}

function sendPane(conn: number, c: Conn, slot: number, reset = false) {
  const h = panes.get(slot);
  const meta = paneMeta(slot);
  if (!h || !meta) return;
  if (meta.convId !== c.watchConv) {
    reset = true;
    c.watchConv = meta.convId;
  }
  if (reset) c.sent.clear();
  const msgs = h
    .messages()
    .filter((m) => m.role !== 'details')
    .slice(-MAX_MSGS)
    .map(toPhone);
  const upd: PhoneMsg[] = [];
  for (const m of msgs) {
    const j = JSON.stringify(m);
    if (c.sent.get(m.id) === j) continue;
    c.sent.set(m.id, j);
    upd.push(m);
  }
  const ids = msgs.map((m) => m.id);
  // Several frames if a lot changed (the link carries ≤ 1 MB per message).
  const batches: PhoneMsg[][] = [[]];
  let size = 0;
  for (const m of upd) {
    const n = c.sent.get(m.id)!.length;
    if (size + n > BATCH && batches[batches.length - 1].length) {
      batches.push([]);
      size = 0;
    }
    batches[batches.length - 1].push(m);
    size += n;
  }
  batches.forEach((b, i) => sendTo(conn, { t: 'pane', slot, meta, ids, upd: b, reset: reset && i === 0 }));
}

function appState(): PhoneApp | null {
  if (!win) return null;
  const list = win
    .visible()
    .map((s) => paneMeta(s))
    .filter((m): m is PhonePaneMeta => !!m);
  return { t: 'app', panes: list, canAdd: list.length < 4 || list.some((m) => panes.get(m.slot)?.isEmpty()), ...win.app() };
}

function sendApp(force = false) {
  const app = appState();
  if (!app) return;
  const j = JSON.stringify(app);
  for (const [conn, c] of conns) {
    if (!force && c.lastApp === j) continue;
    c.lastApp = j;
    sendTo(conn, app);
  }
}

// ---- change tracking (only while a phone is connected) ----
const dirty = new Set<number>();
let flushTimer: ReturnType<typeof setTimeout> | undefined;
let appTimer: ReturnType<typeof setTimeout> | undefined;

/** A pane's chat or settings changed. */
export function paneChanged(slot: number) {
  if (!conns.size) return;
  dirty.add(slot);
  flushTimer ??= setTimeout(() => {
    flushTimer = undefined;
    for (const slot of dirty) {
      for (const [conn, c] of conns) if (c.watch === slot) sendPane(conn, c, slot);
    }
    dirty.clear();
  }, 250);
  scheduleApp();
}

/** Something on the home screen changed (panes, accounts, history). */
export function scheduleApp() {
  if (!conns.size) return;
  appTimer ??= setTimeout(() => {
    appTimer = undefined;
    sendApp();
  }, 400);
}

// ---- requests from phones ----

/** A pane for new work: an empty one on screen, or a new one. */
async function freePane(): Promise<PaneRemote> {
  if (!win) throw new Error('Wren is starting.');
  for (const s of win.visible()) {
    const h = panes.get(s);
    if (h?.isEmpty()) return h;
  }
  const slot = await win.addPane();
  // The new pane registers itself when it mounts.
  for (let i = 0; i < 50 && !panes.has(slot); i++) await new Promise((r) => setTimeout(r, 40));
  const h = pane(slot);
  await Promise.race([h.opened(), new Promise((r) => setTimeout(r, 10_000))]);
  return h;
}

function pane(slot: number): PaneRemote {
  const h = panes.get(slot);
  if (!h || !win || win.position(slot) < 0) throw new Error('That pane is no longer open on the computer.');
  return h;
}

const asSent = (files: Uploaded[] | undefined): SentFile[] =>
  (files ?? []).slice(0, 10).map((f) => ({ path: String(f.path), name: String(f.name), size: Number(f.size) || 0, mime: String(f.mime ?? ''), kind: f.kind === 'image' ? 'image' : 'file' }));

async function run(conn: number, c: Conn, r: PhoneRequest): Promise<unknown> {
  switch (r.op) {
    case 'watch':
      c.watch = r.slot;
      c.watchConv = null;
      if (r.slot !== null) sendPane(conn, c, r.slot, true);
      return null;
    case 'send':
      return pane(r.slot).send(String(r.text ?? ''), asSent(r.files));
    case 'stop':
      return pane(r.slot).stop();
    case 'approve':
      return pane(r.slot).approve(r.msg, r.decision);
    case 'answer':
      return pane(r.slot).answer(r.msg, r.answers);
    case 'new_chat':
      return pane(r.slot).newChat();
    case 'close': {
      const h = pane(r.slot);
      if (win!.visible().length <= 1) throw new Error("The computer's last pane can't be closed. Use New chat instead.");
      return h.close();
    }
    case 'config':
      return pane(r.slot).config(r.patch ?? {});
    case 'limit':
      return pane(r.slot).limit(r.action, r.accountId);
    case 'new_task': {
      const h = await freePane();
      await h.prepare(r.config);
      if (r.text.trim() || r.files?.length) await h.send(r.text.trim(), asSent(r.files));
      return { slot: h.slot };
    }
    case 'open_chat': {
      if (!win) throw new Error('Wren is starting.');
      // Already open somewhere: just show it.
      for (const s of win.visible()) if (panes.get(s)?.meta().convId === r.convId) return { slot: s };
      const h = await freePane();
      await h.openConversation(r.convId);
      return { slot: h.slot };
    }
    case 'models':
      return invoke<ModelInfo[]>('list_models', { provider: r.provider, accountId: r.accountId });
    default:
      throw new Error('Wren on the computer is older than this phone page. Update Wren.');
  }
}

async function onRequest(conn: number, msg: { id?: number } & PhoneRequest) {
  const c = conns.get(conn);
  if (!c) return;
  try {
    const data = await run(conn, c, msg);
    sendTo(conn, { t: 'res', id: msg.id, ok: true, data: data ?? null });
  } catch (e) {
    sendTo(conn, { t: 'res', id: msg.id, ok: false, error: e instanceof Error ? e.message : String(e) });
  }
  // Most actions change what the phone sees.
  if (c.watch !== null) paneChanged(c.watch);
  scheduleApp();
}

function onEvent(ev: RemoteEvent) {
  switch (ev.kind) {
    case 'status':
      remote.view = ev.view;
      if (!ev.view.pairing_until) remote.code = null;
      break;
    case 'connected':
      conns.set(ev.conn, { device: ev.device, watch: null, watchConv: null, sent: new Map(), lastApp: '' });
      remote.phones = conns.size;
      remote.asks = remote.asks.filter((a) => a.conn !== ev.conn);
      // A phone paired: the QR code is used up.
      remote.code = null;
      sendApp(true);
      break;
    case 'disconnected':
      conns.delete(ev.conn);
      remote.phones = conns.size;
      remote.asks = remote.asks.filter((a) => a.conn !== ev.conn);
      break;
    case 'request':
      onRequest(ev.conn, ev.msg);
      break;
    case 'pair_request':
      remote.asks = [...remote.asks.filter((a) => a.conn !== ev.conn), { conn: ev.conn, name: ev.name }];
      break;
  }
}

export async function startRemote(w: WindowRemote) {
  win = w;
  const ch = new Channel<RemoteEvent>();
  ch.onmessage = onEvent;
  try {
    remote.view = await invoke<RemoteView>('remote_attach', { onEvent: ch });
  } catch {}
}

export async function configureRemote(enabled: boolean, relay: string, accessKey: string | null, keepAwake: boolean) {
  remote.view = await invoke<RemoteView>('remote_configure', { enabled, relay, accessKey, keepAwake });
}

export async function pairPhone() {
  remote.code = await invoke<PairCode>('remote_pair');
}

export async function cancelPairing() {
  remote.code = null;
  await invoke('remote_cancel_pair').catch(() => {});
}

export async function answerPhone(conn: number, allow: boolean) {
  remote.asks = remote.asks.filter((a) => a.conn !== conn);
  await invoke('remote_confirm', { conn, allow });
}

export const removePhone = (id: string) => invoke('remote_remove_device', { id });
export const renamePhone = (id: string, name: string) => invoke('remote_rename_device', { id, name });

/** Push to paired phones (each phone picks the kinds it wants). */
export function pushToPhones(kind: 'approval' | 'question' | 'done' | 'problems', title: string, body: string, slot: number) {
  if (!remote.view?.enabled || !remote.view.devices.some((d) => d.push)) return;
  invoke('remote_push', { kind, title, body: body.slice(0, 240), tag: `pane-${slot}` }).catch(() => {});
}
