#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$workspace_root"

failed=0

check_direct_dependencies() {
    local package="$1"
    shift
    local allowed=" $* "
    local line dependency

    while IFS= read -r line; do
        dependency="${line%% *}"
        if [[ "$dependency" == "$package" ]]; then
            continue
        fi
        if [[ "$dependency" == gpui* || "$dependency" == floem* ]]; then
            echo "$package must remain UI-framework agnostic, but directly depends on $dependency" >&2
            failed=1
        fi
        if [[ "$dependency" == html-* && "$allowed" != *" $dependency "* ]]; then
            echo "$package has a forbidden direct HTML-crate dependency on $dependency" >&2
            failed=1
        fi
    done < <(cargo tree -p "$package" --edges normal --prefix none --depth 1 2>/dev/null)
}

# Pipeline direction: source/DOM -> parse/style -> layout -> orchestration.
# Resources form a separate source/DOM service used only by orchestration.
check_direct_dependencies html-source
check_direct_dependencies html-dom
check_direct_dependencies html-parse html-dom
check_direct_dependencies html-style-model html-dom
check_direct_dependencies html-style html-dom html-style-model
check_direct_dependencies html-layout html-dom html-style-model
check_direct_dependencies html-resources html-dom html-source
check_direct_dependencies html-pipeline html-dom html-parse html-style html-layout html-resources
check_direct_dependencies html-view-core html-layout html-pipeline html-resources

# These crates are implementation units of this workspace, not independently
# supported distribution APIs.
for manifest in HtmlRenderer/crates/html-*/Cargo.toml; do
    if ! rg -q '^publish = false$' "$manifest"; then
        echo "$manifest must set publish = false" >&2
        failed=1
    fi
done

if rg -n '\b(ComputedStyles|StyleIndices|LayoutTree|LayoutState|InlineContent|GlyphMetrics)\b' HtmlRenderer/crates/html-dom/src >/dev/null; then
    echo "html-dom contains downstream style/layout representation types" >&2
    failed=1
fi

if rg -n '^pub use html_(dom|style_model|layout_model|layout|pipeline|resources)::\*;' HtmlRenderer/crates/html-view-core/src >/dev/null; then
    echo "html-view-core exposes a wildcard representation reexport" >&2
    failed=1
fi
if rg -n '^pub use html_layout_model::\*;' HtmlRenderer/crates/html-layout/src >/dev/null; then
    echo "html-layout exposes its entire internal representation crate" >&2
    failed=1
fi
if rg -n '^\s*pub use html_layout_model::\*;' HtmlRenderer/crates/html-layout/src >/dev/null; then
    echo "html-layout exposes a wildcard internal-layout re-export" >&2
    failed=1
fi

if rg -n "html_layout_model::" HtmlRenderer/crates -g '*.rs' | rg -v '^HtmlRenderer/crates/html-layout/' >/dev/null; then
    echo "A crate outside html-layout depends on html-layout-model internals" >&2
    failed=1
fi
if rg -n "html-layout-model" HtmlRenderer/crates -g 'Cargo.toml' | rg -v 'HtmlRenderer/crates/html-layout-model/Cargo.toml' >/dev/null; then
    echo "a crate besides html-layout-model depends on the old layout model crate" >&2
    failed=1
fi
if rg -n '^pub mod document\b' HtmlRenderer/crates/html-view-core/src/lib.rs >/dev/null; then
    echo "html-view-core exposes its internal representation compatibility module" >&2
    failed=1
fi

if rg -n '^pub mod layout_model\b' HtmlRenderer/crates/html-layout/src/lib.rs >/dev/null; then
    echo "html-layout exposes its mutable layout-model module" >&2
    failed=1
fi
if rg -n '^pub use .*GlyphMetrics' HtmlRenderer/crates/html-layout/src/lib.rs >/dev/null; then
    echo "html-layout exposes its mutable glyph-metrics store" >&2
    failed=1
fi

