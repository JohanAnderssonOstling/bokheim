<script lang="ts">
  let {
    src,
    title,
    fallbackText,
    alt = "",
    className = "",
    fit = "cover",
    disabled = false,
    ariaLabel,
    onclick,
  }: {
    src?: string | null;
    title: string;
    fallbackText?: string;
    alt?: string;
    className?: string;
    fit?: "cover" | "contain";
    disabled?: boolean;
    ariaLabel?: string;
    onclick?: (event: MouseEvent) => void | Promise<void>;
  } = $props();
</script>

{#if onclick}
  <button
    class={`book-cover interactive ${className}`.trim()}
    class:contain={fit === "contain"}
    type="button"
    {disabled}
    aria-label={ariaLabel}
    {onclick}
  >
    {#if src}
      <img
        {src}
        {alt}
      />
    {:else}
      <span>{fallbackText ?? (title.slice(0, 1) || "?")}</span>
    {/if}
  </button>
{:else}
  <span
    class={`book-cover ${className}`.trim()}
    class:contain={fit === "contain"}
    aria-hidden={alt ? undefined : "true"}
  >
    {#if src}
      <img
        {src}
        {alt}
      />
    {:else}
      <span>{fallbackText ?? (title.slice(0, 1) || "?")}</span>
    {/if}
  </span>
{/if}

<style>
  .book-cover {
    display: grid;
    min-width: 0;
    min-height: 0;
    place-items: center;
    overflow: hidden;
    background: var(--color-raised-bg);
  }

  .book-cover.interactive {
    padding: 0;
    border: 0;
    color: inherit;
    cursor: pointer;
    font: inherit;
    -webkit-tap-highlight-color: transparent;
  }

  .book-cover.interactive:active {
    transform: scale(0.985);
  }

  .book-cover.interactive:disabled {
    cursor: default;
    opacity: 0.58;
  }

  .book-cover img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .book-cover.contain img {
    object-fit: contain;
    object-position: center top;
  }

  .book-cover span {
    color: var(--color-subtle-text);
    font-size: 1.5rem;
    font-weight: 700;
  }

  .book-cover.detail {
    width: 100%;
    aspect-ratio: 5 / 8;
    color: var(--color-subtle-text);
    background: var(--color-raised-bg);
    box-shadow: 0 0.55rem 1.5rem var(--color-box-shadow);
  }

  .book-cover.detail img {
    height: auto;
  }

  .book-cover.detail > span {
    font-size: 2.5rem;
  }

  .book-cover.audiobook-expanded {
    width: 82px;
    height: 110px;
    padding: 0.35rem;
    border: 1px solid var(--color-border);
  }

  .book-cover.audiobook-expanded > span {
    display: -webkit-box;
    overflow: hidden;
    font-size: 0.8rem;
    font-weight: 600;
    line-height: 1.1;
    line-clamp: 5;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 5;
  }

  .book-cover.audiobook-dock {
    display: flex;
    align-items: flex-end;
    width: 42px;
    height: 58px;
    padding: 0.22rem;
    border: 1px solid var(--color-border);
  }

  .book-cover.audiobook-dock > span {
    display: -webkit-box;
    overflow: hidden;
    font-size: 0.55rem;
    font-weight: 600;
    line-height: 1.05;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
  }

  @media (min-width: 45rem) {
    .book-cover.audiobook-dock {
      width: 48px;
      height: 64px;
    }
  }
</style>
