<script lang="ts">
  import { onMount } from "svelte";
  import App from "../App.svelte";
  import { applyApplicationAppearance } from "../lib/app/application-appearance";
  import { reportBackendHostMismatch } from "../lib/app/platform";
  import { DEFAULT_BROWSING_PREFERENCES } from "../lib/shared/types";
  import "virtual:design-tokens.css";
  import "../app.css";

  let { children } = $props();

  onMount(() => {
    // Paint the default palette before the stored preferences arrive, so the
    // first frame is styled rather than unstyled.
    applyApplicationAppearance(DEFAULT_BROWSING_PREFERENCES);
    reportBackendHostMismatch();
  });
</script>

<App>
  {#snippet routeContent()}
    {@render children()}
  {/snippet}
</App>
