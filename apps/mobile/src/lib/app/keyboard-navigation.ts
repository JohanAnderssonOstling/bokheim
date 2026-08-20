type ModalFocusOptions = {
  onClose: () => void;
  initialFocus?: string;
};

type ArrowNavigationOptions = {
  orientation: "horizontal" | "vertical";
  selector?: string;
};

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

function visibleElements(container: HTMLElement, selector: string) {
  return Array.from(container.querySelectorAll<HTMLElement>(selector)).filter(
    (element) =>
      element.getClientRects().length > 0 &&
      element.getAttribute("aria-hidden") !== "true",
  );
}

export function modalFocus(node: HTMLElement, options: ModalFocusOptions) {
  let currentOptions = options;
  const previousFocus =
    document.activeElement instanceof HTMLElement ? document.activeElement : null;

  const focusInitial = () => {
    const preferred = currentOptions.initialFocus
      ? node.querySelector<HTMLElement>(currentOptions.initialFocus)
      : null;
    const target =
      preferred && preferred.getClientRects().length > 0
        ? preferred
        : visibleElements(node, FOCUSABLE_SELECTOR)[0];
    target?.focus();
  };

  const handleKeydown = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      currentOptions.onClose();
      return;
    }
    if (event.key !== "Tab") return;

    const focusable = visibleElements(node, FOCUSABLE_SELECTOR);
    if (!focusable.length) {
      event.preventDefault();
      node.focus();
      return;
    }

    const first = focusable[0];
    const last = focusable.at(-1)!;
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !node.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && active === last) {
      event.preventDefault();
      first.focus();
    }
  };

  node.addEventListener("keydown", handleKeydown);
  const frame = requestAnimationFrame(focusInitial);

  return {
    update(nextOptions: ModalFocusOptions) {
      currentOptions = nextOptions;
    },
    destroy() {
      cancelAnimationFrame(frame);
      node.removeEventListener("keydown", handleKeydown);
      if (previousFocus?.isConnected) previousFocus.focus();
    },
  };
}

export function arrowKeyNavigation(node: HTMLElement, options: ArrowNavigationOptions) {
  let currentOptions = options;

  const handleKeydown = (event: KeyboardEvent) => {
    const previousKey =
      currentOptions.orientation === "vertical" ? "ArrowUp" : "ArrowLeft";
    const nextKey =
      currentOptions.orientation === "vertical" ? "ArrowDown" : "ArrowRight";
    if (![previousKey, nextKey, "Home", "End"].includes(event.key)) return;
    if (
      event.target instanceof HTMLInputElement ||
      event.target instanceof HTMLTextAreaElement ||
      event.target instanceof HTMLSelectElement
    )
      return;

    const selector = currentOptions.selector ?? "button:not([disabled])";
    const items = visibleElements(node, selector);
    const current =
      event.target instanceof Element
        ? event.target.closest<HTMLElement>(selector)
        : null;
    const index = current ? items.indexOf(current) : -1;
    if (index < 0 || !items.length) return;

    event.preventDefault();
    const targetIndex =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : event.key === previousKey
            ? (index - 1 + items.length) % items.length
            : (index + 1) % items.length;
    items[targetIndex]?.focus();
  };

  node.addEventListener("keydown", handleKeydown);
  return {
    update(nextOptions: ArrowNavigationOptions) {
      currentOptions = nextOptions;
    },
    destroy() {
      node.removeEventListener("keydown", handleKeydown);
    },
  };
}

export function managedMenu(node: HTMLDetailsElement) {
  const summary = node.querySelector<HTMLElement>("summary");
  const items = () =>
    visibleElements(
      node,
      "[role='menuitem']:not([disabled]), [role='menuitemcheckbox']:not([disabled]), [role='menuitemradio']:not([disabled])",
    );
  const close = (restoreFocus = true) => {
    if (!node.open) return;
    node.open = false;
    if (restoreFocus) summary?.focus();
  };
  const onToggle = () => {
    if (node.open) requestAnimationFrame(() => items()[0]?.focus());
  };
  const onPointerDown = (event: PointerEvent) => {
    if (node.open && event.target instanceof Node && !node.contains(event.target))
      close(false);
  };
  const onKeyDown = (event: KeyboardEvent) => {
    if (!node.open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (!["ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
    const menuItems = items();
    const index =
      document.activeElement instanceof HTMLElement
        ? menuItems.indexOf(document.activeElement)
        : -1;
    if (!menuItems.length) return;
    event.preventDefault();
    const target =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? menuItems.length - 1
          : event.key === "ArrowUp"
            ? (index - 1 + menuItems.length) % menuItems.length
            : (index + 1) % menuItems.length;
    menuItems[target]?.focus();
  };
  node.addEventListener("toggle", onToggle);
  node.addEventListener("keydown", onKeyDown);
  document.addEventListener("pointerdown", onPointerDown);
  return {
    destroy() {
      node.removeEventListener("toggle", onToggle);
      node.removeEventListener("keydown", onKeyDown);
      document.removeEventListener("pointerdown", onPointerDown);
    },
  };
}
