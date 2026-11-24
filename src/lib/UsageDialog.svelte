<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { accounts, findAccount } from './accounts.svelte';
  import LimitBars from './LimitBars.svelte';
  import { planLimits, refreshLimits, sourceText } from './planLimits.svelte';
  import { PROVIDERS, type Provider, type UsageRecord } from './types';

  let { open = $bindable(false) }: { open: boolean } = $props();

  type Rec = UsageRecord & { conversation: string; title: string; cwd: string };

  let dlg: HTMLDialogElement | undefined = $state();
  let records = $state<Rec[]>([]);
  let loading = $state(false);
  let range = $state<7 | 30 | 90 | 0>(30);
  let metric = $state<'cost' | 'tokens'>('cost');
  let showTable = $state(false);
  let hover = $state<number | null>(null);

  $effect(() => {
    if (!dlg) return;
    if (open && !dlg.open) {
      dlg.showModal();
      now = Date.now();
      load();
      refreshAll(120_000);
    }
    if (!open && dlg.open) dlg.close();
  });

  async function load() {
    loading = true;
    try {
      records = await invoke<Rec[]>('usage_records');
    } catch {
      records = [];
    }
    loading = false;
  }

  const AIS: Provider[] = ['claude', 'codex', 'antigravity'];
  let now = $state(Date.now());

  // ---- plan limits (from each AI's own CLI) ----
  /** Accounts that can have plan limits (Antigravity can't be read; see the note). */
  const limitAccounts = $derived(
    accounts.list.filter((a) => a.provider !== 'antigravity' && (a.status === 'signed_in' || (a.is_default && a.status === 'unknown'))),
  );
  function refreshAll(minAge = 0) {
    for (const a of limitAccounts) refreshLimits(a.id, a.provider, minAge);
  }
  const anyBusy = $derived(limitAccounts.some((a) => planLimits.busy[a.id]));

  // ---- daily usage per account (last 7 days) ----
  const last7 = $derived.by(() => {
    const out: { key: string; date: Date }[] = [];
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    for (let i = 6; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(d.getDate() - i);
      out.push({ key: dayKey(d.getTime()), date: d });
    }
    return out;
  });
  const dailyRows = $derived.by(() => {
    const keys = new Set(last7.map((d) => d.key));
    const map = new Map<string, Record<string, number>>();
    for (const r of records) {
      const k = dayKey(r.t);
      if (!keys.has(k)) continue;
      const row = map.get(r.accountId) ?? {};
      row[k] = (row[k] ?? 0) + r.input + r.cached + r.output;
      map.set(r.accountId, row);
    }
    // Codex accounts that OpenAI reported daily numbers for also get a row.
    for (const a of accounts.list) {
      if (planLimits.byAccount[a.id]?.daily?.length && !map.has(a.id)) map.set(a.id, {});
    }
    return [...map.entries()].map(([id, by]) => {
      const a = findAccount(id);
      const server: Record<string, number> = {};
      for (const d of planLimits.byAccount[id]?.daily ?? []) server[d.date] = d.tokens;
      const hasServer = Object.keys(server).length > 0;
      return {
        id,
        provider: a?.provider,
        label: a ? `${PROVIDERS[a.provider].label} · ${a.name}` : id,
        by,
        total: last7.reduce((s, d) => s + (by[d.key] ?? 0), 0),
        server: hasServer ? server : null,
        serverTotal: last7.reduce((s, d) => s + (server[d.key] ?? 0), 0),
      };
    });
  });
  const dayKey = (t: number) => {
    const d = new Date(t);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  };

  const inRange = $derived.by(() => {
    if (!range) return records;
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    start.setDate(start.getDate() - (range - 1));
    return records.filter((r) => r.t >= start.getTime());
  });

  const value = (r: Rec) => (metric === 'cost' ? (r.cost ?? 0) : r.input + r.cached + r.output);

  const kpi = $derived.by(() => {
    let cost = 0,
      input = 0,
      cached = 0,
      output = 0,
      unpriced = 0;
    const convs = new Set<string>();
    for (const r of inRange) {
      if (r.cost == null) unpriced++;
      else cost += r.cost;
      input += r.input;
      cached += r.cached;
      output += r.output;
      convs.add(r.conversation);
    }
    return { cost, input, cached, output, unpriced, turns: inRange.length, convs: convs.size };
  });

  /** One column per day, stacked by AI. */
  const days = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    let n: number = range;
    if (!n) {
      const first = inRange.reduce((m, r) => Math.min(m, r.t), Date.now());
      n = Math.min(365, Math.max(1, Math.ceil((today.getTime() - first) / 86_400_000) + 1));
    }
    const list: { key: string; date: Date; by: Record<Provider, number>; total: number }[] = [];
    for (let i = n - 1; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(d.getDate() - i);
      list.push({ key: dayKey(d.getTime()), date: d, by: { claude: 0, codex: 0, antigravity: 0 }, total: 0 });
    }
    const idx = new Map(list.map((d, i) => [d.key, i]));
    for (const r of inRange) {
      const i = idx.get(dayKey(r.t));
      if (i === undefined) continue;
      const v = value(r);
      list[i].by[r.provider] = (list[i].by[r.provider] ?? 0) + v;
      list[i].total += v;
    }
    return list;
  });

  // ---- chart geometry ----
  const W = 680,
    H = 200,
    PAD_L = 52,
    PAD_B = 22,
    PAD_T = 8;
  const max = $derived(niceMax(Math.max(0, ...days.map((d) => d.total))));
  const plotW = W - PAD_L - 8;
  const plotH = H - PAD_B - PAD_T;
  const slot = $derived(plotW / Math.max(1, days.length));
  const barW = $derived(Math.max(2, Math.min(28, slot * 0.7)));
  const y = (v: number) => PAD_T + plotH - (max ? (v / max) * plotH : 0);

  function niceMax(v: number) {
    if (v <= 0) return metric === 'cost' ? 1 : 1000;
    const p = Math.pow(10, Math.floor(Math.log10(v)));
    for (const m of [1, 2, 2.5, 5, 10]) if (m * p >= v) return m * p;
    return 10 * p;
  }

  const fmtTok = (n: number) =>
    n >= 1e9 ? (n / 1e9).toFixed(1) + 'B' : n >= 1e6 ? (n / 1e6).toFixed(1) + 'M' : n >= 1e3 ? (n / 1e3).toFixed(1) + 'k' : String(Math.round(n));
  const fmtUsd = (n: number) => (n > 0 && n < 0.01 ? '<$0.01' : '$' + n.toFixed(2));
  const fmt = (v: number) => (metric === 'cost' ? fmtUsd(v) : fmtTok(v));
  const fmtDay = (d: Date) => d.toLocaleDateString([], { month: 'short', day: 'numeric' });

  /** Stacked segments for one day: bottom to top, with a 2px gap between segments. */
  function segments(d: (typeof days)[number]) {
    let base = 0;
    const out: { p: Provider; y: number; h: number; top: boolean }[] = [];
    const present = AIS.filter((p) => d.by[p] > 0);
    present.forEach((p, i) => {
      const y0 = y(base);
      const y1 = y(base + d.by[p]);
      base += d.by[p];
      const gap = i > 0 ? 2 : 0;
      out.push({ p, y: y1, h: Math.max(1, y0 - y1 - gap), top: i === present.length - 1 });
    });
    return out;
  }

  /** Breakdown rows: name, cost, tokens, share of the chosen metric. */
  function breakdown(keyOf: (r: Rec) => string, labelOf: (k: string) => string) {
    const map = new Map<string, { cost: number; tokens: number; v: number }>();
    for (const r of inRange) {
      const k = keyOf(r);
      const e = map.get(k) ?? { cost: 0, tokens: 0, v: 0 };
      e.cost += r.cost ?? 0;
      e.tokens += r.input + r.cached + r.output;
      e.v += value(r);
      map.set(k, e);
    }
    const total = [...map.values()].reduce((s, e) => s + e.v, 0) || 1;
    return [...map.entries()]
      .map(([k, e]) => ({ key: k, label: labelOf(k), ...e, share: e.v / total }))
      .sort((a, b) => b.v - a.v)
      .slice(0, 8);
  }

  const byAI = $derived(breakdown((r) => r.provider, (k) => PROVIDERS[k as Provider]?.label ?? k));
  const byAccount = $derived(
    breakdown(
      (r) => r.accountId,
      (k) => {
        const a = findAccount(k);
        return a ? `${PROVIDERS[a.provider].label} · ${a.name}` : k;
      },
    ),
  );
  const byModel = $derived(breakdown((r) => r.model || '(default)', (k) => k));
  const byFolder = $derived(breakdown((r) => r.cwd || '(no folder)', (k) => k.split(/[\\/]/).filter(Boolean).pop() || k));
  const ticks = $derived([0, 0.25, 0.5, 0.75, 1].map((f) => f * max));
  // date labels counted back from today, evenly spaced (no collisions)
  const labelStep = $derived(Math.max(1, Math.ceil(days.length / 7)));
  type Row = ReturnType<typeof breakdown>[number];
  const sections = $derived<{ title: string; rows: Row[]; colored: boolean }[]>([
    { title: 'By AI', rows: byAI, colored: true },
    { title: 'By account', rows: byAccount, colored: false },
    { title: 'By model', rows: byModel, colored: false },
    { title: 'By project folder', rows: byFolder, colored: false },
  ]);
