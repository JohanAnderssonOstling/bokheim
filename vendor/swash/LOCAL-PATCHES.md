# Local font-stack patch

Upstream: swash 0.2.10, https://github.com/dfrg/swash.
Licenses: Apache-2.0 OR MIT; both license files are included.

- Inherit Skrifa 0.46.2 from the application workspace instead of the upstream version range capped at 0.44. This shares Skrifa and read-fonts with Cosmic Text and Formulary.
- Keep upstream production Rust sources and feature flags. Rendering, scaling, and hinting call the shared Skrifa version.
- `Cargo.toml.orig` retains the original upstream manifest for reference.
