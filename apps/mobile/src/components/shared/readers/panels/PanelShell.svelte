<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    children,
    variant,
    ariaLabel,
  }: {
    children: Snippet;
    variant: "sidebar" | "info" | "settings";
    ariaLabel: string;
  } = $props();
</script>

<section
  class="reader-panel"
  class:reader-sidebar={variant === "sidebar"}
  class:info-panel={variant === "info"}
  class:settings-panel={variant === "settings"}
  aria-label={ariaLabel}
>
  {@render children()}
</section>

<style>
  .reader-panel {
    position: absolute;
    z-index: 6;
    top: var(
      --reader-panel-top,
      calc(var(--reader-topbar-height, 3.625rem) + env(safe-area-inset-top))
    );
    right: max(0.65rem, env(safe-area-inset-right));
    width: min(
      28rem,
      calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right))
    );
    max-height: calc(
      100dvh - 8rem - env(safe-area-inset-top) - env(safe-area-inset-bottom)
    );
    box-sizing: border-box;
    overflow: auto;
    padding: 1rem;
    border: 1px solid var(--chrome-strong);
    border-radius: 0;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 1rem 3rem color-mix(in srgb, var(--color-ink-text) 24%, transparent);
  }

  .reader-sidebar,
  .info-panel {
    left: 0;
    right: auto;
    bottom: 0;
    width: min(380px, 40vw);
    max-height: none;
    border-width: 0 1px 0 0;
    border-radius: 0;
    box-shadow: none;
  }

  .reader-sidebar {
    display: flex;
    flex-direction: column;
    gap: 0;
    margin: 0;
    padding: 0;
  }

  .settings-panel {
    display: flex;
    flex-direction: column;
    gap: var(--reader-settings-gap);
    width: min(
      var(--reader-settings-max-width),
      calc(100% - 1.3rem - env(safe-area-inset-left) - env(safe-area-inset-right))
    );
    max-height: min(
      var(--reader-settings-max-height),
      calc(100dvh - var(--reader-topbar-height, 3.625rem) - env(safe-area-inset-top))
    );
    padding: var(--reader-settings-padding);
    border-radius: 0;
  }

  @media (max-width: 720px) {
    .reader-sidebar {
      left: 0;
      right: 0;
      width: 100%;
      border-inline: 0;
      border-radius: 0;
    }

    .info-panel {
      right: 0;
      width: 100%;
      border-inline: 0;
    }
  }
</style>
