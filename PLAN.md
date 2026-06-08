# Image Pipeline Plan

1) Add image data structures
   - Add `ImageSource`, `ImageResource`, `InlineRun`, and `ImageFragment` to the document model.
   - Store image fragments per line for fast lookup in the renderer.

2) Parsing + layout updates
   - Parse `<img>` tags into image runs and resolve file/EPUB sources.
   - Probe intrinsic dimensions (header-only) for images without width/height attributes.
   - Update shaping and layout to handle mixed text/image runs and emit image fragments.

3) Background caches
   - Add a bytes worker and a decode worker using channels.
   - Maintain separate bytes and decoded caches, evicting outside the current+next page window.

4) Rendering integration
   - Prefetch images for current + next page lines.
   - Draw cached images in the line painter, fall back to placeholders when missing.
