import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { settings } from './settings.svelte';

/** What a notification is about (each can be turned off in Settings). */
export type NotifyKind = 'approval' | 'question' | 'done' | 'problems';

export const notifyState = $state<{ blocked: boolean; error: string | null }>({ blocked: false, error: null });

/** Asked once per run; macOS / Windows remember the person's answer. */
let permission: Promise<boolean> | null = null;

function allowed(): Promise<boolean> {
  permission ??= (async () => {
    try {
      if (await isPermissionGranted()) return true;
      return (await requestPermission()) === 'granted';
    } catch {
      return false;
    }
  })().then((ok) => {
    notifyState.blocked = !ok;
    if (!ok) permission = null; // ask again next time (e.g. after allowing in System Settings)
    return ok;
  });
  return permission;
}

/** The pane that notified last becomes the active one when the person comes back to Wren. */
let comeBack: (() => void) | null = null;
let listening = false;

function listenForFocus() {
  if (listening) return;
  listening = true;
  getCurrentWindow()
    .onFocusChanged(({ payload: focused }) => {
      if (!focused || !comeBack) return;
      const go = comeBack;
      comeBack = null;
      go();
    })
    .catch(() => (listening = false));
}

/** Last notification per pane + kind, so a burst of cards gives one notification. */
const recent = new Map<string, number>();
const QUIET_MS = 15_000;

/** Each system's own notification sound (no sound name = silent). */
const SOUND = navigator.userAgent.includes('Mac')
  ? 'NSUserNotificationDefaultSoundName'
  : navigator.userAgent.includes('Windows')
    ? 'Default'
    : 'message-new-instant';

function send(title: string, body: string) {
  sendNotification({ title, body: body.slice(0, 180), ...(settings.notify.sound ? { sound: SOUND } : {}) });
}

/**
 * Tell the person something happened in a pane, if they allow this kind and
 * Wren isn't the window in front. `onreturn` runs when they come back to Wren.
 */
export async function notify(pane: string, kind: NotifyKind, title: string, body: string, onreturn?: () => void) {
  const n = settings.notify;
  if (!n.enabled || !n[kind]) return;
  try {
    if (await getCurrentWindow().isFocused()) return;
  } catch {
    return;
  }
  const key = `${pane}:${kind}`;
  const now = Date.now();
  if (now - (recent.get(key) ?? 0) < QUIET_MS) return;
  recent.set(key, now);
  if (!(await allowed())) return;
  try {
    send(title, body);
    notifyState.error = null;
    if (onreturn) {
      comeBack = onreturn;
      listenForFocus();
    }
  } catch (e) {
    notifyState.error = String(e);
  }
}

/** Settings → "Send a test notification" (also while Wren is in front). */
export async function testNotification(): Promise<boolean> {
  if (!(await allowed())) return false;
  try {
    send('Wren', 'Notifications are on. You\'ll see these when a pane needs you or finishes.');
    notifyState.error = null;
    return true;
  } catch (e) {
    notifyState.error = String(e);
    return false;
  }
}

/** First line of a markdown answer, as plain text for a notification. */
export function firstLine(md: string): string {
  const line =
    md
      .split('\n')
      .map((l) => l.trim())
      .find((l) => l && !l.startsWith('```')) ?? '';
  return line
    .replace(/^#+\s*|^[-*>]\s+|^\d+\.\s+/, '')
    .replace(/[*`~]|\[([^\]]*)\]\([^)]*\)/g, '$1')
    .trim();
}
