<script lang="ts">
  import { page } from "$app/state";
  import TrashPage from "./TrashPage.svelte";
  import { createTrashPageController } from "./trash-page";
  import { onLibraryRefresh } from "../../../../lib/app/library-events";

  const trash = createTrashPageController();

  $effect(() => {
    void trash.load(page.params.libraryId ?? "");
  });

  $effect(() =>
    onLibraryRefresh(() => void trash.load(page.params.libraryId ?? "", true)),
  );
</script>

<svelte:head>
  <title>Trash · Bokheim</title>
</svelte:head>

<TrashPage
  contents={$trash.contents}
  loading={$trash.loading}
  busy={$trash.busy}
  error={$trash.error}
  onretry={() => trash.load($trash.libraryId, true)}
  onrestorebook={trash.restoreBook}
  onpurgebook={trash.purgeBook}
  onrestorefolder={trash.restoreFolder}
  onpurgefolder={trash.purgeFolder}
  onempty={trash.empty}
/>
