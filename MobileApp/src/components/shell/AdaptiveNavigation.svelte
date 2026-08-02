<script lang="ts">
  import { NAV_ITEMS, type Destination, type NavigationItem } from "../../lib/navigation";

  let {
    active,
    onnavigate,
    onsettings,
  }: {
    active: Destination;
    onnavigate: (destination: Destination) => void;
    onsettings: () => void;
  } = $props();
</script>

{#snippet icon(item: NavigationItem)}
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d={item.path}></path></svg>
{/snippet}

<aside class="primary-nav" aria-label="Primary navigation">
  <div class="nav-items">
    {#each NAV_ITEMS as item}
      <button class:active={active === item.id} type="button" aria-current={active === item.id ? "page" : undefined} onclick={() => onnavigate(item.id)}>
        {@render icon(item)}
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
  <button class="sidebar-action" type="button" onclick={onsettings}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6l-.05.08h-4l-.05-.08a1.7 1.7 0 0 0-1-.6 1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1l-.08-.05v-4L4 9.9a1.7 1.7 0 0 0 .6-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6l.05-.08h4l.05.08a1.7 1.7 0 0 0 1 .6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.12.4.33.75.6 1l.08.05v4L20 14.1c-.27.25-.48.6-.6.9z"></path></svg>
    <span>Settings</span>
  </button>
</aside>

<nav class="bottom-nav" aria-label="Primary navigation">
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
  .bottom-nav button, .nav-items button { display: grid; place-items: center; gap: .18rem; min-height: 3.35rem; padding: .35rem; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; font-size: .62rem; font-weight: 720; }
  .bottom-nav button.active, .nav-items button.active { color: var(--color-accent); background: var(--color-active-highlight); }
  .bottom-nav svg, .nav-items svg { width: 1.25rem; height: 1.25rem; }

  @media (min-width: 45rem) {
    .primary-nav { position: sticky; z-index: 35; top: 0; display: grid; grid-template-rows: minmax(0, 1fr) auto; justify-items: center; height: 100dvh; padding: calc(1rem + env(safe-area-inset-top)) .65rem calc(1rem + env(safe-area-inset-bottom)); border-right: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); background: var(--color-app-bg); }
    .nav-items { display: grid; align-content: start; gap: .5rem; width: 100%; }
    .nav-items button { width: 100%; }
    .sidebar-action { display: grid; justify-items: center; gap: .2rem; width: 100%; padding: .6rem .25rem; border: 0; border-radius: var(--radius-md); color: var(--color-subtle-text); background: transparent; font-size: .58rem; font-weight: 720; cursor: pointer; }
    .sidebar-action:hover { color: var(--color-accent); background: var(--color-hover-bg); }
    .sidebar-action svg { width: 1.2rem; }
    .bottom-nav { display: none; }
  }

  @media (min-width: 70rem) {
    .primary-nav { justify-items: stretch; padding-inline: 1rem; }
    .nav-items button { grid-template-columns: 1.6rem 1fr; justify-items: start; gap: .75rem; min-height: 2.85rem; padding: .55rem .75rem; font-size: .78rem; }
    .sidebar-action { grid-template-columns: 1.5rem 1fr; justify-items: start; gap: .65rem; padding: .75rem; font-size: .72rem; }
  }
</style>
