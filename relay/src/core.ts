/**
 * Wren's phone relay: passes messages between one Wren ("host") and the
 * phones paired with it. Everything the relay forwards is end-to-end
 * encrypted by Wren and the phone (see phone/src/protocol.ts and
 * src-tauri/src/remote/crypto.rs); the relay can't read it and stores nothing.
 *
 * Wire format (JSON text frames):
 *   host  → relay  {t:"auth", room, key}        first frame; key = access key from Wren's Settings
 *   relay → host   {t:"ok"} | {t:"open", c} | {t:"close", c} | {t:"msg", c, d}
 *   host  → relay  {t:"msg", c, d} | {t:"kick", c}
 *   relay → phone  {t:"host", on} | {t:"msg", d}
 *   phone → relay  {t:"msg", d}
 * `c` = the relay's number for a phone connection, `d` = an opaque string.
 *
 * Platform-neutral: the Node server (node.ts) and a future Cloudflare Worker
 * only adapt their WebSockets to `Sock`.
 */

export interface Sock {
  send(data: string): void;
  close(code: number, reason: string): void;
}

export interface Limits {
  /** Largest frame (characters) in either direction. */
  maxFrame: number;
  /** Phones connected to one Wren at the same time. */
  maxPhonesPerRoom: number;
  /** Wren copies connected at the same time. */
  maxRooms: number;
}

export const DEFAULT_LIMITS: Limits = { maxFrame: 1_048_576, maxPhonesPerRoom: 8, maxRooms: 50 };

/** Room ids are 16 random bytes, base64url (made by Wren). */
export const ROOM_RE = /^[A-Za-z0-9_-]{22}$/;

/** Close codes (4000-4999 are free for applications). */
export const CLOSE = {
  replaced: 4000,
  badKey: 4001,
  badFrame: 4002,
  kicked: 4003,
  noHost: 4004,
  full: 4005,
  tooBig: 4009,
} as const;

interface Room {
  host: Sock | null;
  phones: Map<number, Sock>;
}

export async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

function parse(data: string): Record<string, unknown> | null {
  try {
    const v = JSON.parse(data);
    return v && typeof v === 'object' && !Array.isArray(v) ? v : null;
  } catch {
    return null;
  }
}

/** Same-length string compare that doesn't stop at the first difference. */
function sameText(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  return diff === 0;
}

export class Relay {
  private rooms = new Map<string, Room>();
  private nextPhone = 1;

  private keyHashes: string[];
  private limits: Limits;

  /** `keyHashes` = SHA-256 (hex) of the access keys allowed to connect a Wren. */
  constructor(keyHashes: string[], limits: Limits = DEFAULT_LIMITS) {
    this.keyHashes = keyHashes;
    this.limits = limits;
  }

  stats() {
    let phones = 0;
    for (const r of this.rooms.values()) phones += r.phones.size;
    return { rooms: this.rooms.size, phones };
  }

  private async keyOk(key: unknown): Promise<boolean> {
    if (typeof key !== 'string' || key.length < 16 || key.length > 200) return false;
    const h = await sha256Hex(key);
    let ok = false;
    for (const k of this.keyHashes) ok = sameText(k, h) || ok;
    return ok;
  }

  // ---------------- Wren (host) ----------------

  /** A Wren connected. Returns handlers for its frames and its close. */
  host(sock: Sock) {
    let room: string | null = null;
    let authing = false;
    let closed = false;
    const onMessage = async (data: string) => {
      if (data.length > this.limits.maxFrame) return sock.close(CLOSE.tooBig, 'frame too big');
      const m = parse(data);
      if (!m) return sock.close(CLOSE.badFrame, 'bad frame');
      if (room === null) {
        if (m.t !== 'auth' || authing) return sock.close(CLOSE.badFrame, 'auth first');
        authing = true;
        if (typeof m.room !== 'string' || !ROOM_RE.test(m.room)) return sock.close(CLOSE.badFrame, 'bad room');
        if (!(await this.keyOk(m.key))) return sock.close(CLOSE.badKey, 'access key not accepted');
        if (closed) return;
        let r = this.rooms.get(m.room);
        if (!r) {
          if (this.rooms.size >= this.limits.maxRooms) return sock.close(CLOSE.full, 'relay full');
          r = { host: null, phones: new Map() };
          this.rooms.set(m.room, r);
        }
        // The same Wren reconnecting (e.g. after a network change) replaces its old connection.
        if (r.host) r.host.close(CLOSE.replaced, 'replaced by a newer connection');
        r.host = sock;
        room = m.room;
        sock.send(JSON.stringify({ t: 'ok' }));
        for (const [c, p] of r.phones) {
          sock.send(JSON.stringify({ t: 'open', c }));
          p.send(JSON.stringify({ t: 'host', on: true }));
        }
        return;
      }
      const r = this.rooms.get(room);
      if (!r || r.host !== sock) return;
      if (m.t === 'msg' && typeof m.c === 'number' && typeof m.d === 'string') {
        r.phones.get(m.c)?.send(JSON.stringify({ t: 'msg', d: m.d }));
      } else if (m.t === 'kick' && typeof m.c === 'number') {
        const p = r.phones.get(m.c);
        r.phones.delete(m.c);
        p?.close(CLOSE.kicked, 'closed by Wren');
      }
    };
    const onClose = () => {
      closed = true;
      if (room === null) return;
      const r = this.rooms.get(room);
      if (!r || r.host !== sock) return;
      r.host = null;
      // Phones wait for Wren on their own (they reconnect); the room goes away.
      for (const p of r.phones.values()) p.close(CLOSE.noHost, 'Wren went offline');
      this.rooms.delete(room);
    };
    return { onMessage, onClose };
  }

  // ---------------- phones ----------------

  /** A phone connected to `room`. Returns null (and closes it) if that Wren isn't online. */
  phone(sock: Sock, room: string) {
    const r = ROOM_RE.test(room) ? this.rooms.get(room) : undefined;
    if (!r?.host) {
      sock.send(JSON.stringify({ t: 'host', on: false }));
      sock.close(CLOSE.noHost, 'Wren is offline');
      return null;
    }
    if (r.phones.size >= this.limits.maxPhonesPerRoom) {
      sock.close(CLOSE.full, 'too many phones');
      return null;
    }
    const c = this.nextPhone++;
    r.phones.set(c, sock);
    sock.send(JSON.stringify({ t: 'host', on: true }));
    r.host.send(JSON.stringify({ t: 'open', c }));
    const onMessage = (data: string) => {
      if (data.length > this.limits.maxFrame) return sock.close(CLOSE.tooBig, 'frame too big');
      const m = parse(data);
      if (!m || m.t !== 'msg' || typeof m.d !== 'string') return sock.close(CLOSE.badFrame, 'bad frame');
      if (r.phones.get(c) !== sock) return;
      r.host?.send(JSON.stringify({ t: 'msg', c, d: m.d }));
    };
    const onClose = () => {
      if (r.phones.get(c) !== sock) return;
      r.phones.delete(c);
      r.host?.send(JSON.stringify({ t: 'close', c }));
    };
    return { onMessage, onClose };
  }
}
