/**
 * Usage-limit helpers (task 3). Pure functions, no app state.
 *
 * Wren never rotates between accounts of the same AI to get past its limits.
 * When an account runs out, it either continues in ANOTHER AI, or waits for
 * the reset time and then continues by itself.
 */

/** Error text that means "this account is out of usage for now". */
export const LIMIT_RE =
  /usage limit|limit reached|hit your (usage )?limit|rate.?limit|quota|resource_exhausted|\b429\b|out of (credits|usage)|credit balance|5-hour limit|weekly limit/i;

export const isLimitMessage = (text: string | null | undefined) => !!text && LIMIT_RE.test(text);

const UNIT_MS: Record<string, number> = {
  d: 86_400_000,
  day: 86_400_000,
  days: 86_400_000,
  h: 3_600_000,
  hr: 3_600_000,
  hrs: 3_600_000,
  hour: 3_600_000,
  hours: 3_600_000,
  m: 60_000,
  min: 60_000,
  mins: 60_000,
  minute: 60_000,
  minutes: 60_000,
  s: 1000,
  sec: 1000,
  secs: 1000,
  second: 1000,
  seconds: 1000,
};

/**
 * Find when the limit resets, from messages like:
 *  "Claude AI usage limit reached|1764000000"
 *  "5-hour limit reached · resets 3pm (Asia/Phnom_Penh)"
 *  "You've hit your usage limit ... try again in 2 hours 13 minutes"
 *  "Quota exceeded, resets in 4h 30m" / "try again at 3:45 PM" / "resets 15:00"
 *  "... resets at 2026-11-24T08:00:00Z"
 * Returns null when no time can be found.
 */
export function parseResetTime(text: string, now: Date = new Date()): Date | null {
  // 1. epoch seconds/ms after a pipe
  const epoch = text.match(/\|(\d{10}|\d{13})\b/);
  if (epoch) {
    const n = Number(epoch[1]);
    return new Date(epoch[1].length === 10 ? n * 1000 : n);
  }

  // 2. ISO timestamp
  const iso = text.match(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2})?(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?/);
  if (iso) {
    const t = Date.parse(iso[0]);
    if (!Number.isNaN(t)) return new Date(t);
  }

  // 3. relative: "in 2 hours 13 minutes", "in 4h 30m", "after 45 min"
  const rel = text.match(/\b(?:in|after)\s+((?:\d+(?:\.\d+)?\s*(?:days?|d|hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b[\s,]*(?:and\s+)?)+)/i);
  if (rel) {
    let ms = 0;
    for (const part of rel[1].matchAll(/(\d+(?:\.\d+)?)\s*([a-z]+)/gi)) {
      ms += Number(part[1]) * (UNIT_MS[part[2].toLowerCase()] ?? 0);
    }
    if (ms > 0) return new Date(now.getTime() + ms);
  }

  // 4. clock time: "resets 3pm", "resets at 15:00", "try again at 3:45 PM"
  const clock =
    text.match(/\b(?:resets?|reset at|resets at|try again at|available (?:again )?at)\s*(?:at\s+)?(\d{1,2})(?::(\d{2}))?\s*(am|pm)?/i) ??
    text.match(/\bat\s+(\d{1,2}):(\d{2})\s*(am|pm)?/i) ??
    text.match(/\bat\s+(\d{1,2})()\s*(am|pm)\b/i);
  if (clock) {
    let h = Number(clock[1]);
    const min = clock[2] ? Number(clock[2]) : 0;
    const ap = clock[3]?.toLowerCase();
    if (ap === 'pm' && h < 12) h += 12;
    if (ap === 'am' && h === 12) h = 0;
    if (h > 23 || min > 59) return null;
    const t = new Date(now);
    t.setHours(h, min, 0, 0);
    if (t.getTime() <= now.getTime()) t.setDate(t.getDate() + 1);
    return t;
  }
  return null;
}

/** Bubbles that show a recap Wren sent (not something the user wrote). */
export const isRecapBubble = (text: string) => text.startsWith('(Recap');

/**
 * A short recap of a conversation, to carry over to another AI.
 * `why`: 'limit' = the old AI ran out of usage, continue `lastRequest` now;
 * 'switch' = the user moved the conversation, the new AI just reads it.
 */
export function buildRecap(
  messages: { role: string; text: string }[],
  lastRequest: string,
  fromLabel: string,
  maxChars = 8000,
  why: 'limit' | 'switch' = 'limit',
): string {
  const lines: string[] = [];
  let used = 0;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m.role !== 'user' && m.role !== 'assistant') continue;
    if (m.role === 'user' && isRecapBubble(m.text)) continue;
    const line = `${m.role === 'user' ? 'User' : 'Assistant'}: ${m.text.trim()}`;
    if (used + line.length > maxChars) {
      lines.unshift('(earlier messages left out)');
      break;
    }
    lines.unshift(line);
    used += line.length;
  }
  const intro =
    why === 'limit'
      ? `I was working on this with ${fromLabel}, which has run out of usage. Here is the conversation so far:`
      : `I was working on this with ${fromLabel} and have moved the conversation to you. Here is the conversation so far:`;
  const outro =
    why === 'limit'
      ? ['Please continue from here. My latest request was:', lastRequest]
      : lastRequest
        ? ['Please continue from here. My new message is:', lastRequest]
        : ["Please read it and reply in one or two sentences that you're ready to continue. Don't start any work until I ask."];
  return [intro, '', '---', ...lines, '---', '', ...outro].join('\n');
}

export function formatWait(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60_000));
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${m % 60} min`;
}
