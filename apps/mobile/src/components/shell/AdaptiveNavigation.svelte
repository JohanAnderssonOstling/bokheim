<script lang="ts">
  import { APP_NAV_ITEMS, NAV_ITEMS, type Destination, type NavigationItem } from "../../lib/navigation";
  import { arrowKeyNavigation } from "../../lib/keyboard-navigation";
  import { ACCOUNT_CONTRACT, BROWSING_LAYOUT_CONTRACT, SETTINGS_CONTRACT } from "../../lib/types";
  import NavigationIcon from "../shared/NavigationIcon.svelte";

  let {
    active,
    libraryName,
    onnavigate,
    onlibraries,
    onaccount,
    onsettings,
  }: {
    active: Destination;
    libraryName: string;
    onnavigate: (destination: Destination) => void;
    onlibraries: () => void;
    onaccount: () => void;
    onsettings: () => void;
  } = $props();

  const navigationItems = NAV_ITEMS;
  const bottomItems = navigationItems.filter((item) => item.id === "home" || item.id === "folders" || item.id === "subjects");
  // Store is app-scoped, so it comes from the other contract list rather than
  // from NAV_ITEMS; Account and Settings have their own props below.
  const storeItem = APP_NAV_ITEMS.filter((item) => item.id === "store");
  const accountItem = APP_NAV_ITEMS.find((item) => item.id === "account")!;
  const settingsItem = APP_NAV_ITEMS.find((item) => item.id === "settings")!;
  const sidebarItems = [...navigationItems, ...storeItem];
  const moreItems = [...navigationItems.filter((item) => item.id === "authors" || item.id === "trash"), ...storeItem];
  let moreOpen = $state(false);
  let moreActive = $derived(moreItems.some((item) => item.id === active) || active === "account" || active === "settings");
  const navigationIconSize = `${(BROWSING_LAYOUT_CONTRACT.navigation_icon_size_px ?? 24) / 16}rem`;

  function navigate(destination: Destination) {
    moreOpen = false;
    onnavigate(destination);
  }

  function openAccount() {
    moreOpen = false;
    onaccount();
  }

  function openSettings() {
    moreOpen = false;
    onsettings();
  }

  function openLibraries() {
    moreOpen = false;
    onlibraries();
  }
</script>

