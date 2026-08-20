<script lang="ts">
  import type { Snippet } from "svelte";

  /**
   * Chooses which surface owns the screen.
   *
   * A reader route replaces the browser rather than rendering inside it, so
   * its page is rendered here at full size. Everything else renders in the
   * browser shell.
   */
  let {
    isWebApp,
    accountResolved,
    webAppReady,
    readerRoute,
    reader,
    browser,
  }: {
    isWebApp: boolean;
    accountResolved: boolean;
    webAppReady: boolean;
    readerRoute: boolean;
    reader: Snippet;
    browser: Snippet;
  } = $props();
</script>

{#if isWebApp && (!accountResolved || !webAppReady)}
  <main
    class="auth-loading"
    aria-live="polite"
  >
    <span class="loader"></span>
    <strong
      >{accountResolved ? "Opening your library" : "Starting the local reader"}</strong
    >
  </main>
{:else if readerRoute}
  {@render reader()}
{:else}
  {@render browser()}
{/if}

<style>
  .auth-loading {
    display: grid;
    min-height: 100dvh;
    place-content: center;
    justify-items: center;
    gap: 0.8rem;
    color: var(--color-subtle-text);
    background: var(--color-surface-bg);
    font-size: 0.8rem;
  }
  .loader {
    width: 1.4rem;
    height: 1.4rem;
    border: 0.18rem solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .loader {
      animation: none;
    }
  }
</style>