</script>

<dialog bind:this={dlg} onclose={() => (open = false)} aria-labelledby="usage-title">
  <div class="head">
    <h2 id="usage-title">Usage</h2>
    <div class="seg" role="group" aria-label="Time range">
      {#each [[7, '7 days'], [30, '30 days'], [90, '90 days'], [0, 'All']] as [v, l]}
        <button class:on={range === v} aria-pressed={range === v} onclick={() => (range = v as 7 | 30 | 90 | 0)}>{l}</button>
      {/each}
    </div>
    <button class="ghost" onclick={() => (open = false)}>Close</button>
  </div>
  <section class="limits">
    <div class="charthead">
      <h3>Plan limits right now</h3>
      <span class="spacer"></span>
      <button class="ghost" disabled={anyBusy || !limitAccounts.length} onclick={() => { now = Date.now(); refreshAll(); }}>{anyBusy ? 'Reading…' : 'Refresh'}</button>
    </div>
    <p class="sub">Read from each AI's own CLI with its own sign-in (Wren never sees your tokens). Kept only on this computer.</p>
    {#if limitAccounts.length}
      <div class="limrows">
        {#each limitAccounts as a (a.id)}
          {@const lim = planLimits.byAccount[a.id]}
          <div class="limrow">
            <div class="who">
              <span class="limtitle"><span class="sw" style={`background: var(--series-${a.provider})`}></span><b>{PROVIDERS[a.provider].label} · {a.name}</b></span>
              <span class="limnote">
                {#if lim?.plan}{`${lim.plan[0].toUpperCase()}${lim.plan.slice(1)} plan · `}{/if}{#if lim?.windows.length}{sourceText(lim, now)}{#if planLimits.busy[a.id]} · reading…{:else if lim.error} · <span class="warnmark" title={lim.error}>⚠ latest read failed</span>{/if}{:else if planLimits.busy[a.id]}reading…{:else if lim?.error}couldn't read{:else if lim}{sourceText(lim, now)}{:else}not read yet{/if}
              </span>
            </div>
            {#if lim?.error && !lim.windows.length}
              <p class="limnote err">{lim.error}</p>
            {:else}
              <LimitBars windows={lim?.windows ?? []} provider={a.provider} {now} />
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <p class="muted small">No signed-in Claude Code or Codex accounts yet.</p>
    {/if}
    <p class="muted small">Antigravity: its quota is only shown in agy's own <code>/usage</code> screen (Accounts → Open /usage). Wren's own count is below.</p>
  </section>

  <p class="sub">From your conversations in Wren, saved only on this computer.</p>

  {#if loading}
    <p class="muted">Loading…</p>
  {:else if records.length === 0}
    <p class="muted">No usage yet. Send a message in any pane and it will show up here.</p>
  {:else}
    <div class="tiles">
      <div class="tile">
        <span class="k">Estimated cost</span>
        <span class="v">{fmtUsd(kpi.cost)}</span>
        <span class="s">at API prices{kpi.unpriced ? ` · ${kpi.unpriced} answer${kpi.unpriced === 1 ? '' : 's'} without a known price` : ''}</span>
      </div>
      <div class="tile">
        <span class="k">Tokens read</span>
        <span class="v">{fmtTok(kpi.input + kpi.cached)}</span>
        <span class="s">{fmtTok(kpi.cached)} of them from cache</span>
      </div>
      <div class="tile">
        <span class="k">Tokens written</span>
        <span class="v">{fmtTok(kpi.output)}</span>
        <span class="s">the AIs' answers and edits</span>
      </div>
      <div class="tile">
        <span class="k">Activity</span>
        <span class="v">{kpi.turns}</span>
        <span class="s">answers in {kpi.convs} conversation{kpi.convs === 1 ? '' : 's'}</span>
      </div>
    </div>

    <div class="charthead">
      <h3>{metric === 'cost' ? 'Estimated cost per day' : 'Tokens per day'}</h3>
      <div class="seg small" role="group" aria-label="Measure">
        <button class:on={metric === 'cost'} aria-pressed={metric === 'cost'} onclick={() => (metric = 'cost')}>Cost</button>
        <button class:on={metric === 'tokens'} aria-pressed={metric === 'tokens'} onclick={() => (metric = 'tokens')}>Tokens</button>
      </div>
      <span class="spacer"></span>
      <ul class="legend" aria-label="Legend">
        {#each AIS as p}<li><span class="sw" style={`background: var(--series-${p})`}></span>{PROVIDERS[p].label}</li>{/each}
      </ul>
      <button class="ghost" onclick={() => (showTable = !showTable)}>{showTable ? 'Show chart' : 'Show table'}</button>
    </div>

    {#if showTable}
      <div class="tablewrap">
        <table>
          <thead><tr><th>Day</th>{#each AIS as p}<th class="num">{PROVIDERS[p].label}</th>{/each}<th class="num">Total</th></tr></thead>
          <tbody>
            {#each [...days].reverse().filter((d) => d.total > 0) as d}
              <tr><td>{fmtDay(d.date)}</td>{#each AIS as p}<td class="num">{fmt(d.by[p])}</td>{/each}<td class="num"><b>{fmt(d.total)}</b></td></tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else}
      <div class="chart">
        <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={`${metric === 'cost' ? 'Estimated cost' : 'Tokens'} per day, stacked by AI`}>
          {#each ticks as t}
            <line x1={PAD_L} x2={W - 8} y1={y(t)} y2={y(t)} class="grid" />
            <text x={PAD_L - 6} y={y(t) + 4} class="axis" text-anchor="end">{fmt(t)}</text>
          {/each}
          {#each days as d, i}
            {@const cx = PAD_L + slot * i + slot / 2}
            {#each segments(d) as sgm}
              <path
                d={sgm.top
                  ? `M${cx - barW / 2},${sgm.y + sgm.h} V${sgm.y + Math.min(4, sgm.h)} Q${cx - barW / 2},${sgm.y} ${cx - barW / 2 + Math.min(4, barW / 2)},${sgm.y} H${cx + barW / 2 - Math.min(4, barW / 2)} Q${cx + barW / 2},${sgm.y} ${cx + barW / 2},${sgm.y + Math.min(4, sgm.h)} V${sgm.y + sgm.h} Z`
                  : `M${cx - barW / 2},${sgm.y} H${cx + barW / 2} V${sgm.y + sgm.h} H${cx - barW / 2} Z`}
                fill={`var(--series-${sgm.p})`}
                opacity={hover === null || hover === i ? 1 : 0.45}
              />
            {/each}
            {#if (days.length - 1 - i) % labelStep === 0}
              <text x={cx} y={H - 6} class="axis" text-anchor="middle">{fmtDay(d.date)}</text>
            {/if}
            <!-- hover target wider than the bar -->
            <rect
              x={PAD_L + slot * i}
              y={PAD_T}
              width={slot}
              height={plotH}
              fill="transparent"
              role="presentation"
              onpointerenter={() => (hover = i)}
              onpointerleave={() => (hover = null)}
            />
          {/each}
          <line x1={PAD_L} x2={W - 8} y1={y(0)} y2={y(0)} class="base" />
        </svg>
        {#if hover !== null && days[hover]}
          {@const d = days[hover]}
          <div class="tip" style={`left: ${((PAD_L + slot * hover + slot / 2) / W) * 100}%`}>
            <b>{fmtDay(d.date)}</b>
            {#each AIS as p}
              {#if d.by[p] > 0}<div><span class="sw" style={`background: var(--series-${p})`}></span>{PROVIDERS[p].label}: {fmt(d.by[p])}</div>{/if}
            {/each}
            <div class="tot">Total: {fmt(d.total)}</div>
          </div>
        {/if}
      </div>
    {/if}

    <div class="grid4">
      {#each sections as { title, rows, colored }}
        <section class="bd">
          <h3>{title}</h3>
          <table>
            <thead><tr><th>Name</th><th class="num">Cost</th><th class="num">Tokens</th><th class="sharecol">Share of {metric}</th></tr></thead>
            <tbody>
              {#each rows as r (r.key)}
                <tr>
                  <td class="name" title={r.key}>{r.label}</td>
                  <td class="num">{fmtUsd(r.cost)}</td>
                  <td class="num">{fmtTok(r.tokens)}</td>
                  <td class="sharecol">
                    <div class="share">
                      <span class="bar"><span style={`width: ${Math.max(2, r.share * 100)}%; background: ${colored ? `var(--series-${r.key})` : 'var(--muted)'}`}></span></span>
                      <span class="pct">{Math.round(r.share * 100)}%</span>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </section>
      {/each}
    </div>

    <section class="daily">
      <h3>Daily usage per account <span class="muted">(tokens, last 7 days)</span></h3>
      <div class="tablewrap">
        <table>
          <thead>
            <tr><th>Account</th>{#each last7 as d}<th class="num">{d.date.toLocaleDateString([], { weekday: 'short', day: 'numeric' })}</th>{/each}<th class="num">7 days</th></tr>
          </thead>
          <tbody>
            {#each dailyRows as r (r.id)}
              <tr>
                <td class="name" title={r.label}>{#if r.provider}<span class="sw" style={`background: var(--series-${r.provider})`}></span>{/if}{r.label}</td>
                {#each last7 as d}<td class="num">{r.by[d.key] ? fmtTok(r.by[d.key]) : '–'}</td>{/each}
                <td class="num"><b>{fmtTok(r.total)}</b></td>
              </tr>
              {#if r.server}
                <tr class="server">
                  <td class="name">↳ counted by OpenAI (all devices)</td>
                  {#each last7 as d}<td class="num">{r.server[d.key] ? fmtTok(r.server[d.key]) : '–'}</td>{/each}
                  <td class="num">{fmtTok(r.serverTotal)}</td>
                </tr>
              {/if}
            {/each}
          </tbody>
        </table>
      </div>
    </section>

    <details class="explain">
      <summary>How to read this</summary>
      <ul>
        <li><b>Tokens read</b>: everything the AI reads: your message, the files it opens, and the conversation so far. Long conversations re-read their history every turn, so this grows fast.</li>
        <li><b>From cache</b>: parts the AI already read recently. They're much cheaper (often 90% less).</li>
        <li><b>Tokens written</b>: the AI's answers, plans and code edits. Writing costs more per token than reading.</li>
        <li><b>Estimated cost</b>: what this usage would cost at each company's public API prices. Claude Code reports its own number; Codex and Antigravity are estimated by Wren. On a monthly subscription you aren't billed per token, so treat it as a way to compare, not a bill.</li>
        <li><b>Plan limits</b>: subscriptions limit how much you can use in a rolling <b>5-hour</b> window and per <b>week</b>. The bars come from the AI itself and change colour at 80% (⚠) and 95% (⛔). A window that has passed its reset time counts as 0% until it's read again.</li>
        <li><b>Daily usage per account</b>: tokens Wren counted in its own conversations. For Codex, OpenAI's own count (including other devices and apps) is shown under it.</li>
        <li><b>Tips</b>: start a new conversation (<code>\new</code>) when you change topic, use a smaller model or lower effort for simple tasks, and point the AI at specific files instead of the whole project.</li>
      </ul>
    </details>
  {/if}
</dialog>

<style>
  dialog {
    width: min(820px, calc(100vw - 40px));
    max-height: calc(100vh - 40px);
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    padding: 18px 20px;
  }
  dialog::backdrop { background: rgb(0 0 0 / 0.45); }
  .head { display: flex; align-items: center; gap: 12px; }
  .head h2 { margin: 0; font-size: 18px; flex: 1; }
  .sub { margin: 4px 0 12px; color: var(--muted); font-size: 12px; }
  .muted { color: var(--muted); }
  .seg { display: flex; background: var(--surface-2); border: 1px solid var(--border); border-radius: 9px; padding: 2px; gap: 2px; }
  .seg button { border: 0; background: transparent; color: var(--muted); padding: 4px 10px; min-height: 28px; border-radius: 7px; font-size: 12.5px; cursor: pointer; }
  .seg button.on { background: var(--surface); color: var(--text); box-shadow: 0 1px 2px rgb(0 0 0 / 0.12); }
  .tiles { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; }
  .tile { background: var(--surface-2); border: 1px solid var(--border); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 2px; }
  .tile .k { font-size: 12px; color: var(--muted); }
  .tile .v { font-size: 22px; font-weight: 650; font-variant-numeric: tabular-nums; }
  .tile .s { font-size: 11px; color: var(--muted); }
  .charthead { display: flex; align-items: center; gap: 10px; margin: 16px 0 6px; flex-wrap: wrap; }
  .charthead h3 { margin: 0; font-size: 14px; }
  .spacer { flex: 1; }
  .legend { list-style: none; display: flex; gap: 12px; margin: 0; padding: 0; font-size: 12px; color: var(--text); }
  .legend li { display: flex; align-items: center; gap: 5px; }
  .sw { display: inline-block; width: 10px; height: 10px; border-radius: 3px; margin-right: 4px; vertical-align: -1px; }
  .chart { position: relative; }
  svg { width: 100%; height: auto; display: block; }
  .grid { stroke: var(--grid); stroke-width: 1; }
  .base { stroke: var(--muted); stroke-width: 1; }
  .axis { fill: var(--muted); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .tip {
    position: absolute;
    top: 0;
    transform: translateX(-50%);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 6px 10px;
    font-size: 12px;
    pointer-events: none;
    box-shadow: 0 6px 18px rgb(0 0 0 / 0.18);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .tip .tot { border-top: 1px solid var(--border); margin-top: 4px; padding-top: 4px; font-weight: 600; }
  .tablewrap { max-height: 240px; overflow: auto; border: 1px solid var(--border); border-radius: 8px; }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th, td { padding: 5px 8px; text-align: left; border-bottom: 1px solid var(--border); }
  th { color: var(--muted); font-weight: 600; font-size: 11.5px; }
  .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .grid4 { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 14px; margin-top: 16px; }
  .bd h3 { margin: 0 0 4px; font-size: 13px; }
  .name { max-width: 150px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sharecol { width: 34%; }
  .share { display: flex; align-items: center; gap: 6px; }
  .bar { flex: 1; height: 6px; background: var(--surface-2); border-radius: 3px; overflow: hidden; }
  .bar span { display: block; height: 100%; border-radius: 3px; }
  .pct { font-size: 11px; color: var(--muted); width: 32px; text-align: right; font-variant-numeric: tabular-nums; }
  .explain { margin-top: 14px; font-size: 13px; }
  .limits { margin: 4px 0 14px; padding-bottom: 12px; border-bottom: 1px solid var(--border); }
  .limits .charthead { margin-top: 6px; }
  .limrows { display: flex; flex-direction: column; border: 1px solid var(--border); border-radius: 10px; overflow: hidden; }
  .limrow { display: grid; grid-template-columns: minmax(150px, 0.7fr) minmax(0, 2fr); gap: 16px; align-items: center; padding: 10px 12px; }
  .limrow + .limrow { border-top: 1px solid var(--border); }
  .who { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .limtitle { font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .limnote { margin: 0; font-size: 11.5px; color: var(--muted); }
  .limnote.err { color: var(--text); }
  .warnmark { cursor: help; text-decoration: underline dotted; }
  @media (max-width: 640px) { .limrow { grid-template-columns: minmax(0, 1fr); gap: 8px; } }
  .small { font-size: 12px; margin: 8px 0 0; }
  .daily { margin-top: 16px; }
  .daily h3 { margin: 0 0 6px; font-size: 13px; }
  .daily .name { max-width: 200px; }
  tr.server td { color: var(--muted); font-size: 11.5px; }
  .explain summary { cursor: pointer; font-weight: 600; }
  .explain ul { margin: 8px 0 0; padding-left: 18px; display: flex; flex-direction: column; gap: 6px; line-height: 1.5; color: var(--text); }
  @media (max-width: 700px) {
    .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .grid4 { grid-template-columns: minmax(0, 1fr); }
  }
</style>
