<script lang="ts">
  import { NAV_ITEMS, type Destination, type NavigationItem } from "../../lib/navigation";
  import { arrowKeyNavigation } from "../../lib/keyboard-navigation";
  import { ACCOUNT_CONTRACT, SETTINGS_CONTRACT } from "../../lib/types";

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
</script>

{#snippet icon(item: NavigationItem)}
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.path}></path></svg>
{/snippet}

<aside class="primary-nav" aria-label="Primary navigation" use:arrowKeyNavigation={{ orientation: "vertical" }}>
  <div class="nav-items">
    <button class="library-selector" type="button" aria-label={`Switch library, currently ${libraryName}`} title={libraryName} onclick={onlibraries}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5.5h5v14H4zM10.5 5.5h5v14h-5zM17 7l3-.8 3.2 12.9-3 .8z"></path></svg>
      <span>{libraryName}</span>
    </button>
    {#each NAV_ITEMS as item}
      <button title={item.label} class:active={active === item.id} type="button" aria-current={active === item.id ? "page" : undefined} onclick={() => onnavigate(item.id)}>
        {@render icon(item)}
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
  <div class="sidebar-actions">
    <button class:active={active === "account"} class="sidebar-action" type="button" aria-current={active === "account" ? "page" : undefined} title={ACCOUNT_CONTRACT.navigation_label} onclick={onaccount}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 12.5a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 21a7.5 7.5 0 0 1 15 0"></path></svg>
      <span>{ACCOUNT_CONTRACT.navigation_label}</span>
    </button>
    <button class:active={active === "settings"} class="sidebar-action" type="button" aria-current={active === "settings" ? "page" : undefined} title={SETTINGS_CONTRACT.navigation_label} onclick={onsettings}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6l-.05.08h-4l-.05-.08a1.7 1.7 0 0 0-1-.6 1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1l-.08-.05v-4L4 9.9a1.7 1.7 0 0 0 .6-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6l.05-.08h4l.05.08a1.7 1.7 0 0 0 1 .6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.12.4.33.75.6 1l.08.05v4L20 14.1c-.27.25-.48.6-.6.9z"></path></svg>
      <span>{SETTINGS_CONTRACT.navigation_label}</span>
    </button>
  </div>
</aside>

<nav class="bottom-nav" aria-label="Primary navigation" use:arrowKeyNavigation={{ orientation: "horizontal" }}>
  {#each NAV_ITEMS as item}
    <button class:active={active === item.id} type="button" aria-current={active === item.id ? "page" : undefined} onclick={() => onnavigate(item.id)}>
      {@render icon(item)}
      <span>{item.label}</span>
    </button>
  {/each}
</nav>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .primary-nav { display: none; }
  .bottom-nav { position: fixed; z-index: 30; right: 0; bottom: 0; left: 0; display: grid; grid-template-columns: repeat(5, 1fr); padding: .42rem max(.45rem, env(safe-area-inset-right)) calc(.42rem + env(safe-area-inset-bottom)) max(.45rem, env(safe-area-inset-left)); border-top: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); background: color-mix(in srgb, var(--color-surface-bg) 97%, transparent); backdrop-filter: blur(18px); }
  .bottom-nav button, .nav-items button { display: grid; place-items: center; gap: .18rem; min-width: 0; min-height: 3.35rem; padding: .35rem; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; font-size: .62rem; font-weight: 720; }
  .bottom-nav button.active, .nav-items button.active { color: var(--color-accent); background: var(--color-active-highlight); }
  .bottom-nav svg, .nav-items svg { width: 1.25rem; height: 1.25rem; }
  .nav-items .library-selector { margin-bottom: .35rem; padding-bottom: .7rem; border-bottom: 1px solid color-mix(in srgb, var(--color-border-color) 65%, transparent); border-radius: 0; color: var(--color-accent); cursor: pointer; }
  .nav-items .library-selector:hover { background: var(--color-hover-bg); }
  .nav-items .library-selector span { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  @media (min-width: 45rem) {
    .primary-nav { position: sticky; z-index: 35; top: 0; display: grid; grid-template-rows: minmax(0, 1fr) auto; justify-items: center; width: var(--layout-sidebar-width); height: 100dvh; padding: env(safe-area-inset-top) 8px env(safe-area-inset-bottom); border-right: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); background: var(--color-app-bg); }
    .nav-items { display: grid; align-content: start; gap: 0; width: 100%; }
    .nav-items button { width: 100%; height: 64px; min-height: 64px; }
    .nav-items .library-selector { height: 72px; min-height: 72px; margin: 0; padding-bottom: 0; }
    .sidebar-actions { display: grid; gap: 0; width: 100%; }
    .sidebar-action { display: grid; justify-items: center; gap: .2rem; width: 100%; height: 64px; min-height: 64px; padding: .6rem .25rem; border: 0; border-radius: var(--radius-md); color: var(--color-subtle-text); background: transparent; font-size: .58rem; font-weight: 720; cursor: pointer; }
    .sidebar-action:hover, .sidebar-action.active { color: var(--color-accent); background: var(--color-active-highlight); }
    .sidebar-action svg { width: 1.2rem; }
    .bottom-nav { display: none; }
  }

</style>
