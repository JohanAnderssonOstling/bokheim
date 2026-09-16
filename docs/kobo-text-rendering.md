# Kobo text rendering

The Kobo interface uses the bundled Libertinus Serif family, matching the shared
interface font preference. Reader content continues to respect its font settings.

Glyph masks retain their original 8-bit coverage in both native-size and scaled
rendering paths. Pixel alignment remains enabled; glyph edges are no longer
converted to binary coverage before compositing.

Both browser and reader use the grayscale quality policy when settled. Fast
scrolling and the reader's fast page-turn path still use monochrome waveforms.
The presenter thresholds only monochrome updates; the cached frame remains gray.
After 800 ms without another fast update on Libra 2, the scheduler redraws the
accumulated affected area using partial GC16 and the latest grayscale frame.
Small intervening UI updates do not cancel this cleanup. A completed full refresh
cancels old cleanup work so an earlier frame cannot reappear after wake or navigation.

Regression tests cover native/scaled glyph coverage, scroll cleanup alongside UI
updates, page-turn cleanup, and full-refresh cancellation. Actual contrast,
refresh latency, and ghosting still need comparison on the physical Kobo.
