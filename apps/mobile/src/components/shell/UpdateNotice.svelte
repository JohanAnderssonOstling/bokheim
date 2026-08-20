<script lang="ts">
  import SquareButton from "../shared/buttons/SquareButton.svelte";
  import PrimaryButton from "../shared/buttons/PrimaryButton.svelte";
  import type { ApkUpdateState } from "../../lib/app/apk-updater";
  let {
    state,
    oninstall,
    ondismiss,
  }: { state: ApkUpdateState; oninstall: () => void; ondismiss: () => void } = $props();
  const busy = $derived(state.phase === "downloading" || state.phase === "installing");
</script>

<aside
  class="update-notice"
  aria-live="polite"
>
  <div>
    <strong
      >{state.phase === "permission_required"
        ? "Allow APK installation"
        : state.phase === "installing"
          ? "Ready to install"
          : state.phase === "downloading"
            ? "Downloading update"
            : `Bokheim ${state.version ?? "update"} is available`}</strong
    >
    <small
      >{state.phase === "permission_required"
        ? "Enable Install unknown apps for Bokheim, then return here."
        : state.phase === "installing"
          ? "Confirm the update in Android’s installer."
          : state.phase === "downloading" && state.progress !== null
            ? `${Math.round(state.progress * 100)}%`
            : "Download the signed APK from GitHub Releases."}</small
    >
  </div>
  {#if !busy}<PrimaryButton onclick={oninstall}
      >{state.phase === "permission_required" ? "Open settings" : "Update"}</PrimaryButton
    >{/if}
  {#if state.phase === "available"}<SquareButton
      label="Dismiss update notification"
      onclick={ondismiss}>×</SquareButton
    >{/if}
</aside>

<style>
  .update-notice {
    position: fixed;
    z-index: 120;
    top: max(0.6rem, env(safe-area-inset-top));
    left: 50%;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: 0.65rem;
    width: min(calc(100% - 1.2rem), 34rem);
    padding: 0.7rem;
    transform: translateX(-50%);
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: var(--color-ink-text);
    background: var(--color-raised-bg);
    box-shadow: 0 0.3rem 1.1rem color-mix(in srgb, var(--color-ink-text) 18%, transparent);
  }
  div {
    display: grid;
    gap: 0.12rem;
    min-width: 0;
  }
  strong {
    font-size: 0.82rem;
  }
  small {
    color: var(--color-subtle-text);
    font-size: 0.69rem;
  }
</style>