# Branded DOM identities are issued by a builder/document, never reconstructed
# from an integer or a public raw constructor.
if rg -n 'impl\s+From<(u32|usize)>\s+for\s+DomNodeId|pub\s+fn\s+new\([^)]*(raw|index)[^)]*\)\s*->\s*(Self|DomNodeId)' HtmlRenderer/crates/html-dom/src --glob '*.rs' >/dev/null; then
    echo "DomNodeId exposes forgeable integer construction" >&2
    failed=1
fi

# Styling may inspect parsed content, but must not mutate parser-owned state or
# place CSS-created strings in the DOM interner.
if rg -n '&mut Document|\b(doc|document)\.intern_string\(' HtmlRenderer/crates/html-style/src --glob '*.rs' >/dev/null; then
    echo "html-style mutates parser-owned Document state" >&2
    failed=1
fi

# Lightning CSS is the style crate's parser implementation, not part of the
# pipeline contract. Matching and cascade consume only prepared effective-rule
# handles; raw stylesheet coordinates must not reappear in those stages.
if rg -n '^\s*pub (use|fn|struct|enum).*lightningcss' HtmlRenderer/crates/html-style/src/lib.rs >/dev/null; then
    echo "html-style leaks Lightning CSS parser types through its public boundary" >&2
    failed=1
fi
if rg -n '\bRuleKey\b|SelectorIndex::from_stylesheets|stylesheet_idx|rule_idx' HtmlRenderer/crates/html-style/src/style/{selectors,resolver}.rs >/dev/null; then
    echo "selector or cascade code bypasses the prepared effective-rule boundary" >&2
    failed=1
fi

# Style handles are opaque and store-branded. Downstream crates must use their
# accessors instead of constructing or rewriting table indices.
if rg -n -U 'StyleIndices\s*\{\s*(store_id|font_idx|text_idx|box_idx|border_idx|bg_idx)' HtmlRenderer/crates --glob '*.rs' --glob '!**/html-style-model/**' >/dev/null; then
    echo "a downstream crate constructs opaque StyleIndices" >&2
    failed=1
fi
if rg -n '^\s*pub\s+(font_idx|text_idx|box_idx|border_idx|bg_idx):' HtmlRenderer/crates/html-style-model/src >/dev/null; then
    echo "StyleIndices exposes forgeable index fields" >&2
    failed=1
fi
if rg -n '^\s*pub fn (font_style|text_style|box_model_style|border_style|background_style)\(&self,\s*idx:\s*u32' HtmlRenderer/crates/html-style-model/src HtmlRenderer/crates/html-style/src >/dev/null; then
    echo "a public style lookup accepts a forgeable raw table index" >&2
    failed=1
fi

# The renderer consumes the laid-out query contract, not raw stage storage.
if rg -n '\.document\.(styles|style_view|layout_tree|inline_content|layout_state|box_at|box_element|box_layout_mode|box_style_indices|inline_runs|inline_run|text_runs)\(' HtmlRenderer/crates/html-view-core/src --glob '*.rs' >/dev/null; then
    echo "html-view-core reaches raw layout-stage storage" >&2
    failed=1
fi


# All immutable laid-out reads in renderer code go through RenderView. The only
# direct LaidOutDocument operations allowed there are transitions/mutation with
# their own narrow contracts (render_view, relayout, auxiliary registration,
# and diagnostic memory reporting).
if rg -n '\b(self\.)?document\.(root_font_size|title|images|document_toc_entries|string|lookup_string|glyph_metrics|glyphs|glyph_at|glyph_slice|glyph|link_href_for_glyph|anchor_glyph|anchor_glyphs|line_count)\(' HtmlRenderer/crates/html-view-core/src HtmlRenderer/src/renderer --glob '*.rs' >/dev/null; then
    echo "renderer code bypasses the RenderView contract" >&2
    failed=1
fi

if rg -n '^\s*pub fn (document|styles|style_view|layout_tree|inline_content|layout_state|box_at|box_element|box_layout_mode|box_style_indices|glyph_metrics|inline_runs|inline_run)\(' HtmlRenderer/crates/html-layout/src/stages.rs >/dev/null; then
    echo "html-layout exposes raw stage storage" >&2
    failed=1
fi

if (( failed != 0 )); then
    exit 1
fi

echo "HTML crate boundaries are valid"
