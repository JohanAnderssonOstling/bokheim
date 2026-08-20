<script lang="ts">
  import { type Destination, type NavigationItem } from "../../../lib/app/navigation";
  import { arrowKeyNavigation } from "../../../lib/app/keyboard-navigation";
  import { BROWSING_LAYOUT_CONTRACT } from "../../../lib/shared/types";
  import NavigationIcon from "../../shared/NavigationIcon.svelte";
  import MenuItemButton from "../../shared/buttons/MenuItemButton.svelte";
  import NavItemButton from "../../shared/buttons/NavItemButton.svelte";

  let {
    active,
    bottomItems,
    moreItems,
    accountItem,
    settingsItem,
    hrefFor,
    onlibraries,
  }: {
    active: Destination;
    bottomItems: readonly NavigationItem[];
    moreItems: readonly NavigationItem[];
    accountItem: NavigationItem;
    settingsItem: NavigationItem;
    hrefFor: (destination: Destination) => string;
    onlibraries: () => void;
  } = $props();

  const navigationIconSize = `${(BROWSING_LAYOUT_CONTRACT.navigation_icon_size_px ?? 24) / 16}rem`;
  let moreOpen = $state(false);
  let moreActive = $derived(
    moreItems.some((item) => item.id === active) ||
      active === "account" ||
      active === "settings",
  );

  function closeMenu() {
    moreOpen = false;
  }

  function openLibraries() {
    moreOpen = false;
    onlibraries();
  }
</script>

{#snippet navigationIcon(item: NavigationItem)}
  <NavigationIcon
    icon={item.icon}
    size={navigationIconSize}
  />
{/snippet}

{#if moreOpen}
  <button
    class="more-scrim"
    type="button"
    aria-label="Close more navigation"
    onclick={() => (moreOpen = false)}
  ></button>
  <div
    class="more-menu"
    role="menu"
    aria-label="More navigation"
  >
    <MenuItemButton onclick={openLibraries}
      >{#snippet icon()}<NavigationIcon
          icon="book_open"
          size={navigationIconSize}
        />{/snippet}Switch library</MenuItemButton
    >
    {#each moreItems as item}
      <MenuItemButton
        active={active === item.id}
        ariaCurrent={active === item.id ? "page" : undefined}
        href={hrefFor(item.id)}
        onclick={closeMenu}
        >{#snippet icon()}{@render navigationIcon(
            item,
          )}{/snippet}{item.label}</MenuItemButton
      >
    {/each}
    <MenuItemButton
      active={active === "account"}
      ariaCurrent={active === "account" ? "page" : undefined}
      href={hrefFor("account")}
      onclick={closeMenu}
      >{#snippet icon()}<NavigationIcon
          icon={accountItem.icon}
          size={navigationIconSize}
        />{/snippet}{accountItem.label}</MenuItemButton
    >
    <MenuItemButton
      active={active === "settings"}
      ariaCurrent={active === "settings" ? "page" : undefined}
      href={hrefFor("settings")}
      onclick={closeMenu}
      >{#snippet icon()}<NavigationIcon
          icon={settingsItem.icon}
          size={navigationIconSize}
        />{/snippet}{settingsItem.label}</MenuItemButton
    >
  </div>
{/if}

<nav
  class="bottom-nav"
  aria-label="Primary navigation"
  use:arrowKeyNavigation={{ orientation: "horizontal" }}
>
  {#each bottomItems as item}
    <NavItemButton
      label={item.label}
      size="var(--navigation-touch-item-min-height)"
      active={active === item.id}
      ariaCurrent={active === item.id ? "page" : undefined}
      href={hrefFor(item.id)}
      >{#snippet icon()}{@render navigationIcon(item)}{/snippet}</NavItemButton
    >
  {/each}
  <NavItemButton
    label="More"
    size="var(--navigation-touch-item-min-height)"
    active={moreActive || moreOpen}
    ariaExpanded={moreOpen}
    ariaHasPopup="menu"
    onclick={() => (moreOpen = !moreOpen)}
    >{#snippet icon()}<NavigationIcon
        icon="dots_three"
        size={navigationIconSize}
      />{/snippet}</NavItemButton
  >
</nav>

<style>
  button {
    font: inherit;
    -webkit-tap-highlight-color: transparent;
  }
  .bottom-nav {
    position: fixed;
    z-index: 30;
    right: 0;
    bottom: 0;
    left: 0;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    justify-items: center;
    padding: 0 env(safe-area-inset-right) env(safe-area-inset-bottom)
      env(safe-area-inset-left);
    border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
    background: color-mix(in srgb, var(--color-surface-bg) 97%, transparent);
    backdrop-filter: blur(18px);
  }
  .more-scrim {
    position: fixed;
    z-index: 29;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: color-mix(in srgb, var(--color-ink-text) 12%, transparent);
  }
  .more-menu {
    position: fixed;
    z-index: 32;
    right: max(0.65rem, env(safe-area-inset-right));
    bottom: calc(4.75rem + env(safe-area-inset-bottom));
    display: grid;
    width: min(15rem, calc(100vw - 1.3rem));
    padding: 0.4rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
    box-shadow: 0 0.8rem 2rem color-mix(in srgb, var(--color-box-shadow) 35%, transparent);
  }
  @media (min-width: 45rem) {
    .bottom-nav,
    .more-scrim,
    .more-menu {
      display: none;
    }
  }
</style>
