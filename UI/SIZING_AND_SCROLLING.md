# Sizing & Scrolling Guide

How layout sizing works in this UI, and the rules you must follow to keep views
constrained to the viewport (and to make horizontal scrolls actually scroll).

## The engine: floem 0.2.0 → taffy 0.4

This project pins **floem 0.2.0**, which uses **taffy 0.4** as its layout engine.
This matters because **taffy 0.4 has no `overflow` property**. floem's `Scroll`
view does *not* set `overflow: scroll` in taffy — its only layout style is
`items_start()`. Newer floem/taffy versions behave differently; do not assume
advice from newer docs applies here.

Two consequences drive everything below:

1. **Every flex item defaults to `min-width: auto`, which resolves to its
   min-content width.** There is no scroll-container rule to zero this out (that
   rule only exists in taffy ≥ 0.5). So a node will refuse to shrink below the
   natural width of its content unless you *explicitly* set `min_width(0)`.

2. **A node with no width style sizes to its content.** A bare
   `dyn_view(...)`, `container(...)`, or `v_stack(...)` with no `.style()` takes
   its content's width — even when its parent is correctly constrained. The
   parent being the right width does not force the child to that width.

## The core rule

> **For width to be constrained by the viewport, *every* node on the path from a
> width-definite root down to (and including) your content must carry
> `width_full()` and `min_width(0)`.** A single missing link lets the subtree
> below it balloon to min-content width.

`width_full()` says "be as wide as my parent." `min_width(0)` says "you are
allowed to be narrower than my content." You almost always need both together.
The height equivalents are `height_full()` / `min_height(0)` for vertical
constraint.

## Scrolling

A `Scroll` deliberately lets its content overflow — that is what gives it
something to scroll. So:

- **The scroll node itself** must be constrained (`width_full().min_width(0)` for
  a horizontal scroll, `height_full().min_height(0)` for vertical). If the scroll
  node isn't pinned, it grows to its content and never clips, so no scrollbar
  appears.
- **The scroll's direct child** that holds the scrollable content is the *one*
  place you want the natural (large) size on the scroll axis — leave it
  unconstrained on that axis so it can overflow and scroll.
- **Any other view you nest *inside* a scroll** (e.g. a `dyn_view` wrapper that
  produces the real content) still needs `width_full().min_width(0)`, or it will
  blow up to its content width and defeat the scroll.

A horizontal carousel is therefore: a constrained outer container → a scroll node
pinned with `width_full().min_width(0)` → a `flex_row()` of fixed-width cards left
to overflow on the main axis.

## Worked example: the home carousels

The home page nests a horizontal carousel scroll inside a vertical page scroll.
The chain (root → leaf) is roughly:

```
app_root (min_width 0)
  page nav stack (min_width 0)
    home_view container (min_width 0)
      full_page_stack (min_width 0)
        full_page_container (min_width 0)
          dyn_view  ← wraps library_view        [MUST pin: width_full + min_width 0]
            library_view h_stack (min_width 0)
              content stack of tabs (min_width 0)
                home_tab → library_home_carousels
                  dyn_view  ← scroll content     [MUST pin: width_full + min_width 0]
                    .scroll() (vertical, width_full + min_width 0)
                      sections column (min_width 0)
                        section v_stack (width_full)
                          carousel row .scroll()  (width_full + min_width 0)
                            flex_row of 200px cards  ← overflows → scrolls
```

The bug that motivated this doc: two `dyn_view` wrappers had **no width style**
(`home_page.rs` content_view, and `home_carousels.rs::library_home_carousels`).
Each sized to its min-content and dragged the whole subtree wider than the
window, so the carousels ran off-screen with no horizontal scroll. Fix was to add
`width_full().min_width(0)` to both. Note that intermediate wrappers (like the
section `v_stack`) did **not** need `min_width(0)` once the two `dyn_view`s were
pinned — so don't spray `min_width(0)` everywhere; find the unpinned link.

## Checklist when a view won't constrain / a scroll won't scroll

1. Walk the parent chain from a known-definite ancestor down to the content.
2. Find the first node that lacks `width_full()` **or** `min_width(0)` — bare
   `dyn_view` / `container` / stack wrappers are the usual culprits.
3. Pin it with `.style(|s| s.width_full().min_width(0))` (add the `height_*`
   variants for vertical).
4. Confirm the scroll node itself is pinned, and its scrollable child is left
   free only on the scroll axis.

## How to verify layout empirically

Don't reason about taffy from source — versions differ and it's easy to read the
wrong one. Instead, attach `.on_resize()` to the nodes in question and read the
computed widths:

```rust
view.on_resize(|rect| println!("NODE width = {}", rect.width()))
```

Run headlessly and inspect the output:

```
timeout 5 ./target/debug/Floem-Epub > /tmp/sizes.log 2>/dev/null
sort -u /tmp/sizes.log
```

A node whose width jumps to thousands of px (its content width) instead of the
window width is your unconstrained link. A throwaway crate that depends on
`floem = "0.2.0"` and reproduces just the nesting in question is the fastest way
to bisect a layout issue without driving the full app.
