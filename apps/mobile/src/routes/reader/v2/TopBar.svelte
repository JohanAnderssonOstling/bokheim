<!--
  A bar of buttons. It carries only the action it cannot perform itself: going
  back is a history pop, and settings own their trigger and panel.

  The measurements match the v1 reader's chrome so the two look the same —
  notably the safe-area padding, which is what keeps the buttons clear of the
  status bar and the rounded corners on a phone.
-->
<script>
  import SquareButton from "../../../components/shared/buttons/SquareButton.svelte";
  import FileIcon from "../../../components/shared/FileIcon.svelte";
  import arrowLeft from "@icons/phosphor/arrow-left.svg";
  import folder from "@icons/phosphor/folder.svg";
  import magnifyingGlass from "@icons/phosphor/magnifying-glass.svg";
  import Settings from "./Settings.svelte";

  /// Which sidebar panel is showing, or "" when the sidebar is closed.
  let {
    title = "",
    panel = "",
    searchOpen = false,
    onClose,
    onContents,
    onSearch,
    hidden = false,
  } = $props();
</script>

<header
  class="reader-topbar"
  class:hidden
  aria-label="Reader controls"
>
  <div class="reader-topbar-side">
    <SquareButton
      label="Return to library"
      onclick={onClose}
    >
      <FileIcon source={arrowLeft} />
    </SquareButton>
    <SquareButton
      active={panel === "contents"}
      label="Contents and annotations"
      onclick={onContents}
    >
      <FileIcon source={folder} />
    </SquareButton>
  </div>

  <div class="reader-topbar-title">
    <strong>{title}</strong>
  </div>

  <div class="reader-topbar-side reader-topbar-side--right">
    <SquareButton
      active={searchOpen}
      label="Search in book"
      onclick={onSearch}
    >
      <FileIcon source={magnifyingGlass} />
    </SquareButton>
    <Settings />
  </div>
</header>

<style>
  .reader-topbar {
    --reader-topbar-height: 3.625rem;
    --reader-topbar-top: calc(
      var(--reader-topbar-height, 3.625rem) + env(safe-area-inset-top)
    );

    position: relative;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    box-sizing: border-box;
    min-width: 0;
    height: var(--reader-topbar-top);
    min-height: var(--reader-topbar-top);
    padding: env(safe-area-inset-top) max(14px, env(safe-area-inset-right)) 0
      max(14px, env(safe-area-inset-left));
    background: var(--color-surface-bg);
    border-bottom: 1px solid var(--color-border);
  }

  /* Collapsed rather than unmounted, so the buttons keep their state and the
     book grows into the space instead of being covered. */
  .reader-topbar.hidden {
    display: none;
  }

  .reader-topbar-side {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .reader-topbar-side--right {
    justify-content: flex-end;
  }

  .reader-topbar-title {
    display: grid;
    flex: 1;
    min-width: 0;
    padding-inline: 5px;
  }

  .reader-topbar strong {
    overflow: hidden;
    margin: 0;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-interface-family);
    font-size: 1rem;
    font-weight: 600;
    line-height: 1.1;
  }
</style>
