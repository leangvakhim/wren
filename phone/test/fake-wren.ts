/**
 * A stand-in for Wren's desktop side (encryption like src-tauri/src/remote),
 * with sample panes, to check the phone page in a browser:
 *   node --experimental-strip-types phone/test/fake-wren.ts <relay base> <access key>
 * Prints the pairing link, then one line per phone request.
 */
import { b64, pairProof, random, unb64 } from '../src/protocol.ts';

const [relay, key] = process.argv.slice(2);
const enc = new TextEncoder();
const dec = new TextDecoder();

const host = (await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, true, ['deriveBits'])) as CryptoKeyPair;
const hostPub = new Uint8Array(await crypto.subtle.exportKey('raw', host.publicKey));
const room = b64(random(16));
const secret = random(32);

class Box {
  n = 0n;
  key: CryptoKey;
  constructor(k: CryptoKey) {
    this.key = k;
  }
  iv() {
    const iv = new Uint8Array(12);
    new DataView(iv.buffer).setBigUint64(4, this.n++);
    return iv;
  }
  async seal(t: string) {
    return b64(new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv: this.iv() }, this.key, enc.encode(t))));
  }
  async open(s: string) {
    return dec.decode(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: this.iv() }, this.key, unb64(s)));
  }
}

const conns = new Map<number, { send: Box; recv: Box; q: Promise<unknown> }>();
const ws = new WebSocket(relay.replace(/^http/, 'ws') + '/host');
const raw = (c: number, d: unknown) => ws.send(JSON.stringify({ t: 'msg', c, d: JSON.stringify(d) }));
async function sendApp(c: number, m: unknown) {
  const s = conns.get(c);
  if (!s) return;
  const p = s.q.then(async () => raw(c, { t: 'e', d: await s.send.seal(JSON.stringify(m)) }));
  s.q = p;
  await p;
}

// ---- sample state ----
const now = Date.now();
const accounts = [
  { id: 'default-claude', provider: 'claude', name: 'Personal', usable: true },
  { id: 'claude-work', provider: 'claude', name: 'Work', usable: true },
  { id: 'default-codex', provider: 'codex', name: 'Codex', usable: true },
];
type M = Record<string, unknown> & { id: number };
const panes: Record<number, { meta: Record<string, unknown>; msgs: M[] }> = {
  0: {
    meta: {
      slot: 0, pos: 0, provider: 'claude', accountId: 'default-claude', accountName: 'Personal', permission: 'manual', mode: null, model: '', effort: '',
      modelLabel: 'Opus 5.5', cwd: '/Users/me/Projects/shop', title: 'Fix the failing checkout test', convId: 'c1', busy: true, startedAt: now - 83_000,
      thinking: false, waitingFor: 'approval', waitingLimit: null, limitHit: false, problem: null, totals: { input: 1200, output: 300, cost: 0.02, hasCost: true },
    },
    msgs: [
      { id: 1, role: 'user', text: 'Fix the failing checkout test' },
      { id: 2, role: 'tool', text: 'Read', detail: 'src/checkout.test.ts', result: '…' },
      { id: 3, role: 'tool', text: 'Bash', detail: 'npm test -- checkout', result: '1 failed', isError: true },
      { id: 4, role: 'assistant', text: 'The total is rounded before tax. Let me run the fix.' },
      { id: 5, role: 'approval', tool: 'Bash', text: 'npm test -- checkout --update', detail: 'Runs the checkout tests', approvalState: 'waiting', allowSession: true },
    ],
  },
  1: {
    meta: {
      slot: 1, pos: 1, provider: 'codex', accountId: 'default-codex', accountName: 'Codex', permission: 'auto', mode: null, model: '', effort: '',
      modelLabel: 'GPT-6.2-Sol', cwd: '/Users/me/Projects/blog', title: 'Add dark mode', convId: 'c2', busy: false, startedAt: 0, thinking: false,
      waitingFor: 'question', waitingLimit: null, limitHit: false, problem: null, totals: { input: 0, output: 0, cost: 0, hasCost: false },
    },
    msgs: [
      { id: 1, role: 'user', text: 'Add dark mode' },
      { id: 2, role: 'tool', text: 'exec_command', detail: 'ls src', result: 'app.css\nmain.ts' },
      { id: 3, role: 'assistant', text: 'Here is the plan:\n\n```css\n:root[data-theme="dark"] { --bg: #111; }\n```\n\nIt follows the **system** setting.', durationMs: 42_000 },
      {
        id: 4, role: 'question', tool: 'Codex', text: '', questionState: 'waiting',
        questions: [{ id: 'q1', header: 'Toggle', question: 'Where should the theme switch go?', options: [{ label: 'Header', description: 'Top right' }, { label: 'Settings page', description: null }], multi: false, other: true, secret: false }],
      },
    ],
  },
};
const history = [
  { id: 'c1', title: 'Fix the failing checkout test', provider: 'claude', accountId: 'default-claude', cwd: '/Users/me/Projects/shop', createdAt: now - 9e5, updatedAt: now - 6e4 },
  { id: 'c2', title: 'Add dark mode', provider: 'codex', accountId: 'default-codex', cwd: '/Users/me/Projects/blog', createdAt: now - 4e6, updatedAt: now - 3.6e6 },
  { id: 'c3', title: 'Explain quicksort', provider: 'antigravity', accountId: 'default-agy', cwd: '/Users/me/Desktop', createdAt: now - 9e7, updatedAt: now - 8.6e7 },
];
const app = () => ({
  t: 'app', panes: Object.values(panes).map((p) => p.meta), canAdd: true, accounts,
  installed: { claude: true, codex: true, antigravity: false }, folders: ['/Users/me/Projects/shop', '/Users/me/Projects/blog', '/Users/me/Desktop'],
  defaultFolder: '/Users/me/Desktop', history,
});
const pane = (slot: number, reset = false) => ({ t: 'pane', slot, meta: panes[slot].meta, ids: panes[slot].msgs.map((m) => m.id), upd: panes[slot].msgs, reset });
const watching = new Map<number, number | null>();

