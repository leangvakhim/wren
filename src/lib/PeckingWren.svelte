<script lang="ts">
  // The Perch wren, tipping forward from its feet to peck at crumbs (loading).
  // Holds still when the system asks for reduced motion.
  let { size = 18, color = 'var(--accent-fill)', hole = 'var(--surface)' }: { size?: number; color?: string; hole?: string } = $props();
</script>

<svg class="peck" width={size * 1.2} height={size} viewBox="0 0 120 100" aria-hidden="true">
  <g class="crumbs" fill="var(--muted)">
    <circle cx="101" cy="86" r="2.6" />
    <circle cx="108" cy="87.5" r="2" />
    <circle cx="114" cy="85.5" r="1.6" />
  </g>
  <!-- feet and the cursor perch stay put -->
  <g fill={color}>
    <rect x="41" y="74" width="4" height="12" />
    <rect x="53" y="74" width="4" height="12" />
    <rect x="24" y="84" width="52" height="7" rx="3.5" />
  </g>
  <!-- body, head and tail tip forward around the feet -->
  <g class="bird">
    <g fill={color}>
      <ellipse cx="47" cy="57" rx="25" ry="20" />
      <circle cx="65" cy="41" r="14" />
      <path d="M77 36 L93 41 L77 46 Z" />
      <path d="M30 52 L13 20 Q12 16 16 15 L21 15 Q24 15 25 18 L40 46 Z" />
    </g>
    <rect x="66" y="35" width="5" height="8" rx="1" fill={hole} />
  </g>
</svg>

<style>
  .peck { flex-shrink: 0; overflow: visible; }
  .bird {
    transform-box: view-box;
    transform-origin: 49px 76px;
    animation: peck 1.6s ease-in-out infinite;
  }
  .crumbs { animation: crumbs 1.6s ease-in-out infinite; }
  @keyframes peck {
    0%, 18% { transform: rotate(0deg); }
    34% { transform: rotate(44deg); }
    42% { transform: rotate(36deg); }
    50% { transform: rotate(44deg); }
    66%, 100% { transform: rotate(0deg); }
  }
  @keyframes crumbs {
    0%, 30% { opacity: 1; }
    40%, 60% { opacity: 0.55; }
    80%, 100% { opacity: 1; }
  }
  @media (prefers-reduced-motion: reduce) {
    .bird, .crumbs { animation: none; }
  }
</style>
