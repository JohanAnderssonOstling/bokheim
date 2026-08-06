<script lang="ts">
  let {
    progress = 0,
    active = false,
    complete = false,
    label,
  }: {
    progress?: number;
    active?: boolean;
    complete?: boolean;
    label: string;
  } = $props();

  const clampedProgress = $derived(Math.min(1, Math.max(0, progress)));
  const dashOffset = $derived(100 - clampedProgress * 100);
</script>

{#if complete}
  <span class="indicator complete" aria-label={label} title={label}>
    <svg class="check" viewBox="0 0 24 24" aria-hidden="true"><path d="m6.5 12.5 3.4 3.4 7.6-8"></path></svg>
  </span>
{:else}
  <span class:active class="indicator" role="img" aria-label={label} title={label}>
    <svg class="ring" viewBox="0 0 36 36" aria-hidden="true">
      <circle class="track" cx="18" cy="18" r="15.9"></circle>
      <circle class="value" cx="18" cy="18" r="15.9" style={`stroke-dashoffset: ${dashOffset}`}></circle>
    </svg>
    {#if active && clampedProgress === 0}
      <span class="dot"></span>
    {:else}
      <span class="state-dot"></span>
    {/if}
  </span>
{/if}

<style>
  .indicator { position: relative; display: grid; width: 2rem; height: 2rem; padding: 0; place-items: center; border: 0; border-radius: 50%; color: var(--color-accent); background: color-mix(in srgb, var(--color-white-surface) 92%, transparent); box-shadow: 0 .1rem .35rem color-mix(in srgb, var(--color-box-shadow) 45%, transparent); }
  .indicator.complete { width: 1.55rem; height: 1.55rem; color: color-mix(in srgb, var(--color-accent) 75%, var(--color-subtle-text)); background: color-mix(in srgb, var(--color-white-surface) 82%, transparent); box-shadow: none; }
  .ring { position: absolute; inset: 0; width: 100%; height: 100%; transform: rotate(-90deg); fill: none; }
  .track, .value { fill: none; stroke-width: 2.2; }
  .track { stroke: color-mix(in srgb, var(--color-border-color) 75%, transparent); }
  .value { stroke: currentColor; stroke-dasharray: 100; stroke-linecap: round; transition: stroke-dashoffset .2s ease; }
  .check { position: relative; width: 1rem; height: 1rem; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.9; }
  .check { width: .95rem; height: .95rem; stroke-width: 2.2; }
  .state-dot { width: .35rem; height: .35rem; border-radius: 50%; background: currentColor; opacity: .55; }
  .dot { width: .35rem; height: .35rem; border-radius: 50%; background: currentColor; animation: pulse 1s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: .35; transform: scale(.75); } }
  @media (prefers-reduced-motion: reduce) { .value { transition: none; } .dot { animation: none; } }
</style>