async function onReq(c: number, r: Record<string, unknown>) {
  console.log('REQ ' + JSON.stringify(r));
  const reply = (data: unknown = null) => sendApp(c, { t: 'res', id: r.id, ok: true, data });
  const slot = r.slot as number;
  switch (r.op) {
    case 'visible':
      return;
    case 'watch':
      watching.set(c, slot);
      if (slot !== null) await sendApp(c, pane(slot, true));
      return reply();
    case 'approve': {
      const m = panes[slot].msgs.find((x) => x.id === r.msg)!;
      m.approvalState = r.decision === 'allow' ? 'allowed' : r.decision === 'allow_session' ? 'allowed_session' : 'denied';
      panes[slot].meta.waitingFor = null;
      panes[slot].msgs.push({ id: 6, role: 'assistant', text: 'Done: the test passes now. ✅' });
      panes[slot].meta.busy = false;
      await reply();
      await sendApp(c, pane(slot));
      return sendApp(c, app());
    }
    case 'answer': {
      const m = panes[slot].msgs.find((x) => x.id === r.msg)!;
      m.questionState = 'answered';
      m.answers = r.answers;
      panes[slot].meta.waitingFor = null;
      await reply();
      return sendApp(c, pane(slot));
    }
    case 'send': {
      const p = panes[slot];
      const id = Math.max(...p.msgs.map((m) => m.id)) + 1;
      p.msgs.push({ id, role: 'user', text: r.text as string });
      p.msgs.push({ id: id + 1, role: 'assistant', text: `You said: *${r.text}*` });
      await reply();
      return sendApp(c, pane(slot));
    }
    case 'models':
      return reply([
        { id: 'opus', label: 'Opus 5.5', efforts: ['low', 'medium', 'high', 'max'], default_effort: 'high', description: 'Best for complex tasks', resolved: 'claude-opus-5-5', is_default: true },
        { id: 'sonnet', label: 'Sonnet 5.5', efforts: ['low', 'medium', 'high'], default_effort: 'medium', description: 'Fast', resolved: 'claude-sonnet-5-5', is_default: false },
      ]);
    case 'new_task': {
      const cfg = r.config as Record<string, string>;
      panes[2] = {
        meta: { ...panes[1].meta, slot: 2, pos: 2, provider: cfg.provider, accountId: cfg.accountId, accountName: 'Work', permission: cfg.permission, cwd: cfg.cwd, title: r.text, convId: 'c9', busy: true, startedAt: Date.now(), waitingFor: null, modelLabel: cfg.model || 'Opus 5.5' },
        msgs: [{ id: 1, role: 'user', text: r.text as string }, { id: 2, role: 'tool', text: 'Grep', detail: 'TODO' }],
      };
      await sendApp(c, app());
      return reply({ slot: 2 });
    }
    default:
      return reply();
  }
}

ws.onopen = () => ws.send(JSON.stringify({ t: 'auth', room, key }));
ws.onmessage = async (ev) => {
  const f = JSON.parse(String(ev.data));
  if (f.t === 'ok') {
    console.log(`PAIR ${relay}/#p=${room}.${b64(hostPub)}.${b64(secret)}`);
    return;
  }
  if (f.t === 'close') return conns.delete(f.c);
  if (f.t !== 'msg') return;
  const m = JSON.parse(f.d);
  if (m.t === 'hello') {
    if (m.mac) {
      const ok = b64(await pairProof(secret, unb64(m.pub), unb64(m.n))) === m.mac;
      console.log(`HELLO pair from "${m.name}" proof ${ok ? 'ok' : 'BAD'}`);
      if (!ok) return raw(f.c, { t: 'deny', why: 'expired' });
      raw(f.c, { t: 'wait' });
      await new Promise((r) => setTimeout(r, 1200));
    }
    const nh = random(16);
    const phone = await crypto.subtle.importKey('raw', unb64(m.pub), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
    const shared = await crypto.subtle.deriveBits({ name: 'ECDH', public: phone }, host.privateKey, 256);
    const ikm = await crypto.subtle.importKey('raw', shared, 'HKDF', false, ['deriveBits']);
    const salt = new Uint8Array([...unb64(m.n), ...nh]);
    const okm = new Uint8Array(await crypto.subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt, info: enc.encode('wren-remote-v1') }, ikm, 512));
    const aes = (b: Uint8Array<ArrayBuffer>) => crypto.subtle.importKey('raw', b, 'AES-GCM', false, ['encrypt', 'decrypt']);
    conns.set(f.c, { send: new Box(await aes(okm.slice(0, 32))), recv: new Box(await aes(okm.slice(32))), q: Promise.resolve() });
    raw(f.c, { t: 'welcome', n: b64(nh) });
    await sendApp(f.c, { t: 'ready', host: "Vakhim's MacBook Air", version: '0.1.11', vapid: b64(hostPub), device: { id: m.dev, name: m.name ?? '' }, push_kinds: [] });
    await sendApp(f.c, app());
    return;
  }
  if (m.t === 'e') {
    const s = conns.get(f.c);
    if (!s) return;
    const text = await s.recv.open(m.d);
    await onReq(f.c, JSON.parse(text));
  }
};
ws.onclose = (e) => console.log('CLOSED', e.code, e.reason);
