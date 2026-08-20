<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLInputAttributes } from "svelte/elements";
  import magnifyingGlass from "@icons/phosphor/magnifying-glass.svg";
  import FileIcon from "./FileIcon.svelte";

  const noop = () => {};

  let {
    value = $bindable(),
    placeholder,
    ariaLabel = placeholder,
    id,
    autocomplete = "off",
    showIcon = true,
    class: className = "",
    oninput = noop,
    onsubmit = noop,
    onkeydown,
    children,
  }: {
    value: string;
    placeholder: string;
    ariaLabel?: string;
    id?: string;
    autocomplete?: HTMLInputAttributes["autocomplete"];
    showIcon?: boolean;
    class?: string;
    oninput?: (value: string) => void;
    onsubmit?: (event: SubmitEvent) => void | Promise<void>;
    onkeydown?: (event: KeyboardEvent) => void;
    children?: Snippet;
  } = $props();

  let input: HTMLInputElement | undefined = $state();

  /** Lets an owner move focus here without reaching through the DOM for it. */
  export function focus() {
    input?.focus();
    input?.select();
  }

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    void onsubmit(event);
  }

  function handleInput(event: Event) {
    value = (event.currentTarget as HTMLInputElement).value;
    oninput(value);
  }
</script>

<form
  class={`search-field ${className}`.trim()}
  class:without-icon={!showIcon}
  onsubmit={handleSubmit}
>
  {#if showIcon}<FileIcon source={magnifyingGlass} />{/if}
  <input
    bind:this={input}
    class="search-input"
    {id}
    type="search"
    enterkeyhint="search"
    {value}
    {placeholder}
    aria-label={ariaLabel}
    {autocomplete}
    oninput={handleInput}
    {onkeydown}
  />
  {@render children?.()}
</form>

<style>
  .search-field {
    min-width: 0;
  }

  .search-input {
    box-sizing: border-box;
    min-width: 0;
    font: inherit;
  }
</style>
