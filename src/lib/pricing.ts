/**
 * API list prices (USD per 1M tokens) used to ESTIMATE cost for Codex and
 * Antigravity. Claude Code reports its own cost, so Claude panes don't use this.
 * On subscription plans you are not billed per token: these are estimates.
 *
 * Sources (checked 2026-11-23):
 * - OpenAI API pricing page (gpt-6.2-astra, gpt-6.2-sol, gpt-6-astra, gpt-6.1-sol, gpt-6.1-terra)
 * - OpenAI launch coverage (gpt-6-sol, gpt-6-luna, gpt-5.6 family, gpt-5.5)
 * - Google Gemini API / Vertex pricing (Gemini 4 Carbon/Argon/Hydrogen, Gemini 3.8 Flash)
 * - Anthropic pricing (Claude Opus/Sonnet/Haiku 5.5, used via Antigravity)
 */
export interface Rate {
  input: number;
  cached: number;
  output: number;
  /** Price applies until this date (exclusive), ISO yyyy-mm-dd. */
  until?: string;
}

/** Keys are model names with everything but letters/digits removed. Longest match wins. */
const PRICES: Record<string, Rate[]> = {
  // OpenAI (Codex)
  gpt62astra: [{ input: 8, cached: 0.4, output: 40 }],
  gpt62sol: [{ input: 2, cached: 0.1, output: 10 }],
  gpt6astra: [{ input: 10, cached: 1, output: 50 }],
  gpt61sol: [{ input: 2, cached: 0.1, output: 10 }],
  gpt61terra: [{ input: 0.5, cached: 0.025, output: 2.5 }],
  gpt6sol: [{ input: 2, cached: 0.2, output: 10 }],
  gpt6luna: [{ input: 0.1, cached: 0.01, output: 0.5 }],
  gpt56sol: [{ input: 5, cached: 0.5, output: 30 }],
  gpt56terra: [{ input: 2, cached: 0.2, output: 12 }],
  gpt56luna: [{ input: 0.2, cached: 0.02, output: 1.2 }],
  gpt55: [{ input: 5, cached: 0.5, output: 30 }],
  // Google (Antigravity)
  gemini4carbon: [
    { input: 2, cached: 0.1, output: 10, until: '2026-12-01' },
    { input: 4, cached: 0.2, output: 20 },
  ],
  gemini4argon: [
    { input: 2, cached: 0.1, output: 10, until: '2026-12-01' },
    { input: 4, cached: 0.2, output: 20 },
  ],
  gemini4hydrogen: [{ input: 1.5, cached: 0.075, output: 7.5 }],
  // cached rate not published here: charged at the input rate (over-estimates)
  gemini38flash: [
    { input: 0.75, cached: 0.75, output: 3.75, until: '2027-01-01' },
    { input: 1.5, cached: 1.5, output: 7.5 },
  ],
  // Anthropic models offered inside Antigravity
  claudeopus55: [{ input: 4, cached: 0.2, output: 20 }],
  claudesonnet55: [{ input: 2, cached: 0.1, output: 10 }],
  claudehaiku55: [{ input: 0.1, cached: 0.01, output: 0.5 }],
};

const KEYS = Object.keys(PRICES).sort((a, b) => b.length - a.length);
const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '');

export function rateFor(model: string | null | undefined, now = new Date()): Rate | null {
  if (!model) return null;
  const n = norm(model);
  const key = KEYS.find((k) => n.includes(k));
  if (!key) return null;
  const today = now.toISOString().slice(0, 10);
  return PRICES[key].find((r) => !r.until || today < r.until) ?? null;
}

/** USD estimate for one turn. `input` excludes cached tokens. */
export function estimateCost(
  model: string | null | undefined,
  usage: { input_tokens: number; output_tokens: number; cache_read_tokens: number; cache_write_tokens: number },
): number | null {
  const r = rateFor(model);
  if (!r) return null;
  return (
    ((usage.input_tokens + usage.cache_write_tokens) * r.input +
      usage.cache_read_tokens * r.cached +
      usage.output_tokens * r.output) /
    1_000_000
  );
}
