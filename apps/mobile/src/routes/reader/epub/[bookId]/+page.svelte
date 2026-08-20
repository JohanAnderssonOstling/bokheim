<script>
  import { page } from "$app/state";
  import ReaderV2 from "../../v2/ReaderV2.svelte";
</script>

<!-- A new book is a new reader: tearing down is what discards the old one. -->
{#key page.params.bookId}
  <ReaderV2 />
{/key}
