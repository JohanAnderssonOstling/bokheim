# Coding preferences

- In Rust, prefer `use` imports for repeated types, traits, and functions rather than repeatedly writing fully qualified `::` paths inline. Keep qualified paths where they clarify ambiguous names or platform-specific code, and follow the surrounding code style.
- Do not add code used only for tests.
- Do not run tests unless the user explicitly asks for them. Requests to implement, fix, optimize, or validate a change do not authorize tests; use non-test checks when appropriate.

# Build storage

- Never fall back to SSHFS network mounts for source checkouts, build caches, compiler targets, temporary build files, or build outputs.
- If local storage is insufficient, report the constraint instead of relocating builds to SSHFS.

# Rust compilation

- Run only one Rust compilation at a time. Before starting a Rust build, check, or test compilation, check for an existing Rust compilation and wait for it to finish. Do not start another compilation while one is running, even with a different target directory.

# Linux compatibility

- Linux desktop and USB installer releases must target glibc 2.17, including every bundled executable and shared library. Keep the compatibility checks at 2.17; fix incompatible dependencies instead of raising the baseline.
