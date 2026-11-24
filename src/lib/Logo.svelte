<script lang="ts">
  // "Perch": a wren perched on a cursor, with a block-cursor eye.
  let { size = 24, color = 'currentColor', hole = 'var(--bg)' }: { size?: number; color?: string; hole?: string } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 100 100" aria-hidden="true">
  <g fill={color}>
    <ellipse cx="47" cy="57" rx="25" ry="20" />
    <circle cx="65" cy="41" r="14" />
    <path d="M77 36 L93 41 L77 46 Z" />
    <path d="M30 52 L13 20 Q12 16 16 15 L21 15 Q24 15 25 18 L40 46 Z" />
    <rect x="41" y="74" width="4" height="12" />
    <rect x="53" y="74" width="4" height="12" />
    <rect x="24" y="84" width="52" height="7" rx="3.5" />
  </g>
  <rect x="66" y="35" width="5" height="8" rx="1" fill={hole} />
</svg>
