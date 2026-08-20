<script lang="ts">
  import type { Snippet } from "svelte";
  import ButtonGroup from "../../ButtonGroup.svelte";
  import PanelShell from "../panels/PanelShell.svelte";

  type SidebarTab = "contents" | "highlights" | "bookmarks";

  let {
    activeTab,
    onTabChange,
    hidden = false,
    contents,
    highlights,
    bookmarks,
  }: {
    activeTab: SidebarTab;
    onTabChange: (tab: SidebarTab) => void;
    hidden?: boolean;
    contents: Snippet;
    highlights: Snippet;
    bookmarks: Snippet;
  } = $props();

  const tabs = [
    { value: "contents", label: "Contents" },
    { value: "highlights", label: "Highlights" },
    { value: "bookmarks", label: "Bookmarks" },
  ];
</script>

<div {hidden}>
<PanelShell
  variant="sidebar"
  ariaLabel={activeTab === "contents"
    ? "Table of contents"
    : activeTab === "highlights"
      ? "Highlights"
      : "Bookmarks"}
>
  <ButtonGroup
    options={tabs}
    value={activeTab}
    ariaLabel="Reader sidebar"
    width="100%"
    onSelect={(tab) => onTabChange(tab as SidebarTab)}
  />

  {#if activeTab === "contents"}
    {@render contents()}
  {:else if activeTab === "highlights"}
    {@render highlights()}
  {:else}
    {@render bookmarks()}
  {/if}
</PanelShell>
</div>
