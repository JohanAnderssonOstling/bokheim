# Bundled application fonts

Bokheim embeds these fonts so desktop and AppImage builds do not depend on the
ignored `reference/plato` development checkout or on fonts installed by the
host system.

- Noto Sans Regular and Bold are from the Noto Fonts project and are licensed
  under the SIL Open Font License 1.1. See `LICENSE-NOTO.txt`.
- Bodoni Moda Regular and SemiBold (version 2.005, static instances) are from
  https://github.com/indestructible-type/Bodoni and are licensed under the
  SIL Open Font License 1.1. See `LICENSE-BODONI-MODA.txt`. The radial graph
  uses it for prominent headings and graph labels with the serif UI choice.
- NV Garamond and NV Jost Regular, Bold, Italic and BoldItalic are from
  https://github.com/nicoverbruggen/ebook-fonts at revision
  `f875aca16d3e871a01af52c854b67bc7f7abce9d` (`fonts/core`). These reader fonts
  are variants of EB Garamond and Jost, licensed under the SIL Open Font License
  1.1. See `LICENSE-NV-GARAMOND.txt` and `LICENSE-NV-JOST.txt`. NV Garamond
  is used by the interface and reader; NV Jost is retained for comparison previews.
- Raleway Regular, Bold, Italic and BoldItalic (4.101, upstream static TTFs)
  are from https://github.com/theleagueof/raleway/releases/tag/4.101 and are
  licensed under the SIL Open Font License 1.1. See `LICENSE-RALEWAY.txt`.
  The interface and reader use this sans-serif family on every platform.

The shared `ui-components` font registration installs NV Garamond, Raleway and
Bodoni Moda once per application, before either the browser or reader uses them.
The sans-serif UI choice uses Raleway for both body text and prominent headings.

Libertinus is no longer bundled by the application. Historical comparison
assets and their license are in `apps/ui/design-tokens/assets/fonts/libertinus-comparison`.

The files are redistributed without modification.
