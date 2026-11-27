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
  /** Show the update window (a newer version was found and you haven't said "Later" this session). */
  popup: boolean;
  phase: 'idle' | 'checking' | 'downloading' | 'installing' | 'error' | 'latest';
  /** 0-100 while downloading, when the size is known. */
  percent: number | null;
  error: string | null;
}>({ current: '', available: null, popup: false, phase: 'idle', percent: null, error: null });

const EVERY_MS = 6 * 60 * 60 * 1000;
let timer: ReturnType<typeof setInterval> | undefined;
/** "Later" was clicked: no more pop-ups until Wren opens again (the top-bar button stays). */
let snoozed = false;

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
    // Background checks pop up once per session; "Check for updates" always does.
    if (u && (!quiet || !snoozed)) updater.popup = true;
  } catch (e) {
    updater.phase = quiet ? 'idle' : 'error';
    if (!quiet) updater.error = String(e);
  }
}

/** "Later": close the window and don't pop up again until Wren is opened again. */
export function snoozeUpdate() {
  snoozed = true;
  updater.popup = false;
}

/**
 * The "What's new" part of the release notes (the release workflow puts the
 * version's CHANGELOG.md section first, above a `---` line). Older releases
 * only had download instructions: nothing to show then.
 */
export function whatsNew(body: string | undefined): string {
  const head = /^#+\s*What's new.*$/im.exec(body ?? '');
  if (!head || !body) return '';
  const rest = body.slice(head.index + head[0].length);
  const end = rest.search(/^-{3,}\s*$/m);
  return (end < 0 ? rest : rest.slice(0, end)).trim();
}

/** First check soon after Wren opens, then every 6 hours while it stays open. */
export function startUpdateChecks() {
  getVersion().then((v) => (updater.current = v)).catch(() => {});
  setTimeout(() => checkForUpdate(true), 3000);
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
