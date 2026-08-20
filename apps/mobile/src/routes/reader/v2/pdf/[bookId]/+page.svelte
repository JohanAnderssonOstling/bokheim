<script>
  import { page } from "$app/state";
  import PdfReaderV2 from "../PdfReaderV2.svelte";
</script>

<!-- A new book is a new reader: tearing down is what discards the old one. -->
{#key page.params.bookId}
  <PdfReaderV2 />
{/key}
