<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * A button that reads as a link: no border, no fill, accent label. Used for
   * "See all" style affordances beside a heading, and for inline actions in
   * running text where a bordered control would break the line.
   *
   * It keeps a 44px hit target on touch even though it has no visible box.
   */
  let {
    children,
    tone = "accent",
    disabled = false,
    onclick,
  }: {
    children: Snippet;
    tone?: "accent" | "inherit";
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
  } = $props();
</script>

<button
  class={["text-button", tone === "inherit" && "inherit-tone"]}
  type="button"
  {disabled}
  {onclick}
>
  {@render children()}
</button>

<style>
  .text-button {
    display: inline-flex;
    align-items: center;
    /* Grid rows would otherwise stretch a borderless button to full width and
       give it a hit area far wider than the words it underlines. */
    justify-self: start;
    gap: 5px;
    min-height: 2.25rem;
    padding: 5px 6px;
    border: 0;
    border-radius: 0;
    color: var(--color-accent);
    background: transparent;
    font-family: inherit;
    font-size: 0.78rem;
    font-weight: 750;
    line-height: 1.2;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }

  .text-button.inherit-tone {
    color: inherit;
    font: inherit;
  }

  .text-button:hover:not(:disabled) {
    text-decoration: underline;
  }

  .text-button:disabled {
    opacity: 0.55;
    cursor: default;
  }
</style>
