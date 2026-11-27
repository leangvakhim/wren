/**
 * Wren relay on Node (local testing, or a Docker server). Serves the phone
 * page from ./public and the two WebSocket endpoints:
 *   /host   one Wren (needs an access key)
 *   /phone  phones (?room=…); everything they send is end-to-end encrypted
 *
 * Settings (environment):
 *   WREN_RELAY_KEYS  comma-separated SHA-256 hashes of the allowed access keys
 *                    (make one with `npm run key`)
 *   PORT             default 8787
 *   HOST             default 127.0.0.1 (use 0.0.0.0 in Docker)
 *   PUBLIC_DIR       default ../public (the built phone page)
 *
 * Put it behind HTTPS (e.g. Caddy): phones need HTTPS for encryption,
 * Add to Home Screen and push notifications.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { WebSocketServer, type WebSocket } from 'ws';
import { DEFAULT_LIMITS, Relay, type Sock } from './core.ts';

const here = fileURLToPath(new URL('.', import.meta.url));
const PUBLIC = resolve(process.env.PUBLIC_DIR ?? join(here, '..', 'public'));
const PORT = Number(process.env.PORT ?? 8787);
const HOST = process.env.HOST ?? '127.0.0.1';
const KEYS = (process.env.WREN_RELAY_KEYS ?? '')
  .split(',')
  .map((k) => k.trim().toLowerCase())
  .filter((k) => /^[0-9a-f]{64}$/.test(k));

if (!KEYS.length) {
  console.error('No access keys set. Run `npm run key` and start with WREN_RELAY_KEYS=<hash>.');
  process.exit(1);
}

const relay = new Relay(KEYS);

const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.webmanifest': 'application/manifest+json',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
};

const SECURITY_HEADERS = {
  'Content-Security-Policy':
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; manifest-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'; object-src 'none'",
  'X-Content-Type-Options': 'nosniff',
  'Referrer-Policy': 'no-referrer',
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
};

async function serveFile(req: IncomingMessage, res: ServerResponse) {
  const url = new URL(req.url ?? '/', 'http://x');
  if (url.pathname === '/healthz') {
    res.writeHead(200, { 'Content-Type': 'application/json' });
    return res.end(JSON.stringify(relay.stats()));
  }
  let path = decodeURIComponent(url.pathname);
  if (path.endsWith('/')) path += 'index.html';
  const file = normalize(join(PUBLIC, path));
  if (!file.startsWith(PUBLIC)) {
    res.writeHead(404);
    return res.end();
  }
  try {
    const s = await stat(file);
    if (!s.isFile()) throw new Error('not a file');
    const body = await readFile(file);
    const ext = extname(file);
    res.writeHead(200, {
      ...SECURITY_HEADERS,
      'Content-Type': TYPES[ext] ?? 'application/octet-stream',
      // The page and service worker always check for updates; hashed assets are kept.
      'Cache-Control': /\/assets\//.test(path) ? 'public, max-age=31536000, immutable' : 'no-cache',
    });
    res.end(body);
  } catch {
    res.writeHead(404, SECURITY_HEADERS);
    res.end('Not found');
  }
}

const server = createServer((req, res) => {
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405);
    return res.end();
  }
  serveFile(req, res).catch(() => {
    res.writeHead(500);
    res.end();
  });
});

const wss = new WebSocketServer({ noServer: true, maxPayload: DEFAULT_LIMITS.maxFrame + 1024 });

function adapt(ws: WebSocket): Sock {
  return {
    send: (d) => ws.readyState === ws.OPEN && ws.send(d),
    close: (code, reason) => ws.close(code, reason),
  };
}

const alive = new WeakMap<WebSocket, boolean>();

server.on('upgrade', (req, socket, head) => {
  const url = new URL(req.url ?? '/', 'http://x');
  const origin = req.headers.origin;
  // Phones come from this relay's own page; Wren sends no Origin.
  let sameSite = true;
  try {
    sameSite = !origin || new URL(origin).host === req.headers.host;
  } catch {
    sameSite = false;
  }
  if (!sameSite) return socket.destroy();
  if (url.pathname !== '/host' && url.pathname !== '/phone') return socket.destroy();
  wss.handleUpgrade(req, socket, head, (ws) => {
    alive.set(ws, true);
    ws.on('pong', () => alive.set(ws, true));
    const sock = adapt(ws);
    const h = url.pathname === '/host' ? relay.host(sock) : relay.phone(sock, url.searchParams.get('room') ?? '');
    if (!h) return;
    ws.on('message', (data, isBinary) => {
      if (isBinary) return ws.close(4002, 'text only');
      void h.onMessage(data.toString());
    });
    ws.on('close', () => h.onClose());
    ws.on('error', () => ws.terminate());
  });
});

// Drop connections that stopped answering (e.g. a phone that lost signal).
setInterval(() => {
  for (const ws of wss.clients) {
    if (!alive.get(ws)) {
      ws.terminate();
      continue;
    }
    alive.set(ws, false);
    ws.ping();
  }
}, 30_000).unref();

server.listen(PORT, HOST, () => console.log(`Wren relay on http://${HOST}:${PORT} (phone page from ${PUBLIC})`));
