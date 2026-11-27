<script lang="ts">
  import { colorById } from './folderIcons';
  import { lookFor } from './folders.svelte';
  import { BUILT_IN, glyph } from './iconCatalog.svelte';

  let { cwd, size = 14 }: { cwd: string; size?: number } = $props();

  const look = $derived(lookFor(cwd));
  const color = $derived(colorById(look.color));
  // The plain folder while an icon set loads, or if the icon was removed.
  const g = $derived(glyph(look.icon) ?? { kind: 'svg' as const, body: BUILT_IN[0].body });
</script>

<!-- Emoji and pictures are shown as text / <img>, never as markup. -->
{#if g.kind === 'emoji'}
  <span class="emo" style:width="{size}px" style:height="{size}px" style:font-size="{size * 0.95}px" aria-hidden="true">{g.text}</span>
{:else if g.kind === 'img'}
  <img class="pic" src={g.url} alt="" width={size} height={size} draggable="false" />
{:else}
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="2"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    style:color={color || null}
    style:flex-shrink="0"
  >
    {@html g.body}
  </svg>
{/if}

<style>
  .emo { display: inline-grid; place-items: center; flex-shrink: 0; line-height: 1; font-family: 'Apple Color Emoji', 'Segoe UI Emoji', 'Noto Color Emoji', sans-serif; }
  .pic { flex-shrink: 0; object-fit: contain; border-radius: 3px; }
</style>
