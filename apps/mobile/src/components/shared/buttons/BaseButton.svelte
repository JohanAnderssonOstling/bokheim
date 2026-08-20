<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * The shared body of the boxed buttons. Not for direct use: reach for
   * `PrimaryButton`, `SecondaryButton`, `DangerButton` or `FormButton`, which
   * exist so a call site names the button's job rather than its appearance.
   *
   * Everything the boxed buttons agree on lives here — geometry, type scale,
   * disabled treatment — so the four of them cannot drift apart the way the
   * per-page rules this replaced had (2.1rem to 2.7rem tall, weights 600 to
   * 760). `variant` picks the only thing they disagree about, which is colour.
   *
   * Heights and type sizes are `rem` so they follow the UI font preference;
   * padding and gaps stay in `px` because scaling them only loosens dense
   * views. See `lib/shared/layout-tokens.ts` for the same split.
   *
   * `TextButton` and `MenuItemButton` deliberately do not build on this: a
   * borderless link and a full-width menu row would have to override nearly
   * every rule below, which is worse than stating their own.
   */
  let {
    children,
    variant,
    type = "button",
    disabled = false,
    active = false,
    ariaLabel,
    ariaPressed,
    onclick,
  }: {
    children: Snippet;
    variant: "primary" | "secondary" | "danger";
    type?: "button" | "submit";
    disabled?: boolean;
    active?: boolean;
    ariaLabel?: string;
    ariaPressed?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  class={["base-button", variant, active && "active"]}
  {type}
  {disabled}
  aria-label={ariaLabel}
  aria-pressed={ariaPressed}
  {onclick}
>
  {@render children()}
</button>

<style>
  .base-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 2.5rem;
    padding: 8px 14px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    font-family: inherit;
    font-size: 0.8125rem;
    font-weight: 700;
    line-height: 1.15;
    white-space: nowrap;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .base-button.primary {
    border-color: var(--color-accent);
    color: var(--color-raised-bg);
    background: var(--color-accent);
  }

  .base-button.danger {
    color: var(--color-danger-text);
  }

  .base-button.secondary:hover:not(:disabled) {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }

  .base-button.danger:hover:not(:disabled) {
    border-color: var(--color-danger-text);
  }

  .base-button.active {
    border-color: var(--color-accent);
    background: var(--color-active-highlight);
  }

  .base-button:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
