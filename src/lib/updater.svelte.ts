import { getVersion } from '@tauri-apps/api/app';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

/**
 * In-app updates. Wren asks the latest GitHub release (latest.json, made by the
 * release workflow) whether there is a newer version. Updates are signed with
 * the owner's key; the updater refuses anything that isn't.
 */
export const updater = $state<{
  current: string;
  /** Newer version found, waiting for the user's click. */
  available: Update | null;
  phase: 'idle' | 'checking' | 'downloading' | 'installing' | 'error' | 'latest';
  /** 0-100 while downloading, when the size is known. */
  percent: number | null;
  error: string | null;
}>({ current: '', available: null, phase: 'idle', percent: null, error: null });

const EVERY_MS = 6 * 60 * 60 * 1000;
let timer: ReturnType<typeof setInterval> | undefined;

/** `quiet`: background check, no error shown (e.g. offline). */
export async function checkForUpdate(quiet = true): Promise<void> {
  if (updater.phase === 'checking' || updater.phase === 'downloading' || updater.phase === 'installing') return;
  updater.phase = 'checking';
  updater.error = null;
  try {
    if (!updater.current) updater.current = await getVersion();
    const u = await check();
    updater.available = u;
    updater.phase = u ? 'idle' : 'latest';
  } catch (e) {
    updater.phase = quiet ? 'idle' : 'error';
    if (!quiet) updater.error = String(e);
  }
}

/** First check shortly after start, then every 6 hours while Wren is open. */
export function startUpdateChecks() {
  getVersion().then((v) => (updater.current = v)).catch(() => {});
  setTimeout(() => checkForUpdate(true), 8000);
  clearInterval(timer);
  timer = setInterval(() => checkForUpdate(true), EVERY_MS);
}

/** Download, install, then restart Wren. */
export async function installUpdate(): Promise<void> {
  const u = updater.available;
  if (!u) return;
  updater.phase = 'downloading';
  updater.percent = null;
  updater.error = null;
  let total = 0;
  let got = 0;
  try {
    await u.downloadAndInstall((ev) => {
      if (ev.event === 'Started') total = ev.data.contentLength ?? 0;
      else if (ev.event === 'Progress') {
        got += ev.data.chunkLength;
        updater.percent = total ? Math.min(100, Math.round((got / total) * 100)) : null;
      } else if (ev.event === 'Finished') updater.phase = 'installing';
    });
    await relaunch();
  } catch (e) {
    updater.phase = 'error';
    updater.error = String(e);
  }
}
