<script lang="ts">
  import UpdateNotice from "./UpdateNotice.svelte";
  import {
    apkUpdaterAvailable,
    checkForApkUpdate,
    EMPTY_APK_UPDATE_STATE,
    installApkUpdate,
    observeApkUpdates,
    openApkInstallSettings,
    type ApkUpdateState,
  } from "../../lib/app/apk-updater";

  /**
   * The Android update prompt, start to finish.
   *
   * Nothing outside this component needs the update state, so it subscribes,
   * decides when it is worth showing, and remembers what the reader dismissed.
   * On every other platform `apkUpdaterAvailable()` is false and this renders
   * nothing.
   */
  let update = $state<ApkUpdateState>({ ...EMPTY_APK_UPDATE_STATE });
  let dismissedVersion = $state("");

  // An update being downloaded or installed is not dismissable: it reports
  // progress until it finishes. Only an available one can be waved away, and
  // only until a newer version supersedes what was dismissed.
  const visible = $derived(
    update.phase === "downloading" ||
      update.phase === "permission_required" ||
      update.phase === "installing" ||
      (update.phase === "available" && update.version !== dismissedVersion),
  );

  $effect(() => {
    if (!apkUpdaterAvailable()) return;
    checkForApkUpdate(false);
    return observeApkUpdates((next) => (update = next));
  });
</script>

{#if visible}
  <UpdateNotice
    state={update}
    oninstall={() =>
      update.phase === "permission_required"
        ? openApkInstallSettings()
        : installApkUpdate(update)}
    ondismiss={() => (dismissedVersion = update.version ?? "dismissed")}
  />
{/if}
