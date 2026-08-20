<script lang="ts">
  import TopBar from "./TopBar.svelte";
  import SquareButton from "../../buttons/SquareButton.svelte";
  import FileIcon from "../../FileIcon.svelte";
  import arrowLeft from "@icons/phosphor/arrow-left.svg";
  import folder from "@icons/phosphor/folder.svg";
  import magnifyingGlass from "@icons/phosphor/magnifying-glass.svg";

  let {
    title,
    onclose,
    onToggleSidebar,
    onToggleSearch,
    onToggleSettings,
    sidebarOpen,
    searchOpen,
    settingsOpen,
    hiddenChrome = false,
    fixed = false,
    ariaLabel = "Reader controls",
    searchLabel = "Search",
    sidebarLabel = "Contents and annotations",
    settingsLabel = "Settings",
    progressLabel,
  }: {
    title: string;
    onclose: () => void;
    onToggleSidebar: () => void;
    onToggleSearch: () => void;
    onToggleSettings: () => void;
    sidebarOpen: boolean;
    searchOpen: boolean;
    settingsOpen: boolean;
    hiddenChrome?: boolean;
    fixed?: boolean;
    ariaLabel?: string;
    searchLabel?: string;
    sidebarLabel?: string;
    settingsLabel?: string;
    progressLabel?: string;
  } = $props();
</script>

<TopBar
  {ariaLabel}
  {fixed}
  {hiddenChrome}
>
  <svelte:fragment slot="left">
    <SquareButton
      label="Return to library"
      onclick={onclose}
    >
      <FileIcon source={arrowLeft} />
    </SquareButton>
    <SquareButton
      active={sidebarOpen}
      label={sidebarLabel}
      onclick={onToggleSidebar}
    >
      <FileIcon source={folder} />
    </SquareButton>
  </svelte:fragment>

  <div class="reader-topbar-title">
    <strong>{title}</strong>
  </div>

  <svelte:fragment slot="right">
    <SquareButton
      active={searchOpen}
      label={searchLabel}
      onclick={onToggleSearch}
    >
      <FileIcon source={magnifyingGlass} />
    </SquareButton>
    {#if progressLabel}
      <output aria-live="polite">{progressLabel}</output>
    {/if}
    <SquareButton
      active={settingsOpen}
      label={settingsLabel}
      onclick={onToggleSettings}
    >
      <slot name="settings-content">
        {settingsLabel}
      </slot>
    </SquareButton>
  </svelte:fragment>
</TopBar>
