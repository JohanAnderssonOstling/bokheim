<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import SubjectBrowse from "./SubjectBrowse.svelte";
  import { commands } from "../../../lib/app/commands";
  import { ROOT_DIRECTORY_ID } from "../../../lib/library/directory-id";
  import { catalogActionKey, type CatalogImportAction } from "../catalog/actions";
  import { createStorePage } from "../store-page.svelte";
  import StoreRouteShell from "../StoreRouteShell.svelte";
  import type {
    CatalogLink,
    CatalogPublication,
    CatalogPublicationKey,
    CatalogSource,
    CatalogSubjectBrowseView,
  } from "../../../lib/bindings";

  const page = createStorePage();
  let sources = $state<CatalogSource[]>([]);
  let browse = $state<CatalogSubjectBrowseView | null>(null);
  let levels = $state<{ title: string; url: string }[]>([]);
  let sourceIds = $state<string[]>([]);
  let query = $state("");
  let loading = $state(false);
  let error = $state("");
  let ownership = $state<Record<string, boolean>>({});
  let actionStates = $state<Record<string, "idle" | "adding" | "added">>({});
  let actionErrors = $state<Record<string, string>>({});

  const sourceLabel = $derived(
    sourceIds.length === 0
      ? "All catalogues"
      : sourceIds.length === 1
        ? (sources.find((source) => source.id === sourceIds[0])?.title ?? sourceIds[0])
        : `${sourceIds.length} catalogues`,
  );
  const groups = $derived.by(() => {
    const current = browse;
    if (!current) return [];
    return sources.flatMap((source) => {
      const publications = current.response.publications
        .filter((item) => item.source_id === source.id)
        .map((item) => item.publication);
      return publications.length ? [{ source, publications }] : [];
    });
  });

  function message(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function ownershipKey(sourceId: string, editionId: string) {
    return `${sourceId}:${editionId}`;
  }

  function ownedEditions(keys: CatalogPublicationKey[]) {
    return Object.fromEntries(
      keys.map((key) => [ownershipKey(key.source_id, key.publication.edition_id), true]),
    );
  }

  function linkKey(link: CatalogLink) {
    const destination =
      link.target.kind === "page" ? link.target.locator : link.target.url;
    return `${link.target.kind}:${destination}:${link.title}`;
  }

  function isOwned(source: CatalogSource, publication: CatalogPublication) {
    return Boolean(ownership[ownershipKey(source.id, publication.identity.edition_id)]);
  }

  async function loadSubjects(locator: string | null, title?: string, push = false) {
    if (loading) return;
    loading = true;
    error = "";
    try {
      const view = await commands.getCatalogSubjects(
        page.state.activeLibraryId,
        locator,
        sourceIds,
        query.trim(),
        48,
      );
      browse = view;
      ownership = ownedEditions(view.owned_publications);
      if (!levels.length || locator === null) levels = [{ title: "Subjects", url: "" }];
      else if (push && title) levels = [...levels, { title, url: locator }];
    } catch (reason) {
      error = message(reason);
    } finally {
      loading = false;
    }
  }

  async function setSources(next: string[]) {
    sourceIds = next;
    await loadSubjects(levels.at(-1)?.url || null);
  }

  async function searchSubjects(event: SubmitEvent) {
    event.preventDefault();
    await loadSubjects(levels.at(-1)?.url || null);
  }

  async function goToLevel(index: number) {
    if (loading || index < 0 || index >= levels.length) return;
    levels = levels.slice(0, index + 1);
    await loadSubjects(levels[index]?.url || null);
  }

  async function openLink(link: CatalogLink) {
    if (link.target.kind === "page")
      await loadSubjects(link.target.locator, link.title, true);
  }

  async function addBook(
    source: CatalogSource,
    publication: CatalogPublication,
    action: CatalogImportAction,
  ) {
    if (action.kind !== "import") return;
    const key = catalogActionKey(action);
    if (actionStates[key] === "adding" || isOwned(source, publication)) return;
    actionStates = { ...actionStates, [key]: "adding" };
    try {
      await commands.importCatalogBook(
        { library_id: page.state.activeLibraryId, directory_id: ROOT_DIRECTORY_ID },
        action.request,
      );
      actionStates = { ...actionStates, [key]: "added" };
      ownership = {
        ...ownership,
        [ownershipKey(source.id, publication.identity.edition_id)]: true,
      };
      await page.imported();
    } catch (reason) {
      actionStates = { ...actionStates, [key]: "idle" };
      actionErrors = { ...actionErrors, [key]: message(reason) };
    }
  }

  onMount(async () => {
    sources = await commands.getCatalogSources().catch(() => []);
    await loadSubjects(null);
  });
</script>

<svelte:head><title>Store subjects · Bokheim</title></svelte:head>

<StoreRouteShell
  activeLibraryId={page.state.activeLibraryId}
  libraryName={page.activeLibrary?.library_name ?? "Libraries"}
  preferences={page.browsing()}
  onpreferencechange={page.savePreference}
  onimported={page.imported}
  oncreatelibrary={page.createLibrary}
>
  {#if browse}
    <SubjectBrowse
      {browse}
      {levels}
      {sources}
      {sourceIds}
      {query}
      {loading}
      {error}
      {groups}
      {sourceLabel}
      getLinkKey={linkKey}
      {isOwned}
      {actionStates}
      {actionErrors}
      onclose={() => void goto("/store")}
      onlevel={goToLevel}
      onsearch={searchSubjects}
      onsourcechange={setSources}
      onselect={openLink}
      onopensource={() => goto("/store")}
      onaction={addBook}
      onpage={(locator) => loadSubjects(locator)}
    />
  {:else if loading}
    <p>Loading subjects…</p>
  {:else}
    <p
      class="error"
      role="alert"
    >
      {error || "Could not load subjects."}
    </p>
  {/if}
</StoreRouteShell>
