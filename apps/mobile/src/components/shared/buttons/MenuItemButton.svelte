<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A row inside a popover menu: full width, left aligned, no border, hover
   * painted with `--color-hover-overlay` rather than a background swap so it
   * works over both raised and surface backgrounds.
   *
   * `icon` is a separate snippet instead of part of `children` so that every
   * menu in the app lines its labels up on the same column, whether or not a
   * given row has an icon.
   *
   * The row shape also suits plain lists that are not menus; pass `role={null}`
   * there so no menu semantics are announced.
   */
  let {
    children,
    icon,
    trailing,
    danger = false,
    active = false,
    disabled = false,
    role = "menuitem",
    ariaChecked,
    ariaCurrent,
    href,
    onclick,
  }: {
    children: Snippet;
    icon?: Snippet;
    trailing?: Snippet;
    danger?: boolean;
    active?: boolean;
    disabled?: boolean;
    role?: "menuitem" | "menuitemradio" | "menuitemcheckbox" | "option" | null;
    ariaChecked?: boolean;
    ariaCurrent?: "page" | "true";
    href?: string;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

{#if href}
  <a
    class={[
      "menu-item-button",
      icon && "with-icon",
      trailing && "with-trailing",
      danger && "danger",
      active && "active",
    ]}
    {href}
    {role}
    aria-checked={ariaChecked}
    aria-current={ariaCurrent}
    >{#if icon}<span class="icon">{@render icon()}</span>{/if}
    <span class="label">{@render children()}</span>
    {#if trailing}<span class="trailing">{@render trailing()}</span>{/if}</a
  >
{:else}
  <button
    class={[
      "menu-item-button",
      icon && "with-icon",
      trailing && "with-trailing",
      danger && "danger",
      active && "active",
    ]}
    type="button"
    {role}
    aria-checked={ariaChecked}
    aria-current={ariaCurrent}
    {disabled}
    {onclick}
  >
    {#if icon}<span class="icon">{@render icon()}</span>{/if}
    <span class="label">{@render children()}</span>
    {#if trailing}<span class="trailing">{@render trailing()}</span>{/if}
  </button>
{/if}

<style>
  .menu-item-button {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    width: 100%;
    min-height: 2.5rem;
    padding: 9px 11px;
    border: 0;
    border-radius: 0;
    color: var(--color-ink-text);
    background: transparent;
    font-family: inherit;
    font-size: 0.78rem;
    font-weight: 650;
    line-height: 1.2;
    text-align: left;
    cursor: pointer;
    text-decoration: none;
    -webkit-tap-highlight-color: transparent;
  }

  .menu-item-button.with-icon {
    grid-template-columns: 1.125rem minmax(0, 1fr);
  }

  .menu-item-button.with-trailing {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .menu-item-button.with-icon.with-trailing {
    grid-template-columns: 1.125rem minmax(0, 1fr) auto;
  }

  .icon {
    display: grid;
    place-items: center;
  }

  .icon :global(svg) {
    width: 1.0625rem;
    height: 1.0625rem;
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .menu-item-button:hover:not(:disabled) {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }

  .menu-item-button.active {
    color: var(--color-accent);
    background: var(--color-active-highlight);
  }

  .menu-item-button.danger {
    color: var(--color-danger-text);
  }

  .menu-item-button:disabled {
    color: var(--color-border);
    cursor: default;
  }
</style>
