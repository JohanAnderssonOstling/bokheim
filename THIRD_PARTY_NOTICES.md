# Third-Party Notices

Bokheim is built with open-source libraries. Their copyrights remain with
their respective authors, and their license terms continue to apply.

The mobile EPUB renderer includes the pinned `foliate-js` submodule, copyright
John Factotum and licensed under the MIT License. Its complete license is at
`MobileApp/src/vendor/foliate-js/LICENSE`.

Foliate.js includes or references the following bundled components:

- PDF.js, licensed under the Apache License 2.0.
- zip.js, licensed under the BSD 3-Clause License.
- fflate, licensed under the MIT License.
- Adobe CMap resources under the BSD-style terms included at
  `MobileApp/src/vendor/foliate-js/vendor/pdfjs/cmaps/LICENSE`.
- Foxit and Liberation standard-font resources under the license texts in
  `MobileApp/src/vendor/foliate-js/vendor/pdfjs/standard_fonts/`.

The Rust and JavaScript dependency manifests and lockfiles identify the exact
remaining dependency versions used by each release. Corresponding source and
license information is available from the upstream projects named in those
manifests.
