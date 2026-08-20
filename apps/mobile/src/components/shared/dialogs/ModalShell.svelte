<script lang="ts">
  import SquareButton from "../buttons/SquareButton.svelte";
  import type { Snippet } from "svelte";
  import { modalFocus } from "$lib/app/keyboard-navigation.ts";

  let {
    children,
    title,
    titleId,
    initialFocus,
    wide = false,
    onclose,
  }: {
    children: Snippet;
    title: string;
    titleId: string;
    initialFocus: string;
    wide?: boolean;
    onclose: () => void;
  } = $props();
</script>

<button
  class="modal-scrim"
  type="button"
  aria-label="Close"
  onclick={onclose}
></button>
<div
  class="modal-shell"
  class:wide
  role="dialog"
  aria-modal="true"
  aria-labelledby={titleId}
  use:modalFocus={{ onClose: onclose, initialFocus }}
>
  <header>
    <h2 id={titleId}>{title}</h2>
    <SquareButton
      label="Close"
      onclick={onclose}
    >
      ×
    </SquareButton>
  </header>
  {@render children()}
</div>

<style>
  .modal-scrim {
    position: fixed;
    z-index: 90;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--color-overlay-scrim);
  }

  .modal-shell {
    position: fixed;
    z-index: 91;
    right: 1rem;
    bottom: max(1rem, env(safe-area-inset-bottom));
    left: 1rem;
    display: grid;
    gap: 0.75rem;
    padding: 1rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    box-shadow: 0 1rem 3rem var(--color-box-shadow);
  }

  .modal-shell header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .modal-shell h2 {
    margin: 0;
    font: 600 1.15rem var(--font-interface-family);
  }

  @media (min-width: 45rem) {
    .modal-shell {
      top: 50%;
      right: auto;
      bottom: auto;
      left: 50%;
      width: min(28rem, calc(100vw - 2rem));
      transform: translate(-50%, -50%);
    }

    .modal-shell.wide {
      width: min(70rem, calc(100vw - 2rem));
    }
  }
</style>
