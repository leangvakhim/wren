/**
 * The phone's side of Wren's end-to-end encryption (WebCrypto only).
 * Must match src-tauri/src/remote/crypto.rs byte for byte:
 *   - P-256 ECDH; Wren's public key comes from the QR code
 *   - pairing proof = HMAC-SHA256(secret, "wren-pair-v1" ‖ phone key ‖ phone nonce)
 *   - HKDF-SHA256(salt = phone nonce ‖ Wren nonce, info "wren-remote-v1") → 64 bytes:
 *     [0,32) Wren→phone, [32,64) phone→Wren
 *   - AES-256-GCM, IV = 4 zero bytes ‖ 64-bit counter (never sent; both sides count)
 * Also used by the relay's end-to-end test under Node.
 */

const enc = new TextEncoder();
const dec = new TextDecoder();

export function b64(bytes: Uint8Array): string {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

export function unb64(s: string): Uint8Array<ArrayBuffer> {
  const bin = atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4));
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export const random = (n: number) => crypto.getRandomValues(new Uint8Array(n));

const concat = (...parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
};

/** What the QR code carries (in the URL fragment, never sent to the relay). */
export interface PairInfo {
  room: string;
  hostKey: string;
  secret: string;
}

export function parsePairFragment(hash: string): PairInfo | null {
  const m = /^#?p=([A-Za-z0-9_-]{22})\.([A-Za-z0-9_-]{87})\.([A-Za-z0-9_-]{43})$/.exec(hash);
  return m ? { room: m[1], hostKey: m[2], secret: m[3] } : null;
}

/** The phone's own key pair (the private half can't be exported). */
export async function newDeviceKey(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits']) as Promise<CryptoKeyPair>;
}

export async function publicRaw(key: CryptoKeyPair): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.exportKey('raw', key.publicKey));
}

export async function pairProof(secret: Uint8Array<ArrayBuffer>, devicePub: Uint8Array, nonce: Uint8Array): Promise<Uint8Array> {
  const k = await crypto.subtle.importKey('raw', secret, { name: 'HMAC', hash: 'SHA-256' }, false, ['sign']);
  return new Uint8Array(await crypto.subtle.sign('HMAC', k, concat(enc.encode('wren-pair-v1'), devicePub, nonce)));
}

/** The first message on every connection (`pair` only right after scanning the QR code). */
export async function hello(dev: string, key: CryptoKeyPair, nonce: Uint8Array, pair?: { secret: string; name: string }) {
  const pub = await publicRaw(key);
  const m: Record<string, unknown> = { t: 'hello', v: 1, dev, pub: b64(pub), n: b64(nonce) };
  if (pair) {
    m.name = pair.name;
    m.mac = b64(await pairProof(unb64(pair.secret), pub, nonce));
  }
  return JSON.stringify(m);
}

class Box {
  private counter = 0n;
  private key: CryptoKey;
  constructor(key: CryptoKey) {
    this.key = key;
  }

  private iv() {
    const iv = new Uint8Array(12);
    new DataView(iv.buffer).setBigUint64(4, this.counter);
    return iv;
  }

  async seal(text: string): Promise<string> {
    const iv = this.iv();
    this.counter++;
    return b64(new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, this.key, enc.encode(text))));
  }

  async open(sealed: string): Promise<string> {
    const iv = this.iv();
    const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, this.key, unb64(sealed));
    this.counter++;
    return dec.decode(plain);
  }
}

/** Both directions of one connection (phone side). */
export class Session {
  private queue: Promise<unknown> = Promise.resolve();
  private send: Box;
  private recv: Box;
  constructor(send: Box, recv: Box) {
    this.send = send;
    this.recv = recv;
  }

  /** Messages are sealed / opened strictly in order (the counters must match Wren's). */
  seal(text: string): Promise<string> {
    const p = this.queue.then(() => this.send.seal(text));
    this.queue = p.catch(() => {});
    return p;
  }

  open(sealed: string): Promise<string> {
    const p = this.queue.then(() => this.recv.open(sealed));
    this.queue = p.catch(() => {});
    return p;
  }
}

