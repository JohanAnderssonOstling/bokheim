<script lang="ts">
  let {
    name,
    count,
    kind = "folder",
    external = false,
    active = false,
    disabled = false,
    reserveEnd = false,
    flat = false,
    onclick,
    onpointerenter,
    onpointerleave,
    onfocus,
    onpointerdown,
    onpointerup,
    onpointercancel,
    onpointermove,
    oncontextmenu,
    onkeydown,
  }: {
    name: string;
    count?: number | string | null;
    kind?: "folder" | "subject";
    external?: boolean;
    active?: boolean;
    disabled?: boolean;
    reserveEnd?: boolean;
    flat?: boolean;
    onclick: (event: MouseEvent) => void | Promise<void>;
    onpointerenter?: (event: PointerEvent) => void;
    onpointerleave?: (event: PointerEvent) => void;
    onfocus?: (event: FocusEvent) => void;
    onpointerdown?: (event: PointerEvent) => void;
    onpointerup?: (event: PointerEvent) => void;
    onpointercancel?: (event: PointerEvent) => void;
    onpointermove?: (event: PointerEvent) => void;
    oncontextmenu?: (event: MouseEvent) => void;
    onkeydown?: (event: KeyboardEvent) => void;
  } = $props();
</script>

<button type="button" class:external class:active class:flat class:reserve-end={reserveEnd} disabled={disabled || active} aria-current={active ? "page" : undefined} title={name} {onclick} {onpointerenter} {onpointerleave} {onfocus} {onpointerdown} {onpointerup} {onpointercancel} {onpointermove} {oncontextmenu} {onkeydown}>
  {#if external}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6m0-6-9 9m7 0v6a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h6"></path></svg>
  {:else if kind === "subject"}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.6 13.6 13.7 20.5a2 2 0 0 1-2.8 0L3.5 13.1a2 2 0 0 1 0-2.8V4h6.3a2 2 0 0 1 1.4.6l9.4 9a1.4 1.4 0 0 1 0 2zM7.2 7.2h.1"></path></svg>
  {:else}
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
  {/if}
  <span>{name}</span>
  {#if count !== null && count !== undefined}<small>{count}</small>{/if}
  {#if active}
    <i aria-hidden="true">✓</i>
  {:else if external}
    <i aria-hidden="true">↗</i>
  {/if}
</button>

<style>
  button { display: grid; width: 100%; min-width: 0; min-height: max(3.5rem, var(--hierarchy-touch-cell-min-height)); grid-template-columns: var(--hierarchy-icon-size) minmax(0, 1fr) auto; gap: .65rem; align-items: center; padding: .72rem .75rem; border: 1px solid var(--color-border); border-radius: 0; color: inherit; background: var(--color-raised-bg); font: inherit; text-align: left; cursor: pointer; }
  button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  button:active { transform: translateY(1px); }
  button:disabled { cursor: default; }
  button.active { border-color: var(--color-accent); background: var(--color-active-highlight); opacity: 1; }
  button.flat { border: 0; border-radius: 0; }
  button.reserve-end { padding-right: 3.5rem; }
  svg { width: var(--hierarchy-icon-size); height: var(--hierarchy-icon-size); fill: none; stroke: var(--color-accent); stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.7; }
  button.external svg { stroke: var(--color-subtle-text); }
  span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--hierarchy-label-size); font-weight: 500; }
  small { color: var(--color-subtle-text); font-size: .8125rem; font-weight: 650; text-align: right; }
  i { color: var(--color-subtle-text); font-style: normal; }
</style>
