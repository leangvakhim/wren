// @ts-nocheck -- plain Node test, run with: node --experimental-strip-types src/lib/limits.test.ts
import assert from 'node:assert/strict';
import { buildRecap, isLimitMessage, parseResetTime } from './limits.ts';

const now = new Date(2026, 10, 23, 14, 0, 0); // 23 Nov 2026 14:00 local

// epoch after a pipe
assert.equal(parseResetTime('Claude AI usage limit reached|1764000000', now)?.getTime(), 1764000000 * 1000);
// relative
assert.equal(parseResetTime("You've hit your usage limit. Try again in 2 hours 13 minutes.", now)?.getTime(), now.getTime() + (2 * 60 + 13) * 60_000);
assert.equal(parseResetTime('Quota exceeded, resets in 4h 30m', now)?.getTime(), now.getTime() + 270 * 60_000);
// clock times (later today / tomorrow)
const at3pm = parseResetTime('5-hour limit reached · resets 3pm (Asia/Phnom_Penh)', now)!;
assert.equal(at3pm.getHours(), 15);
assert.equal(at3pm.getDate(), 23);
const at9am = parseResetTime('Limit reached · resets 9am', now)!;
assert.equal(at9am.getDate(), 24, '9am already passed today, so tomorrow');
assert.equal(parseResetTime('try again at 3:45 PM', now)?.getMinutes(), 45);
assert.equal(parseResetTime('resets 15:00', now)?.getHours(), 15);
// ISO
assert.equal(parseResetTime('resets at 2026-11-24T08:00:00Z', now)?.toISOString(), '2026-11-24T08:00:00.000Z');
// nothing to find
assert.equal(parseResetTime('Claude usage limit reached', now), null);
assert.equal(parseResetTime('error at line 12 in 3 files', now), null);

// detection
assert.ok(isLimitMessage('Claude AI usage limit reached'));
assert.ok(isLimitMessage('RESOURCE_EXHAUSTED: quota'));
assert.ok(!isLimitMessage('File not found'));

// recap keeps the newest messages and the last request
const recap = buildRecap(
  [
    { role: 'user', text: 'old '.repeat(3000) },
    { role: 'assistant', text: 'answer' },
    { role: 'tool', text: 'shell' },
    { role: 'user', text: 'fix the bug' },
  ],
  'fix the bug',
  'Claude Code',
  200,
);
assert.ok(recap.includes('Claude Code'));
assert.ok(recap.includes('(earlier messages left out)'));
assert.ok(recap.includes('Assistant: answer'));
assert.ok(!recap.includes('shell'));
assert.ok(recap.trim().endsWith('fix the bug'));
console.log('limits: all tests passed');