{#snippet icon(item: NavigationItem)}
  <NavigationIcon icon={item.icon} size={navigationIconSize} />
{/snippet}

<aside class="primary-nav" aria-label="Primary navigation" use:arrowKeyNavigation={{ orientation: "vertical" }}>
  <div class="nav-items">
    <button class="library-selector" type="button" aria-label={`Switch library, currently ${libraryName}`} title={libraryName} onclick={onlibraries}>
      <NavigationIcon icon="book_open" size={navigationIconSize} />
      <span>{libraryName}</span>
    </button>
    {#each sidebarItems as item}
      <button title={item.label} class:active={active === item.id} type="button" aria-current={active === item.id ? "page" : undefined} onclick={() => onnavigate(item.id)}>
        {@render icon(item)}
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
  <div class="sidebar-actions">
    <button class:active={active === "account"} class="sidebar-action" type="button" aria-current={active === "account" ? "page" : undefined} title={ACCOUNT_CONTRACT.navigation_label} onclick={onaccount}>
      <NavigationIcon icon={accountItem.icon} size={navigationIconSize} />
      <span>{ACCOUNT_CONTRACT.navigation_label}</span>
    </button>
    <button class:active={active === "settings"} class="sidebar-action" type="button" aria-current={active === "settings" ? "page" : undefined} title={SETTINGS_CONTRACT.navigation_label} onclick={onsettings}>
      <NavigationIcon icon={settingsItem.icon} size={navigationIconSize} />
      <span>{SETTINGS_CONTRACT.navigation_label}</span>
    </button>
  </div>
</aside>

{#if moreOpen}
  <button class="more-scrim" type="button" aria-label="Close more navigation" onclick={() => (moreOpen = false)}></button>
  <div class="more-menu" role="menu" aria-label="More navigation">
    <button type="button" role="menuitem" onclick={openLibraries}>
      <NavigationIcon icon="book_open" size={navigationIconSize} />
      <span>Switch library</span>
    </button>
    {#each moreItems as item}
      <button class:active={active === item.id} type="button" role="menuitem" aria-current={active === item.id ? "page" : undefined} onclick={() => navigate(item.id)}>
        {@render icon(item)}
        <span>{item.label}</span>
      </button>
    {/each}
    <button class:active={active === "account"} type="button" role="menuitem" aria-current={active === "account" ? "page" : undefined} onclick={openAccount}>
      <NavigationIcon icon={accountItem.icon} size={navigationIconSize} />
      <span>{ACCOUNT_CONTRACT.navigation_label}</span>
    </button>
    <button class:active={active === "settings"} type="button" role="menuitem" aria-current={active === "settings" ? "page" : undefined} onclick={openSettings}>
      <NavigationIcon icon={settingsItem.icon} size={navigationIconSize} />
      <span>{SETTINGS_CONTRACT.navigation_label}</span>
    </button>
  </div>
{/if}

<nav class="bottom-nav" aria-label="Primary navigation" use:arrowKeyNavigation={{ orientation: "horizontal" }}>
  {#each bottomItems as item}
    <button class:active={active === item.id} type="button" aria-current={active === item.id ? "page" : undefined} onclick={() => navigate(item.id)}>
      {@render icon(item)}
      <span>{item.label}</span>
    </button>
  {/each}
  <button class:active={moreActive || moreOpen} type="button" aria-expanded={moreOpen} aria-haspopup="menu" onclick={() => (moreOpen = !moreOpen)}>
    <NavigationIcon icon="dots_three" size={navigationIconSize} />
    <span>More</span>
  </button>
</nav>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .primary-nav { display: none; }
  .bottom-nav { position: fixed; z-index: 30; right: 0; bottom: 0; left: 0; display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); padding: 0; border-top: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent); background: color-mix(in srgb, var(--color-surface-bg) 97%, transparent); backdrop-filter: blur(18px); }
  .bottom-nav button, .nav-items button { display: grid; place-items: center; gap: var(--navigation-icon-label-gap); min-width: 0; min-height: var(--navigation-touch-item-min-height); padding: .35rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; font-size: var(--navigation-label-size); font-weight: var(--navigation-label-weight); }
  .bottom-nav button { padding-bottom: calc(.35rem + env(safe-area-inset-bottom)); }
  .bottom-nav button:first-child { padding-left: max(.35rem, env(safe-area-inset-left)); }
  .bottom-nav button:last-child { padding-right: max(.35rem, env(safe-area-inset-right)); }
  .bottom-nav button.active, .nav-items button.active { color: var(--color-accent); background: var(--color-active-highlight); }
  .more-scrim { position: fixed; z-index: 29; inset: 0; width: 100%; height: 100%; padding: 0; border: 0; background: color-mix(in srgb, var(--color-ink-text) 12%, transparent); }
  .more-menu { position: fixed; z-index: 32; right: max(.65rem, env(safe-area-inset-right)); bottom: calc(4.75rem + env(safe-area-inset-bottom)); display: grid; width: min(15rem, calc(100vw - 1.3rem)); padding: .4rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); box-shadow: 0 .8rem 2rem color-mix(in srgb, var(--color-box-shadow) 35%, transparent); }
  .more-menu button { display: grid; grid-template-columns: 1.5rem minmax(0, 1fr); align-items: center; gap: .7rem; min-height: 3rem; padding: .5rem .7rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; text-align: left; }
  .more-menu button.active { color: var(--color-accent); background: var(--color-active-highlight); }
  .nav-items .library-selector { margin-bottom: .35rem; padding-bottom: .7rem; border-radius: 0; color: var(--color-accent); cursor: pointer; }
  .nav-items .library-selector:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .nav-items .library-selector span { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  @media (min-width: 45rem) {
    .primary-nav { position: sticky; z-index: 35; top: 0; display: grid; grid-template-rows: minmax(0, 1fr) auto; justify-items: center; width: var(--layout-sidebar-width); height: 100dvh; padding: env(safe-area-inset-top) 0 env(safe-area-inset-bottom); border-right: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent); background: var(--color-surface-bg); }
    .nav-items { display: grid; min-height: 0; align-content: start; gap: 0; width: 100%; overflow-y: auto; overscroll-behavior-y: contain; scrollbar-width: none; }
    .nav-items::-webkit-scrollbar { display: none; }
    .nav-items button { width: 100%; min-height: var(--navigation-item-min-height); }
    .nav-items .library-selector { min-height: 72px; margin: 0; padding-bottom: 0; }
    .sidebar-actions { display: grid; gap: 0; width: 100%; }
    .sidebar-action { display: grid; justify-items: center; gap: var(--navigation-icon-label-gap); width: 100%; min-height: var(--navigation-item-min-height); padding: .6rem .25rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; font-size: var(--navigation-label-size); font-weight: var(--navigation-label-weight); cursor: pointer; }
    .sidebar-action:hover, .sidebar-action.active { color: var(--color-accent); background: var(--color-active-highlight); }
    .bottom-nav { display: none; }
    .more-scrim, .more-menu { display: none; }
  }

</style>
