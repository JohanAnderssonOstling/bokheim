<script lang="ts">
  import download from "@icons/shared/download.svg";
  import FileIcon from "../FileIcon.svelte";
  let {
    progress = 0,
    active = false,
    label,
  }: {
    progress?: number;
    active?: boolean;
    label: string;
  } = $props();

  const clampedProgress = $derived(Math.min(1, Math.max(0, progress)));
  const dashOffset = $derived(100 - clampedProgress * 100);
</script>

<!-- Progress belongs to a running folder download. A folder that merely holds
     some downloaded books offers the download instead: a half-filled ring would
     read as work in flight. Callers hide the indicator once nothing is left. -->
{#if active}
  <span
    class="indicator active"
    role="img"
    aria-label={label}
    title={label}
  >
    <svg
      class="ring"
      viewBox="0 0 36 36"
      aria-hidden="true"
    >
      <circle
        class="track"
        cx="18"
        cy="18"
        r="15.9"
      ></circle>
      <circle
        class="value"
        cx="18"
        cy="18"
        r="15.9"
        style={`stroke-dashoffset: ${dashOffset}`}
      ></circle>
    </svg>
    {#if clampedProgress === 0}
      <span class="dot"></span>
    {:else}
      <span class="state-dot"></span>
    {/if}
  </span>
{:else}
  <span
    class="indicator"
    role="img"
    aria-label={label}
    title={label}
  >
    <FileIcon source={download} />
  </span>
{/if}

<style>
  .indicator {
    position: relative;
    display: grid;
    width: 2rem;
    height: 2rem;
    padding: 0;
    place-items: center;
    border: 0;
    border-radius: 50%;
    color: var(--color-accent);
    background: color-mix(in srgb, var(--color-raised-bg) 92%, transparent);
    box-shadow: 0 0.1rem 0.35rem
      color-mix(in srgb, var(--color-box-shadow) 45%, transparent);
  }
  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
    fill: none;
  }
  .track,
  .value {
    fill: none;
    stroke-width: 2.2;
  }
  .track {
    stroke: color-mix(in srgb, var(--color-border) 75%, transparent);
  }
  .value {
    stroke: currentColor;
    stroke-dasharray: 100;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.2s ease;
  }
  .state-dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.55;
  }
  .dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 50%;
    background: currentColor;
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
      transform: scale(0.75);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .value {
      transition: none;
    }
    .dot {
      animation: none;
    }
  }
</style>
