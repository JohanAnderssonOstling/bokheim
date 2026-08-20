<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A destination in the app's primary navigation: an icon above its label, in
   * a square box. Square rather than full-bleed so the active state reads as a
   * tile centred in the bar, the same shape in the desktop sidebar as in the
   * mobile tab bar.
   *
   * `size` is one length because the box is square; pass the layout contract's
   * navigation measurement for the surface (`--navigation-item-min-height` on
   * the sidebar, `--navigation-touch-item-min-height` on touch) so the tile
   * keeps following the UI font preference.
   *
   * `accent` marks the odd one out — the library switcher, which sits in the
   * nav but opens a dialog instead of navigating.
   */
  let {
    icon,
    label,
    size = "var(--navigation-item-min-height)",
    active = false,
    accent = false,
    ariaLabel,
    ariaCurrent,
    ariaExpanded,
    ariaHasPopup,
    href,
    onclick,
  }: {
    icon: Snippet;
    label: string;
    size?: string;
    active?: boolean;
    accent?: boolean;
    ariaLabel?: string;
    ariaCurrent?: "page";
    ariaExpanded?: boolean;
    ariaHasPopup?: "menu" | "dialog";
    href?: string;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

{#if href}
  <a
    class={["nav-item-button", active && "active", accent && "accent"]}
    style={`--nav-item-size: ${size};`}
    {href}
    title={label}
    aria-label={ariaLabel}
    aria-current={ariaCurrent}>{@render icon()}<span>{label}</span></a
  >
{:else}
  <button
    class={["nav-item-button", active && "active", accent && "accent"]}
    style={`--nav-item-size: ${size};`}
    type="button"
    title={label}
    aria-label={ariaLabel}
    aria-current={ariaCurrent}
    aria-expanded={ariaExpanded}
    aria-haspopup={ariaHasPopup}
    {onclick}
  >
    {@render icon()}
    <span>{label}</span>
  </button>
{/if}

<style>
  .nav-item-button {
    display: grid;
    place-items: center;
    align-content: center;
    gap: var(--navigation-icon-label-gap);
    width: var(--nav-item-size);
    height: var(--nav-item-size);
    border: 0;
    border-radius: 0;
    color: var(--color-ink-text);
    background: transparent;
    font-family: inherit;
    font-size: var(--navigation-label-size);
    font-weight: var(--navigation-label-weight);
    line-height: 1.15;
    cursor: pointer;
    text-decoration: none;
    -webkit-tap-highlight-color: transparent;
  }

  .nav-item-button span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .nav-item-button.accent {
    color: var(--color-accent);
  }

  .nav-item-button:hover {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }

  .nav-item-button.active {
    color: var(--color-accent);
    background: var(--color-active-highlight);
  }
</style>
