<script lang="ts">
  let {
    ariaLabel = "Reader controls",
    fixed = false,
    hiddenChrome = false,
  }: {
    ariaLabel?: string;
    fixed?: boolean;
    hiddenChrome?: boolean;
  } = $props();
</script>

<header
  class="reader-topbar"
  class:fixed
  class:hidden-chrome={hiddenChrome}
  aria-label={ariaLabel}
>
  <div class="reader-topbar-side">
    <slot name="left" />
  </div>
  <div class="reader-topbar-center">
    <slot />
  </div>
  <div class="reader-topbar-side reader-topbar-side--right">
    <slot name="right" />
  </div>
</header>

<style>
  .reader-topbar {
    --reader-topbar-height: 3.625rem;
    --reader-topbar-top: calc(
      var(--reader-topbar-height, 3.625rem) + env(safe-area-inset-top)
    );

    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
    box-sizing: border-box;
    background: var(--color-surface-bg);
    height: var(--reader-topbar-top);
    min-height: var(--reader-topbar-top);
    padding: env(safe-area-inset-top) max(14px, env(safe-area-inset-right)) 0
      max(14px, env(safe-area-inset-left));
    border-bottom: 1px solid var(--color-border);
    transition:
      opacity 160ms ease,
      transform 120ms ease;
  }

  .reader-topbar.fixed {
    position: fixed;
    z-index: 10;
    top: 0;
    left: 0;
    right: 0;
    transform: translateY(var(--reader-topbar-shift, 0px));
  }

  .reader-topbar.hidden-chrome {
    height: 0;
    min-height: 0;
    opacity: 0;
    pointer-events: none;
  }

  .reader-topbar-side {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .reader-topbar-side--right {
    justify-content: flex-end;
  }

  .reader-topbar-center {
    min-width: 0;
    flex: 1;
    display: grid;
    padding-inline: 5px;
  }

  .reader-topbar strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-interface-family);
    font-size: 1rem;
    font-weight: 600;
    line-height: 1.1;
    margin: 0;
  }

  .reader-topbar-title {
    min-width: 9rem;
    display: grid;
    padding-inline: 5px;
  }

  .reader-topbar output {
    min-width: 44px;
    text-align: right;
    color: var(--color-ink-muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
  }

  @media (prefers-reduced-motion: reduce) {
    .reader-topbar {
      transition: none;
    }
  }
</style>