export async function session(key: CryptoKeyPair, hostKey: string, phoneNonce: Uint8Array, hostNonce: Uint8Array): Promise<Session> {
  const host = await crypto.subtle.importKey('raw', unb64(hostKey), { name: 'ECDH', namedCurve: 'P-256' }, false, []);
  const shared = await crypto.subtle.deriveBits({ name: 'ECDH', public: host }, key.privateKey, 256);
  const ikm = await crypto.subtle.importKey('raw', shared, 'HKDF', false, ['deriveBits']);
  const okm = new Uint8Array(
    await crypto.subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt: concat(phoneNonce, hostNonce), info: enc.encode('wren-remote-v1') }, ikm, 512),
  );
  const aes = (b: Uint8Array<ArrayBuffer>) => crypto.subtle.importKey('raw', b, 'AES-GCM', false, ['encrypt', 'decrypt']);
  const h2p = await aes(okm.slice(0, 32));
  const p2h = await aes(okm.slice(32));
  return new Session(new Box(p2h), new Box(h2p));
}

// ---------------- one connection to Wren through the relay ----------------

export type LinkState = 'connecting' | 'offline' | 'waiting' | 'ready' | 'denied';

export interface LinkOptions {
  /** wss://relay/phone?room=… */
  url: string;
  hostKey: string;
  dev: string;
  key: CryptoKeyPair;
  /** Only for the first connection after scanning the QR code. */
  pair?: { secret: string; name: string };
  onstate: (s: LinkState, why?: string) => void;
  onmessage: (m: Record<string, unknown>) => void;
  /** WebSocket constructor (the browser's, or one from Node in tests). */
  WS?: typeof WebSocket;
}

/** Connects once; resolves when the socket closes. */
export function connect(o: LinkOptions): { done: Promise<void>; send: (m: unknown) => Promise<void>; close: () => void } {
  const WS = o.WS ?? WebSocket;
  const ws = new WS(o.url);
  let sess: Session | null = null;
  /** Sealed frames can arrive while the keys are still being made: they wait for them, in order. */
  let pending: Promise<Session> | null = null;
  const nonce = random(16);
  let closed = false;
  o.onstate('connecting');
  const send = async (m: unknown) => {
    if (!sess || ws.readyState !== 1) throw new Error('Not connected to Wren.');
    const d = await sess.seal(JSON.stringify(m));
    ws.send(JSON.stringify({ t: 'msg', d: JSON.stringify({ t: 'e', d }) }));
  };
  const done = new Promise<void>((resolve) => {
    ws.onclose = (e: CloseEvent) => {
      closed = true;
      if (e.code !== 4003 || !sess) o.onstate('offline', e.reason || undefined);
      resolve();
    };
    ws.onmessage = async (ev: MessageEvent) => {
      let f: { t?: string; on?: boolean; d?: string };
      try {
        f = JSON.parse(String(ev.data));
      } catch {
        return;
      }
      if (f.t === 'host') {
        if (f.on && !pending) ws.send(JSON.stringify({ t: 'msg', d: await hello(o.dev, o.key, nonce, o.pair) }));
        if (!f.on) o.onstate('offline', 'Wren is offline');
        return;
      }
      if (f.t !== 'msg' || typeof f.d !== 'string') return;
      let m: { t?: string; n?: string; d?: string; why?: string };
      try {
        m = JSON.parse(f.d);
      } catch {
        return;
      }
      if (m.t === 'wait') return o.onstate('waiting');
      if (m.t === 'deny') return o.onstate('denied', m.why);
      if (m.t === 'welcome' && m.n && !pending) {
        pending = session(o.key, o.hostKey, nonce, unb64(m.n));
        sess = await pending;
        return;
      }
      if (m.t === 'e' && m.d && pending) {
        try {
          const s = await pending;
          const msg = JSON.parse(await s.open(m.d));
          // Wren proved it holds the key from the QR code by sealing this.
          if (msg.t === 'ready') o.onstate('ready');
          o.onmessage(msg);
        } catch {
          // Not from the real Wren (or out of order): stop.
          if (!closed) ws.close();
        }
      }
    };
  });
  return { done, send, close: () => ws.close() };
}
