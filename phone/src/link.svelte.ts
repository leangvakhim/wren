import { b64, connect, newDeviceKey, parsePairFragment, random, unb64, type PairInfo } from './protocol';
import { forgetPairing, loadPairing, savePairing, type Pairing } from './store';
import type { FromWren, PhoneApp, PhoneMsg, PhonePaneMeta, PhoneRequest, Uploaded } from '../../src/lib/remoteTypes';

export type Stage =
  | 'loading'
  /** Not paired, no pairing link. */
  | 'unpaired'
  /** A pairing link was opened: ask before pairing. */
  | 'offer'
  | 'connecting'
  /** Waiting for "Allow" on the computer. */
  | 'waiting'
  | 'ready'
  | 'offline'
  | 'denied';

export interface PaneState {
  meta: PhonePaneMeta;
  ids: number[];
  byId: Record<number, PhoneMsg>;
}

export const link = $state<{
  stage: Stage;
  why: string;
  host: string;
  version: string;
  vapid: string;
  pushKinds: string[];
  app: PhoneApp | null;
  panes: Record<number, PaneState>;
  offer: PairInfo | null;
  /** Wren answered at least once on this connection. */
  everReady: boolean;
}>({ stage: 'loading', why: '', host: '', version: '', vapid: '', pushKinds: [], app: null, panes: {}, offer: null, everReady: false });

let pairing: Pairing | null = null;
let current: ReturnType<typeof connect> | null = null;
let nextId = 1;
const waiting = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
let watching: number | null = null;
let wake: (() => void) | null = null;
let running = false;

const socketUrl = (room: string) => {
  const u = new URL(`./phone?room=${room}`, location.href);
  u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:';
  u.hash = '';
  return u.href;
};

export function deviceName(): string {
  const ua = navigator.userAgent;
  const device = /iPhone/.test(ua) ? 'iPhone' : /iPad/.test(ua) ? 'iPad' : /Android/.test(ua) ? 'Android phone' : 'Phone';
  const browser = /CriOS|Chrome/.test(ua) ? 'Chrome' : /FxiOS|Firefox/.test(ua) ? 'Firefox' : /Safari/.test(ua) ? 'Safari' : '';
  return browser ? `${device} · ${browser}` : device;
}

export const standalone = () => matchMedia('(display-mode: standalone)').matches || (navigator as { standalone?: boolean }).standalone === true;
export const isIOS = () => /iPhone|iPad|iPod/.test(navigator.userAgent);

export async function boot() {
  const offer = parsePairFragment(location.hash);
  pairing = (await loadPairing()) ?? null;
  if (offer && (!pairing || pairing.room !== offer.room || pairing.hostKey !== offer.hostKey)) {
    link.offer = offer;
    link.stage = 'offer';
    return;
  }
  if (offer) history.replaceState(null, '', location.pathname);
  if (pairing) {
    link.host = pairing.host;
    loop();
  } else {
    link.stage = 'unpaired';
  }
}

/** "Paste the pairing link" (for a Home Screen app, which can't open the QR link itself). */
export function usePairLink(text: string): boolean {
  const i = text.indexOf('#');
  const offer = i >= 0 ? parsePairFragment(text.slice(i)) : null;
  if (!offer) return false;
  link.offer = offer;
  link.stage = 'offer';
  return true;
}

export async function pair(name: string) {
  const offer = link.offer;
  if (!offer) return;
  const key = await newDeviceKey();
  const dev = 'ph-' + b64(random(12));
  pairing = { room: offer.room, hostKey: offer.hostKey, dev, key, host: '', name: name.trim() || deviceName() };
  history.replaceState(null, '', location.pathname);
  loop(offer.secret);
}

export async function unpair() {
  running = false;
  current?.close();
  await forgetPairing();
  pairing = null;
  link.stage = 'unpaired';
  link.app = null;
  link.panes = {};
  link.everReady = false;
}

/** Keep a connection to Wren, reconnecting after drops (also when the page comes back). */
async function loop(secret?: string) {
  if (running) return;
  running = true;
  let wait = 1000;
  let pairSecret = secret;
  while (running && pairing) {
    const p = pairing;
    let denied = false;
    current = connect({
      url: socketUrl(p.room),
      hostKey: p.hostKey,
      dev: p.dev,
      key: p.key,
      pair: pairSecret ? { secret: pairSecret, name: p.name } : undefined,
      onstate: (s, why) => {
        if (s === 'denied') denied = true;
        if (s === 'offline' && link.stage === 'denied') return;
        link.stage = s === 'connecting' && link.everReady ? 'offline' : s;
        link.why = why ?? '';
        if (s === 'ready') wait = 1000;
      },
      onmessage: onMessage,
    });
    await current.done;
    current = null;
    for (const [, w] of waiting) {
      clearTimeout(w.timer);
      w.reject(new Error('The connection to Wren dropped. Try again.'));
    }
    waiting.clear();
    if (denied) {
      // Refused, removed on the computer, or an old QR code: start over.
      running = false;
      link.stage = 'denied';
      if (!pairSecret) await forgetPairing();
      pairing = null;
      return;
    }
    // Paired: later connections don't need (or have) the secret.
    if (link.everReady) pairSecret = undefined;
    if (!running) return;
    link.stage = 'offline';
    await new Promise<void>((r) => {
      const t = setTimeout(r, wait);
      wake = () => {
        clearTimeout(t);
        r();
      };
    });
    wake = null;
    wait = Math.min(wait * 2, 30_000);
  }
  running = false;
}

