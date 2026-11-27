# Wren relay (phone link)

The relay lets your phone reach Wren on your computer from anywhere. It only
passes messages along: Wren and the phone encrypt everything end to end
(P-256 keys, AES-256-GCM), so the relay can't read your chats, approvals or
files, and it stores nothing. It also serves the phone page.

```
phone ──https/wss──▶ relay ◀──wss── Wren on your computer
          (sealed boxes only; the pairing secret never reaches the relay)
```

Only Wren copies with an **access key** you made can connect. Phones need
no key; they still can't do anything until you pair them in Wren and click
**Allow** on the computer.

## Run it

Needs Node 22.6 or newer.

```sh
cd relay
npm install
npm run key        # prints an access key + its hash
```

Start the relay with the hash (several hashes = several Wren copies,
comma-separated):

```sh
WREN_RELAY_KEYS=<hash> PORT=8787 HOST=0.0.0.0 npm start
```

Build the phone page into `relay/public` first (from the repository root):

```sh
npm run build:phone
```

Then in Wren: **Phone** (top bar) → relay address `https://<your relay>` and
the **access key** → **Turn on** → **Show pairing code**, and scan it with
the phone.

## HTTPS is required

Phones only allow the encryption, Add to Home Screen and notifications on
HTTPS pages. Put the relay behind HTTPS, for example with Caddy:

```
remote.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

For a quick test without a server, a tunnel also works (e.g.
`cloudflared tunnel --url http://127.0.0.1:8787` or `tailscale funnel 8787`);
use the https address it prints as the relay address in Wren.

(`http://localhost` is accepted by Wren only for testing on the same computer.)

## Settings

| Variable | Default | |
|---|---|---|
| `WREN_RELAY_KEYS` | (required) | SHA-256 hashes of the access keys (from `npm run key`) |
| `PORT` | `8787` | |
| `HOST` | `127.0.0.1` | use `0.0.0.0` in a container |
| `PUBLIC_DIR` | `./public` | the built phone page |

`GET /healthz` shows how many Wren copies and phones are connected.

Limits: 8 phones per Wren, 50 Wren copies, 1 MB per message (Wren sends
bigger files in pieces; phone uploads are up to 25 MB).

## Tests

```sh
npm test                                   # relay rules
cd ../src-tauri && WREN_RELAY_E2E=1 cargo test real_relay_round_trip -- --nocapture
                                           # Wren's side + this relay + the phone's crypto
```

The core (`src/core.ts`) doesn't depend on Node, so a Cloudflare Worker
version only needs a small adapter.
