# html-view-core

This crate owns the renderer's Floem-independent state, selection, search
highlighting, pagination, painting, host interface, and command/event boundary.
The core owns document loading, cross-document navigation, selection, search, and
paint-command generation. Floem integration, concrete glyph-layout resolution,
and the window-system painter belong in the `html-renderer` façade.