/** Try again now (the page came back to the front, or the network returned). */
export function reconnectNow() {
  wake?.();
}

async function onMessage(raw: Record<string, unknown>) {
  const m = raw as unknown as FromWren;
  switch (m.t) {
    case 'ready':
      link.host = m.host;
      link.version = m.version;
      link.vapid = m.vapid;
      link.pushKinds = m.push_kinds ?? [];
      link.everReady = true;
      if (pairing && (!pairing.host || pairing.host !== m.host)) {
        pairing.host = m.host;
        await savePairing(pairing);
      }
      sendVisible();
      if (watching !== null) request({ op: 'watch', slot: watching }).catch(() => {});
      break;
    case 'app':
      link.app = m;
      for (const k of Object.keys(link.panes)) if (!m.panes.some((p) => p.slot === Number(k))) delete link.panes[Number(k)];
      for (const p of m.panes) if (link.panes[p.slot]) link.panes[p.slot].meta = p;
      break;
    case 'pane': {
      let ps = link.panes[m.slot];
      if (!ps || m.reset) ps = link.panes[m.slot] = { meta: m.meta, ids: [], byId: {} };
      ps.meta = m.meta;
      for (const u of m.upd) ps.byId[u.id] = u;
      ps.ids = m.ids;
      const keep = new Set(m.ids);
      for (const k of Object.keys(ps.byId)) if (!keep.has(Number(k))) delete ps.byId[Number(k)];
      break;
    }
    case 'res': {
      const w = waiting.get(m.id);
      if (!w) break;
      waiting.delete(m.id);
      clearTimeout(w.timer);
      if (m.ok) w.resolve(m.data);
      else w.reject(new Error(m.error ?? 'Wren could not do that.'));
      break;
    }
  }
}

/** Ask Wren to do something; resolves with its answer. */
export function request<T = unknown>(r: PhoneRequest | { op: string; [k: string]: unknown }, timeoutMs = 60_000): Promise<T> {
  if (!current || link.stage !== 'ready') return Promise.reject(new Error('Not connected to Wren.'));
  const id = nextId++;
  const p = new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      waiting.delete(id);
      reject(new Error("Wren didn't answer in time."));
    }, timeoutMs);
    waiting.set(id, { resolve: resolve as (v: unknown) => void, reject, timer });
  });
  current.send({ t: 'req', id, ...r }).catch((e) => {
    const w = waiting.get(id);
    waiting.delete(id);
    if (w) {
      clearTimeout(w.timer);
      w.reject(e instanceof Error ? e : new Error(String(e)));
    }
  });
  return p;
}

/** The pane on screen (Wren sends its chat while it's watched). */
export function watch(slot: number | null) {
  watching = slot;
  if (link.stage === 'ready') request({ op: 'watch', slot }).catch(() => {});
}

/** Tell Wren whether this page is on screen (no push needed then). */
export function sendVisible() {
  if (link.stage !== 'ready' || !current) return;
  current.send({ t: 'req', id: 0, op: 'visible', on: document.visibilityState === 'visible' }).catch(() => {});
}

// ---- files from the phone ----

const CHUNK = 192 * 1024;

export async function upload(file: File, progress?: (done: number) => void): Promise<Uploaded> {
  const u = b64(random(9));
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (!bytes.length) throw new Error(`"${file.name}" is empty.`);
  let res: unknown = null;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    const end = Math.min(bytes.length, i + CHUNK);
    res = await request({ op: 'upload', u, name: file.name || 'photo.jpg', size: bytes.length, last: end === bytes.length, data: b64(bytes.subarray(i, end)) }, 120_000);
    progress?.(end / bytes.length);
  }
  return res as Uploaded;
}

// ---- push notifications ----

export const pushSupported = () => 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window;

export async function setPush(kinds: string[]) {
  const reg = await navigator.serviceWorker.ready;
  let sub = await reg.pushManager.getSubscription();
  if (!kinds.length) {
    await request({ op: 'push', sub: null, kinds: [] });
    await sub?.unsubscribe();
    link.pushKinds = [];
    return;
  }
  if ((await Notification.requestPermission()) !== 'granted') {
    throw new Error('Notifications are blocked for this page. Allow them in the phone settings.');
  }
  const key = unb64(link.vapid);
  // A subscription made for another Wren (another key) can't be reused.
  const old = sub?.options.applicationServerKey;
  if (sub && old && b64(new Uint8Array(old)) !== link.vapid) {
    await sub.unsubscribe();
    sub = null;
  }
  sub ??= await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
  await request({ op: 'push', sub: sub.toJSON(), kinds });
  link.pushKinds = kinds;
}
