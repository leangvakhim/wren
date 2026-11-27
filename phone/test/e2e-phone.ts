/**
 * A phone for the end-to-end test (src-tauri/src/remote/mod.rs,
 * `real_relay_round_trip`): pairs with the QR link, asks Wren something,
 * sends a file, then reconnects without the QR secret like a paired phone.
 * Usage: node --experimental-strip-types e2e-phone.ts <relay base url> <pair url>
 * Prints one line per step; the last line is "ALL OK" on success.
 */
import { b64, connect, newDeviceKey, parsePairFragment, random } from '../src/protocol.ts';

const [relay, pairUrl] = process.argv.slice(2);
const info = parsePairFragment(new URL(pairUrl).hash);
if (!info) throw new Error('bad pair link');
const wsUrl = `${relay.replace(/^http/, 'ws')}/phone?room=${info.room}`;
const key = await newDeviceKey();
const dev = 'test-' + b64(random(9));

function step(name: string, pair: boolean, run: (send: (m: unknown) => Promise<void>, next: () => Promise<Record<string, unknown>>) => Promise<void>) {
  return new Promise<void>((resolve, reject) => {
    const inbox: Record<string, unknown>[] = [];
    const waiters: ((m: Record<string, unknown>) => void)[] = [];
    const next = () =>
      new Promise<Record<string, unknown>>((res) => {
        const m = inbox.shift();
        if (m) res(m);
        else waiters.push(res);
      });
    const link = connect({
      url: wsUrl,
      hostKey: info.hostKey,
      dev,
      key,
      pair: pair ? { secret: info.secret, name: 'Node test phone' } : undefined,
      onstate: (s, why) => {
        console.log(`${name}: ${s}${why ? ` (${why})` : ''}`);
        if (s === 'denied') reject(new Error(`denied: ${why}`));
      },
      onmessage: (m) => {
        const w = waiters.shift();
        if (w) w(m);
        else inbox.push(m);
      },
    });
    const timer = setTimeout(() => reject(new Error(`${name}: timed out`)), 20_000);
    (async () => {
      const ready = await next();
      if (ready.t !== 'ready') throw new Error(`expected ready, got ${JSON.stringify(ready)}`);
      console.log(`${name}: Wren is "${ready.host}" ${ready.version}`);
      await run(link.send, next);
      link.close();
      await link.done;
      clearTimeout(timer);
      resolve();
    })().catch(reject);
  });
}

await step('pair', true, async (send, next) => {
  await send({ t: 'req', id: 1, op: 'ping', text: 'hello from the phone' });
  const res = await next();
  if (res.t !== 'res' || res.id !== 1 || (res.data as { echo?: string })?.echo !== 'hello from the phone') throw new Error(`bad answer ${JSON.stringify(res)}`);
  console.log('pair: Wren answered');
  const file = new TextEncoder().encode('name,score\nwren,10\n'.repeat(5000));
  const chunk = 32 * 1024;
  let last: Record<string, unknown> = {};
  for (let i = 0; i < file.length; i += chunk) {
    const end = Math.min(file.length, i + chunk);
    await send({ t: 'req', id: 100 + i, op: 'upload', u: 'up1', name: 'scores.csv', size: file.length, last: end === file.length, data: b64(file.subarray(i, end)) });
    last = await next();
    if (!last.ok) throw new Error(`upload failed ${JSON.stringify(last)}`);
  }
  const a = last.data as { path: string; size: number; name: string };
  if (a.size !== file.length || a.name !== 'scores.csv') throw new Error(`bad attachment ${JSON.stringify(a)}`);
  console.log(`pair: file saved at ${a.path}`);
});

// Same phone again: no QR secret needed.
await step('again', false, async (send, next) => {
  await send({ t: 'req', id: 2, op: 'ping', text: 'again' });
  const res = await next();
  if ((res.data as { echo?: string })?.echo !== 'again') throw new Error('bad second answer');
});

// Another phone with the old QR link: refused (one QR code = one phone).
await new Promise<void>((resolve, reject) => {
  newDeviceKey().then((k) => {
    connect({
      url: wsUrl,
      hostKey: info.hostKey,
      dev: 'test-' + b64(random(9)),
      key: k,
      pair: { secret: info.secret, name: 'Thief' },
      onstate: (s, why) => {
        if (s === 'denied') {
          console.log(`thief: denied (${why})`);
          resolve();
        }
        if (s === 'ready') reject(new Error('a second phone got in with the same QR code'));
      },
      onmessage: () => {},
    });
  });
  setTimeout(() => reject(new Error('thief: no answer')), 10_000);
});

console.log('ALL OK');
process.exit(0);
