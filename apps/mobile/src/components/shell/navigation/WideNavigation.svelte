<script lang="ts">
  import { type Destination, type NavigationItem } from "../../../lib/app/navigation";
  import { arrowKeyNavigation } from "../../../lib/app/keyboard-navigation";
  import { BROWSING_LAYOUT_CONTRACT } from "../../../lib/shared/types";
  import NavigationIcon from "../../shared/NavigationIcon.svelte";
  import NavItemButton from "../../shared/buttons/NavItemButton.svelte";

  let {
    active,
    libraryName,
    sidebarItems,
    accountItem,
    settingsItem,
    hrefFor,
    onlibraries,
  }: {
    active: Destination;
    libraryName: string;
    sidebarItems: readonly NavigationItem[];
    accountItem: NavigationItem;
    settingsItem: NavigationItem;
    hrefFor: (destination: Destination) => string;
    onlibraries: () => void;
  } = $props();

  const navigationIconSize = `${(BROWSING_LAYOUT_CONTRACT.navigation_icon_size_px ?? 24) / 16}rem`;
</script>

{#snippet navigationIcon(item: NavigationItem)}
  <NavigationIcon
    icon={item.icon}
    size={navigationIconSize}
  />
{/snippet}

<aside
  class="primary-nav"
  aria-label="Primary navigation"
  use:arrowKeyNavigation={{ orientation: "vertical" }}
>
  <div class="nav-items">
    <NavItemButton
      label={libraryName}
      accent
      ariaLabel={`Switch library, currently ${libraryName}`}
      ariaHasPopup="dialog"
      onclick={onlibraries}
    >
      {#snippet icon()}<NavigationIcon
          icon="book_open"
          size={navigationIconSize}
        />{/snippet}
    </NavItemButton>
    {#each sidebarItems as item}
      <NavItemButton
        label={item.label}
        active={active === item.id}
        ariaCurrent={active === item.id ? "page" : undefined}
        href={hrefFor(item.id)}
      >
        {#snippet icon()}{@render navigationIcon(item)}{/snippet}
      </NavItemButton>
    {/each}
  </div>
  <div class="sidebar-actions">
    <NavItemButton
      label={accountItem.label}
      active={active === "account"}
      ariaCurrent={active === "account" ? "page" : undefined}
      href={hrefFor("account")}
    >
      {#snippet icon()}<NavigationIcon
          icon={accountItem.icon}
          size={navigationIconSize}
        />{/snippet}
    </NavItemButton>
    <NavItemButton
      label={settingsItem.label}
      active={active === "settings"}
      ariaCurrent={active === "settings" ? "page" : undefined}
      href={hrefFor("settings")}
    >
      {#snippet icon()}<NavigationIcon
          icon={settingsItem.icon}
          size={navigationIconSize}
        />{/snippet}
    </NavItemButton>
  </div>
</aside>

<style>
  .primary-nav {
    display: none;
  }
  @media (min-width: 45rem) {
    .primary-nav {
      position: sticky;
      z-index: 35;
      top: 0;
      display: grid;
      grid-template-rows: minmax(0, 1fr) auto;
      justify-items: center;
      height: 100dvh;
      padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom);
      border-right: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent);
      background: var(--color-surface-bg);
    }
    .nav-items {
      display: grid;
      min-height: 0;
      align-content: start;
      justify-items: center;
      gap: 0;
      width: 100%;
      overflow-y: auto;
      overscroll-behavior-y: contain;
      scrollbar-width: none;
    }
    .nav-items::-webkit-scrollbar {
      display: none;
    }
    .sidebar-actions {
      display: grid;
      justify-items: center;
      gap: 0;
      width: 100%;
    }
    .nav-items > :global(.nav-item-button:first-child) {
      margin-bottom: 0.35rem;
    }
  }
</style>
