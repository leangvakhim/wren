/**
 * Makes a new access key for a Wren copy. The key goes into Wren
 * (Settings → Phone); only its hash goes into the relay's WREN_RELAY_KEYS,
 * so the relay's settings can't be used to connect.
 */
import { sha256Hex } from './core.ts';

const bytes = crypto.getRandomValues(new Uint8Array(32));
const key = 'wren_' + Buffer.from(bytes).toString('base64url');
console.log(`Access key (paste into Wren → Settings → Phone):\n  ${key}\n`);
console.log(`Hash (add to the relay's WREN_RELAY_KEYS, comma-separated):\n  ${await sha256Hex(key)}`);
