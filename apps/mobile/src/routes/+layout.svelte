<script lang="ts">
  import { onMount } from "svelte";
  import App from "../App.svelte";
  import { applyApplicationAppearance } from "../lib/application-appearance";
  import { DEFAULT_BROWSING_PREFERENCES } from "../lib/types";
  import "virtual:design-tokens.css";

  let { children } = $props();

  onMount(() => applyApplicationAppearance(DEFAULT_BROWSING_PREFERENCES));
</script>

<App />
<div hidden aria-hidden="true">{@render children()}</div>
