/** The phone's pairing with one Wren, kept in this browser's IndexedDB (the private key can't be read out). */
export interface Pairing {
  room: string;
  hostKey: string;
  dev: string;
  key: CryptoKeyPair;
  /** The computer's name, from Wren. */
  host: string;
  name: string;
}

function db(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open('wren-phone', 1);
    req.onupgradeneeded = () => req.result.createObjectStore('kv');
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest): Promise<T> {
  const d = await db();
  return new Promise((resolve, reject) => {
    const r = run(d.transaction('kv', mode).objectStore('kv'));
    r.onsuccess = () => resolve(r.result as T);
    r.onerror = () => reject(r.error);
  });
}

export const loadPairing = () => tx<Pairing | undefined>('readonly', (s) => s.get('pairing')).catch(() => undefined);
export const savePairing = (p: Pairing) => tx<void>('readwrite', (s) => s.put(p, 'pairing'));
export const forgetPairing = () => tx<void>('readwrite', (s) => s.delete('pairing'));
