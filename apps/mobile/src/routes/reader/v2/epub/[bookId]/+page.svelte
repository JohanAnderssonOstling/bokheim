<script>
  import { page } from "$app/state";
  import EpubReaderV2 from "../EpubReaderV2.svelte";
</script>

<!-- A new book is a new reader: tearing down is what discards the old one. -->
{#key page.params.bookId}
  <EpubReaderV2 />
{/key}
