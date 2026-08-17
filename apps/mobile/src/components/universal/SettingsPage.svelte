<script lang="ts">
  import BrowsingSettingsDialog from "../shell/BrowsingSettingsDialog.svelte";
  import type { BrowsingPreferencePatch, BrowsingPreferences, NetworkStatus, TransferPreferencePatch, TransferPreferences } from "../../lib/types";
  let { preferences, networkStatus, transferPreferences, serverConnected, error, onchange, ontransferpreference }: {
    preferences: BrowsingPreferences; networkStatus: NetworkStatus; transferPreferences: TransferPreferences; serverConnected: boolean; error: string;
    onchange: (patch: BrowsingPreferencePatch) => void | Promise<void>;
    ontransferpreference: (patch: TransferPreferencePatch) => void | Promise<void>;
  } = $props();
</script>
<BrowsingSettingsDialog {preferences} {networkStatus} {transferPreferences} {serverConnected} {error} {onchange} {ontransferpreference} />
