import { getCurrentWindow } from '@tauri-apps/api/window';

export type Theme = 'light' | 'dark' | 'system';

const media = () => window.matchMedia('(prefers-color-scheme: dark)');
let unlisten: (() => void) | null = null;

/** Apply light, dark or follow-the-system, and keep following the system. */
export function applyTheme(theme: Theme) {
  const set = () => {
    const dark = theme === 'dark' || (theme === 'system' && media().matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  };
  set();
  unlisten?.();
  unlisten = null;
  if (theme === 'system') {
    const mq = media();
    mq.addEventListener('change', set);
    unlisten = () => mq.removeEventListener('change', set);
  }
  // Native title bar follows too (null = system).
  getCurrentWindow()
    .setTheme(theme === 'system' ? null : theme)
    .catch(() => {});
}
