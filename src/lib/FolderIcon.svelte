<script lang="ts">
  import { colorById, iconById } from './folderIcons';
  import { lookFor } from './folders.svelte';

  let { cwd, size = 14 }: { cwd: string; size?: number } = $props();

  const look = $derived(lookFor(cwd));
  const color = $derived(colorById(look.color));
</script>

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
  {@html iconById(look.icon).body}
</svg>
