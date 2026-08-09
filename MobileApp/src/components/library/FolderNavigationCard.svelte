<script lang="ts">
  let {
    name,
    count,
    external = false,
    active = false,
    disabled = false,
    reserveEnd = false,
    onclick,
    onpointerenter,
    onpointerleave,
    onfocus,
    onpointerdown,
  }: {
    name: string;
    count?: number | string | null;
    external?: boolean;
    active?: boolean;
    disabled?: boolean;
    reserveEnd?: boolean;
    onclick: (event: MouseEvent) => void | Promise<void>;
    onpointerenter?: (event: PointerEvent) => void;
    onpointerleave?: (event: PointerEvent) => void;
    onfocus?: (event: FocusEvent) => void;
    onpointerdown?: (event: PointerEvent) => void;
  } = $props();
</script>

<button type="button" class:external class:active class:reserve-end={reserveEnd} disabled={disabled || active} aria-current={active ? "page" : undefined} title={name} {onclick} {onpointerenter} {onpointerleave} {onfocus} {onpointerdown}>
  {#if external}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6m0-6-9 9m7 0v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h6"></path></svg>
  {:else}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
  {/if}
  <span>{name}{#if count}<small>{count}</small>{/if}</span>
  {#if active}
    <i aria-hidden="true">✓</i>
  {:else if external}
    <i aria-hidden="true">↗</i>
  {/if}
</button>

<style>
  button { display: grid; width: 100%; min-width: 0; min-height: 2.6rem; grid-template-columns: 1.35rem minmax(0, 1fr) auto; gap: .65rem; align-items: center; padding: .72rem .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: inherit; background: var(--color-white-surface); font: inherit; text-align: left; cursor: pointer; }
  button:hover { background: var(--color-hover-bg); }
  button:active { transform: translateY(1px); }
  button:disabled { cursor: default; }
  button.active { border-color: var(--color-accent); background: var(--color-active-highlight); opacity: 1; }
  button.reserve-end { padding-right: 2.7rem; }
  svg { width: 1.35rem; height: 1.35rem; fill: none; stroke: var(--color-accent); stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.7; }
  button.external svg { stroke: var(--color-subtle-text); }
  span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  small { margin-left: .45rem; color: var(--color-subtle-text); font-size: .65rem; font-weight: 650; }
  i { color: var(--color-subtle-text); font-style: normal; }
</style>
