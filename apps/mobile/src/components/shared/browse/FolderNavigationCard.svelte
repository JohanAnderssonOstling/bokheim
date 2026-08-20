<script lang="ts">
  import arrowSquareOut from "@icons/phosphor/arrow-square-out.svg";
  import folder from "@icons/phosphor/folder.svg";
  import tag from "@icons/phosphor/tag.svg";
  import FileIcon from "../FileIcon.svelte";
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

<button
  type="button"
  class:external
  class:active
  class:flat
  class:reserve-end={reserveEnd}
  disabled={disabled || active}
  aria-current={active ? "page" : undefined}
  title={name}
  {onclick}
  {onpointerenter}
  {onpointerleave}
  {onfocus}
  {onpointerdown}
  {onpointerup}
  {onpointercancel}
  {onpointermove}
  {oncontextmenu}
  {onkeydown}
>
  {#if external}<FileIcon source={arrowSquareOut} />
  {:else if kind === "subject"}<FileIcon source={tag} />
  {:else}<FileIcon source={folder} />{/if}
  <span>{name}</span>
  {#if count !== null && count !== undefined}<small>{count}</small>{/if}
  {#if active}
    <i aria-hidden="true">✓</i>
  {:else if external}
    <i aria-hidden="true">↗</i>
  {/if}
</button>

<style>
  button {
    display: grid;
    width: 100%;
    min-width: 0;
    min-height: max(3.5rem, var(--hierarchy-touch-cell-min-height));
    grid-template-columns: var(--hierarchy-icon-size) minmax(0, 1fr) auto;
    gap: 0.65rem;
    align-items: center;
    padding: 0.72rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button:hover {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }
  button:active {
    transform: translateY(1px);
  }
  button:disabled {
    cursor: default;
  }
  button.active {
    border-color: var(--color-accent);
    background: var(--color-active-highlight);
    opacity: 1;
  }
  button.flat {
    border: 0;
    border-radius: 0;
  }
  button.reserve-end {
    padding-right: 3.5rem;
  }
  :global(.file-icon) {
    color: var(--color-accent);
  }
  button.external :global(.file-icon) {
    color: var(--color-subtle-text);
  }
  span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--hierarchy-label-size);
    font-weight: 500;
  }
  small {
    color: var(--color-subtle-text);
    font-size: 1rem;
    font-weight: 650;
    text-align: right;
  }
  i {
    color: var(--color-subtle-text);
    font-style: normal;
  }

  @media (min-width: 45rem) {
    button {
      min-height: var(--hierarchy-cell-height);
      height: var(--hierarchy-cell-height);
      padding-top: 0.5rem;
      padding-bottom: 0.5rem;
    }
  }
</style>
