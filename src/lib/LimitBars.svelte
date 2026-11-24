<script lang="ts">
  import { levelOf, resetText, usedNow } from './planLimits.svelte';
  import type { LimitWindow, Provider } from './types';

  /**
   * One account's plan limits on ONE row, two columns:
   * "Current session" (the 5-hour window) | "This week".
   * Any other limits (e.g. "Weekly (Fable)", Codex "Spark") are listed in the
   * row's hover text, with a ⚠ when one of them is near its limit.
   * The fill is the AI's own color until 80% (warning) and 95% (critical),
   * and the state is also written out, never color alone.
   */
  let { windows, provider, now = Date.now() }: { windows: LimitWindow[]; provider: Provider; now?: number } = $props();

  const COLUMNS = [
    { key: 'five_hour', title: 'Current session' },
    { key: 'seven_day', title: 'This week' },
  ];
  const STATE_ICON = { ok: '', warn: ' ⚠', critical: ' ⛔' } as const;
  const STATE_TEXT = { ok: '', warn: 'near the limit', critical: 'almost out' } as const;

  const extras = $derived(windows.filter((w) => !COLUMNS.some((c) => c.key === w.key)));
  const extraWarn = $derived(extras.some((w) => levelOf(usedNow(w, now)) !== 'ok'));
  const hover = $derived(
    extras.length
      ? 'Other limits:\n' +
          extras.map((w) => `${w.label}: ${Math.round(usedNow(w, now))}% used${w.resets_at ? ' · ' + resetsLine(w.resets_at) : ''}`).join('\n')
      : '',
  );

  /** "Resets at 12:10 PM" today, "Resets Tuesday 7:00 AM" within a week, else the date. */
  function resetsLine(resetsAt: number | null): string {
    if (!resetsAt) return 'Reset time not reported';
    const d = new Date(resetsAt * 1000);
    if (d.getTime() <= now) return 'Has reset';
    const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    if (d.toDateString() === new Date(now).toDateString()) return `Resets at ${time}`;
    if (d.getTime() - now < 7 * 86_400_000) return `Resets ${d.toLocaleDateString([], { weekday: 'long' })} ${time}`;
    return `Resets ${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
  }
</script>

<div class="pair" title={hover || undefined}>
  {#each COLUMNS as c (c.key)}
    {@const w = windows.find((x) => x.key === c.key)}
    {#if w}
      {@const pct = usedNow(w, now)}
      {@const lv = levelOf(pct)}
      <div class="cell">
        <div class="top">
          <span class="title">{c.title}</span>
          <span class="pct">{Math.round(pct)}% used{STATE_ICON[lv]}</span>
        </div>
        <div
          class={`track ${lv}`}
          style={`--fill: var(--series-${provider})`}
          role="meter"
          aria-label={c.title}
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={Math.min(100, Math.round(pct))}
          aria-valuetext={`${Math.round(pct)}% used${lv !== 'ok' ? ', ' + STATE_TEXT[lv] : ''}, ${resetText(w.resets_at, now)}`}
        >
          <span class="fill" style={`width: ${Math.min(100, Math.max(0, pct))}%`}></span>
        </div>
        <span class="reset">{resetsLine(w.resets_at)}</span>
      </div>
    {:else}
      <div class="cell empty">
        <div class="top"><span class="title">{c.title}</span><span class="pct">–</span></div>
        <div class="track"><span class="fill" style="width: 0"></span></div>
        <span class="reset">Not reported yet</span>
      </div>
    {/if}
  {/each}
  {#if extras.length}
    <span class="more" aria-label={hover}>+{extras.length} more limit{extras.length === 1 ? '' : 's'} (hover){extraWarn ? ' ⚠' : ''}</span>
  {/if}
</div>

<style>
  .pair {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    column-gap: 20px;
    row-gap: 2px;
    min-width: 0;
  }
  .cell { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .top { display: flex; align-items: baseline; gap: 8px; font-size: 12.5px; }
  .title { color: var(--text); font-weight: 600; white-space: nowrap; }
  .pct { margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .reset { font-size: 11.5px; color: var(--muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .empty .title { color: var(--muted); }
  .track {
    --c: var(--fill, var(--muted));
    height: 6px;
    border-radius: 3px;
    /* a lighter step of the same color, so the state reads across the whole bar */
    background: color-mix(in oklab, var(--c) 22%, var(--surface));
    overflow: hidden;
  }
  .track.warn { --c: var(--status-warning); }
  .track.critical { --c: var(--status-critical); }
  .fill { display: block; height: 100%; border-radius: 3px; background: var(--c); }
  .more { grid-column: 1 / -1; font-size: 11px; color: var(--muted); cursor: help; justify-self: start; }
</style>
