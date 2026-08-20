<script lang="ts">
  import { goto } from "$app/navigation";
  import { onMount } from "svelte";
  import EmptyPage from "../../components/shell/EmptyPage.svelte";
  import StatusState from "../../components/shared/StatusState.svelte";
  import { commands } from "../../lib/app/commands";
  import { libraryRootHref } from "../../lib/app/navigation";

  let loading = $state(true);
  let error = $state("");
  let hasLibraries = $state(false);

  onMount(() => {
    void commands.getLibraries().then(
      async (libraries) => {
        hasLibraries = libraries.length > 0;
        loading = false;
        if (libraries[0])
          await goto(libraryRootHref(libraries[0].library_id), { replaceState: true });
      },
      (reason) => {
        loading = false;
        error = reason instanceof Error ? reason.message : String(reason);
      },
    );
  });
</script>

<svelte:head><title>Library · Bokheim</title></svelte:head>
{#if loading}<StatusState message="Loading library" />
{:else if error}<StatusState
    kind="error"
    message={error}
  />
{:else if !hasLibraries}<EmptyPage
    title="No libraries"
    description="Create a library to organize your books."
  />
{/if}
