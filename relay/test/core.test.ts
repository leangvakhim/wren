import { test } from 'node:test';
import assert from 'node:assert/strict';
import { CLOSE, Relay, sha256Hex, type Sock } from '../src/core.ts';

class FakeSock implements Sock {
  sent: Record<string, unknown>[] = [];
  closed: [number, string] | null = null;
  send(d: string) {
    this.sent.push(JSON.parse(d));
  }
  close(code: number, reason: string) {
    this.closed = [code, reason];
  }
}

const ROOM = 'AAAAAAAAAAAAAAAAAAAAAA';
const KEY = 'wren_test_key_0123456789';

async function online() {
  const relay = new Relay([await sha256Hex(KEY)], { maxFrame: 1000, maxPhonesPerRoom: 2, maxRooms: 5 });
  const host = new FakeSock();
  const h = relay.host(host);
  await h.onMessage(JSON.stringify({ t: 'auth', room: ROOM, key: KEY }));
  return { relay, host, h };
}

test('a wrong access key is refused', async () => {
  const relay = new Relay([await sha256Hex(KEY)]);
  const host = new FakeSock();
  await relay.host(host).onMessage(JSON.stringify({ t: 'auth', room: ROOM, key: 'wren_wrong_key_000000' }));
  assert.equal(host.closed?.[0], CLOSE.badKey);
  assert.deepEqual(relay.stats(), { rooms: 0, phones: 0 });
});

test('phones only reach a Wren that is online', async () => {
  const relay = new Relay([await sha256Hex(KEY)]);
  const phone = new FakeSock();
  assert.equal(relay.phone(phone, ROOM), null);
  assert.equal(phone.closed?.[0], CLOSE.noHost);
  assert.equal(relay.phone(new FakeSock(), '../etc'), null);
});

test('messages pass both ways, only to the right phone', async () => {
  const { relay, host, h } = await online();
  assert.deepEqual(host.sent[0], { t: 'ok' });
  const a = new FakeSock();
  const b = new FakeSock();
  const pa = relay.phone(a, ROOM)!;
  relay.phone(b, ROOM);
  const opens = host.sent.filter((m) => m.t === 'open').map((m) => m.c as number);
  assert.equal(opens.length, 2);
  pa.onMessage(JSON.stringify({ t: 'msg', d: 'sealed-1' }));
  assert.deepEqual(host.sent.at(-1), { t: 'msg', c: opens[0], d: 'sealed-1' });
  await h.onMessage(JSON.stringify({ t: 'msg', c: opens[1], d: 'for-b' }));
  assert.deepEqual(b.sent.at(-1), { t: 'msg', d: 'for-b' });
  assert.ok(!a.sent.some((m) => m.d === 'for-b'));
  // A third phone is over the limit.
  const c = new FakeSock();
  assert.equal(relay.phone(c, ROOM), null);
  assert.equal(c.closed?.[0], CLOSE.full);
  // Wren kicks a phone.
  await h.onMessage(JSON.stringify({ t: 'kick', c: opens[0] }));
  assert.equal(a.closed?.[0], CLOSE.kicked);
});

test('big or odd frames close the sender; Wren leaving closes its phones', async () => {
  const { relay, h } = await online();
  const p = new FakeSock();
  const ph = relay.phone(p, ROOM)!;
  ph.onMessage('x'.repeat(2000));
  assert.equal(p.closed?.[0], CLOSE.tooBig);
  ph.onClose(); // a real socket reports its close
  const q = new FakeSock();
  const qh = relay.phone(q, ROOM)!;
  qh.onMessage('{"t":"other"}');
  assert.equal(q.closed?.[0], CLOSE.badFrame);
  qh.onClose();
  const r = new FakeSock();
  relay.phone(r, ROOM);
  h.onClose();
  assert.equal(r.closed?.[0], CLOSE.noHost);
  assert.equal(relay.stats().rooms, 0);
});

test('a reconnecting Wren replaces its old connection', async () => {
  const { relay, host } = await online();
  const again = new FakeSock();
  await relay.host(again).onMessage(JSON.stringify({ t: 'auth', room: ROOM, key: KEY }));
  assert.equal(host.closed?.[0], CLOSE.replaced);
  assert.deepEqual(again.sent[0], { t: 'ok' });
});
