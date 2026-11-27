import type { PhonePaneMeta } from '../../src/lib/remoteTypes';

export const folderOf = (cwd: string) => cwd.split(/[\\/]/).filter(Boolean).pop() ?? '';

export function ago(ms: number): string {
  const s = Math.round((Date.now() - ms) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  const d = new Date(ms);
  if (d.toDateString() === new Date().toDateString()) return d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' });
}

/** One-word state of a pane for the home list. */
export function paneState(p: PhonePaneMeta): { text: string; tone: 'need' | 'busy' | 'idle' | 'bad' } {
  if (p.waitingFor === 'approval') return { text: 'Needs your OK', tone: 'need' };
  if (p.waitingFor === 'question') return { text: 'Asks you', tone: 'need' };
  if (p.limitHit) return { text: 'Out of usage', tone: 'bad' };
  if (p.waitingLimit) return { text: 'Waiting for reset', tone: 'busy' };
  if (p.busy) return { text: 'Working…', tone: 'busy' };
  if (p.problem) return { text: 'Needs setup', tone: 'bad' };
  return { text: 'Idle', tone: 'idle' };
}

export const fmtSize = (n: number) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${Math.ceil(n / 1024)} KB` : `${n} B`);
