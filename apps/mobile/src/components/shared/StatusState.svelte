<script lang="ts">
  import PrimaryButton from "./buttons/PrimaryButton.svelte";

  let {
    kind = "empty",
    variant = "page",
    title = "",
    message = "",
    actionLabel = "",
    onaction,
  }: {
    kind?: "empty" | "loading" | "error";
    variant?: "page" | "compact" | "reader";
    title?: string;
    message?: string;
    actionLabel?: string;
    onaction?: () => void | Promise<void>;
  } = $props();
</script>

<section
  class={`status-state ${kind} ${variant}`}
  role={kind === "error" ? "alert" : undefined}
  aria-live="polite"
>
  {#if kind === "loading"}<span
      class="loader"
      aria-hidden="true"
    ></span>{/if}
  {#if title}<strong>{title}</strong>{/if}
  {#if message}<p>{message}</p>{/if}
  {#if actionLabel}
    <PrimaryButton onclick={() => void onaction?.()}>
      {actionLabel}
    </PrimaryButton>
  {/if}
</section>

<style>
  .status-state {
    display: grid;
    min-height: 240px;
    place-content: center;
    justify-items: center;
    gap: 0.55rem;
    padding: 1.125rem;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
    text-align: center;
  }

  .status-state strong {
    color: var(--color-ink-text);
  }

  .status-state p {
    max-width: 28rem;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .status-state.error {
    color: var(--color-danger-text);
  }

  .status-state.loading {
    grid-template-columns: auto 1fr;
    justify-content: center;
    text-align: start;
  }

  .status-state.compact {
    min-height: 12rem;
  }

  .status-state.reader {
    width: 100%;
    height: 100%;
    min-height: 0;
    box-sizing: border-box;
  }

  .loader {
    width: 1rem;
    height: 1rem;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: status-state-spin 700ms linear infinite;
  }

  @keyframes status-state-spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
