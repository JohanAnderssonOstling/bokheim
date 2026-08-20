<script lang="ts">
  import SquareButton from "../../buttons/SquareButton.svelte";
  import MenuItemButton from "../../buttons/MenuItemButton.svelte";
  import SecondaryButton from "../../buttons/SecondaryButton.svelte";
  import type { ReaderBookmark } from "$lib/shared/types.ts";
  import { openReaderBookmark, readerBookmarks } from "$lib/reader/marks";
  import { addBookmark, deleteBookmark } from "$lib/reader/bookmarks";
  let {
    bookmarks: passedBookmarks = [],
    cfi = "",
    label = "",
    onOpen,
  }: {
    bookmarks?: ReaderBookmark[];
    cfi?: string;
    label?: string;
    onOpen?: (bookmark: ReaderBookmark) => void;
  } = $props();

  let created = $state<ReaderBookmark[]>([]);
  let deleted = $state<Set<string>>(new Set());
  const bookmarks = $derived([...created, ...(readerBookmarks() ?? passedBookmarks)]);
  async function add() {
    if (!cfi) return;
    created = [await addBookmark(cfi, label || "Bookmark"), ...created];
  }

  async function remove(id: string) {
    await deleteBookmark(id);
    deleted = new Set([...deleted, id]);
  }
</script>

<SecondaryButton
  disabled={!cfi}
  onclick={() => void add()}
>Bookmark here</SecondaryButton>

<div class="bookmark-list">
  {#each bookmarks.filter((bookmark) => !deleted.has(bookmark.id)) as bookmark (bookmark.id)}
    <div>
      <MenuItemButton
        role={null}
        onclick={() => void (readerBookmarks() ? openReaderBookmark(bookmark.id) : onOpen?.(bookmark))}
      >{bookmark.label}</MenuItemButton>
      <SquareButton
        label="Delete bookmark"
        onclick={() => void remove(bookmark.id)}
      >×</SquareButton>
    </div>
  {/each}
</div>

<style>
  .bookmark-list {
    display: grid;
    gap: 0.4rem;
    max-height: 42dvh;
    overflow: auto;
    margin-top: 0.5rem;
  }

  .bookmark-list > div {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 0.25rem;
    border: 1px solid var(--chrome-strong);
  }
</style>
