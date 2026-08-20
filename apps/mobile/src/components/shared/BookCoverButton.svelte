<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    ariaLabel,
    disabled = false,
    className = "",
    onclick,
    children,
  }: {
    ariaLabel: string;
    disabled?: boolean;
    className?: string;
    onclick: (event: MouseEvent) => void | Promise<void>;
    children: Snippet;
  } = $props();
</script>

<button
  class={`book-cover-button ${className}`.trim()}
  type="button"
  {disabled}
  aria-label={ariaLabel}
  {onclick}
>
  {@render children()}
</button>

<style>
  .book-cover-button {
    position: relative;
    display: grid;
    width: 100%;
    min-width: 0;
    padding: 0;
    border: 0;
    color: inherit;
    background: transparent;
    cursor: pointer;
    font: inherit;
    -webkit-tap-highlight-color: transparent;
  }

  .book-cover-button:active {
    transform: scale(0.985);
  }

  .book-cover-button:disabled {
    cursor: default;
    opacity: 0.58;
  }
</style>
