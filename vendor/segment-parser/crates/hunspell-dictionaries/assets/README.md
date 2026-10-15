# Bundled Hunspell dictionaries

RustTrados embeds these dictionaries and materializes them as files at runtime
because the Hunspell C API accepts filesystem paths.

| Files | Language | Upstream | Packaged version | License |
| --- | --- | --- | --- | --- |
| `sv_SE.*` | Swedish (Sweden) | Generated: Den stora svenska ordlistan 2.42 + SALDO | see below | LGPL-3.0-only and CC BY 4.0 |
| `de_DE.*` | German (Germany) | igerman98 | 20161207 | GPL-2.0-only and GPL-3.0-only |
| `en_GB-large.*` | British English | SCOWL English dictionaries | 2026.02.25 | See `licenses/English-COPYRIGHT` |

The German and English files were taken without modification from the Arch
Linux packages `hunspell-de` and `hunspell-en_gb`. Full notices and license
texts are retained in `licenses/`.

The Swedish files are generated; do not edit them. They contain every entry of
Den stora svenska ordlistan 2.42 (Arch package `hunspell-sv`, kept unmodified
in `../sources/sv_SE-dsso.*`, which is not embedded) plus SALDO entries that add
lemmas (`st:`), parts of speech (`po:`), descriptor-confirmed compound part
lemmas (`cp:`) and SALDO's inflections. Regenerate with

    cargo run -p xtask -- build-swedish-dictionary <saldom.xml> <saldo20v03.txt>

using SALDO's morphology (`saldom.xml`, LMF, CC BY 4.0) and SALDO 2.3
(`saldo20v03.txt` from `saldo_2.3.tgz`) from
https://spraakbanken.gu.se/en/resources/saldo. See
`licenses/Swedish-SALDO-NOTICE.txt`.

Upstream locations:

- https://extensions.libreoffice.org/en/extensions/show/swedish-spelling-dictionary-den-stora-svenska-ordlistan
- https://www.j3e.de/ispell/igerman98/
- https://wordlist.aspell.net/dicts/
