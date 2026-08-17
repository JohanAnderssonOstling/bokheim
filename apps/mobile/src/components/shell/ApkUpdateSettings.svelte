<script lang="ts">
  import { onMount } from "svelte";
  import { apkUpdaterAvailable, checkForApkUpdate, EMPTY_APK_UPDATE_STATE, installApkUpdate, observeApkUpdates, openApkInstallSettings, type ApkUpdateState } from "../../lib/apk-updater";

  const available = apkUpdaterAvailable();
  let state = $state<ApkUpdateState>({ ...EMPTY_APK_UPDATE_STATE });
  onMount(() => available ? observeApkUpdates((next) => (state = next)) : undefined);
  const busy = $derived(state.phase === "checking" || state.phase === "downloading" || state.phase === "installing");
  const description = $derived.by(() => {
    if (state.phase === "available") return `Bokheim ${state.version} is available from GitHub Releases.`;
    if (state.phase === "current") return `Bokheim ${state.currentVersion ?? ""} is up to date.`;
    if (state.phase === "checking") return "Checking GitHub Releases…";
    if (state.phase === "downloading") return state.progress === null ? "Downloading the signed APK…" : `Downloading the signed APK… ${Math.round(state.progress * 100)}%`;
    if (state.phase === "permission_required") return "Allow Bokheim to install unknown apps, then return to continue.";
    if (state.phase === "installing") return "Confirm the update in Android’s package installer.";
    if (state.phase === "error") return state.error ?? "Could not check for updates.";
    return "Updates installed directly from GitHub require Android’s confirmation.";
  });
</script>

{#if available}
  <fieldset>
    <legend>Software updates</legend>
    <div class="update-row">
      <span><strong>GitHub APK updates</strong><small class:error={state.phase === "error"}>{description}</small></span>
      {#if state.phase === "available"}
        <button type="button" onclick={() => installApkUpdate(state)}>Update</button>
      {:else if state.phase === "permission_required"}
        <button type="button" onclick={openApkInstallSettings}>Open settings</button>
      {:else}
        <button type="button" disabled={busy} onclick={() => checkForApkUpdate(true)}>{busy ? "Working…" : "Check now"}</button>
      {/if}
    </div>
  </fieldset>
{/if}

<style>
  fieldset { display: grid; gap: .55rem; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: .55rem; color: var(--color-subtle-text); font-size: .75rem; font-weight: 750; }
  .update-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: .8rem; min-height: 3.45rem; padding: .55rem .7rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); }
  span { display: grid; gap: .12rem; min-width: 0; } strong { font-size: .84rem; } small { color: var(--color-subtle-text); font-size: .7rem; } small.error { color: var(--color-danger-text); }
  button { min-height: 2.5rem; padding: .45rem .7rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); font: inherit; font-size: .72rem; font-weight: 700; white-space: nowrap; cursor: pointer; }
  button:disabled { opacity: .55; cursor: default; }
</style>
