<script lang="ts">
  import type { ApkUpdateState } from "../../lib/apk-updater";
  let { state, oninstall, ondismiss }: { state: ApkUpdateState; oninstall: () => void; ondismiss: () => void } = $props();
  const busy = $derived(state.phase === "downloading" || state.phase === "installing");
</script>

<aside class="update-notice" aria-live="polite">
  <div>
    <strong>{state.phase === "permission_required" ? "Allow APK installation" : state.phase === "installing" ? "Ready to install" : state.phase === "downloading" ? "Downloading update" : `Bokheim ${state.version ?? "update"} is available`}</strong>
    <small>{state.phase === "permission_required" ? "Enable Install unknown apps for Bokheim, then return here." : state.phase === "installing" ? "Confirm the update in Android’s installer." : state.phase === "downloading" && state.progress !== null ? `${Math.round(state.progress * 100)}%` : "Download the signed APK from GitHub Releases."}</small>
  </div>
  {#if !busy}<button type="button" class="install" onclick={oninstall}>{state.phase === "permission_required" ? "Open settings" : "Update"}</button>{/if}
  {#if state.phase === "available"}<button type="button" class="dismiss" aria-label="Dismiss update notification" onclick={ondismiss}>×</button>{/if}
</aside>

<style>
  .update-notice { position: fixed; z-index: 120; top: max(.6rem, env(safe-area-inset-top)); left: 50%; display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; gap: .65rem; width: min(calc(100% - 1.2rem), 34rem); padding: .7rem; transform: translateX(-50%); border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); box-shadow: 0 .3rem 1.1rem color-mix(in srgb, var(--color-ink-text) 18%, transparent); }
  div { display: grid; gap: .12rem; min-width: 0; }
  strong { font-size: .82rem; } small { color: var(--color-subtle-text); font-size: .69rem; }
  button { min-height: 2.5rem; border: 1px solid var(--color-border); border-radius: 0; font: inherit; cursor: pointer; }
  .install { padding: .45rem .8rem; color: var(--color-raised-bg); background: var(--color-accent); font-size: .75rem; font-weight: 750; }
  .dismiss { width: 2.5rem; padding: 0; color: var(--color-ink-text); background: var(--color-raised-bg); font-size: 1.1rem; }
</style>
