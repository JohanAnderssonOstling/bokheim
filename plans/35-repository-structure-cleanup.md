# Repository structure cleanup

Status: ⬜ open

## Objective

Make the repository layout communicate architectural ownership at a glance:

- deployable clients live under `apps/`;
- independently deployed server processes live under `servers/`;
- reusable application and domain libraries live under `crates/`;
- rendering subsystems live under `renderers/`;
- reusable interface code lives under `ui/`;
- development utilities live under `tools/`;
- plans and other long-lived documentation live under `docs/`;
- local runtime state is isolated under an ignored `var/` directory.

This is a path and ownership cleanup. It must not change runtime behavior or
crate APIs merely to justify the move.

## Architecture decisions carried into the layout

Preferences are device-local state, not synchronized account state. Their
shared values, validation, and UI-facing patch types belong in
`crates/app-contract`; local persistence and update coordination belong in
`crates/backend`; client UIs reach them through the local backend boundary.
The sync server must not own preference routes, storage, notifications, or
database tables.

## Prerequisite

Checkpoint the current working tree before starting. The repository currently
contains an in-flight cross-cutting refactor, including new untracked crates and
large renderer test imports. Mixing those changes with mass path moves would
make review and recovery unnecessarily difficult.

The `HtmlRenderer` move is additionally deferred until its current crate split is
complete, or until its in-flight state has otherwise been committed as a stable
checkpoint.

Do not use a broad `git clean`, and do not delete local books, databases,
thumbnails, test corpora, environment files, or other user state as part of
this plan.

## Target layout

```text
apps/
  desktop-floem/
  desktop-gpui/
  mobile/
  site/

servers/
  catalog/
  sync/
  web/

crates/
  account-contract/
  algorithms/
  app-contract/
  audiobook-library/
  backend/
  catalog-provider/
  domain/
  identifiers/
  library-files/
  opds/
  projection/
  storage-sqlite/
  sync-client/
  sync-common/
  thumbnail/
  wire/

renderers/
  html/
  pdf/
    core/
    view-common/
  shared/
    reader-style/

ui/
  design-tokens/
  gpui/
    components/
    runtime/
    browser/
    pdf-view/
    reader/

tools/
  data-analysis/
  pdf-margin-trim/

docs/
  plans/

testdata/              # small, shareable fixtures
var/                   # ignored local state and large local fixtures
scripts/               # repository-wide automation remains at the root
```

Root-level policy and build files remain at the root: `Cargo.toml`,
`Cargo.lock`, `README.md`, `LICENSE`, `PRIVACY.md`, `RELEASING.md`,
`THIRD_PARTY_NOTICES.md`, toolchain configuration, `.github/`, and agent/editor
configuration.

## Current-to-target mapping

| Current path | Target path | Target Cargo package / Rust crate |
|---|---|
| `MobileApp` | `apps/mobile` | `mobileapp` / `mobileapp_lib` |
| `GPUI/desktop-gpui` | `apps/desktop-gpui` | `desktop-gpui` / `desktop_gpui` |
| `GPUI/audiobook-player` | `apps/audiobook-player` | `audiobook-player` / `audiobook_player` |
| `GPUI/kobo-device` | `apps/kobo-device` | `kobo-device` / `kobo_device` |
| `UI` | `apps/desktop-floem` | retain its established package name |
| `site` | `apps/site` | not a Rust crate |
| `Server` | `servers/sync` | `sync-server` / `sync_server` |
| `WebServer` | `servers/web` | retain its established package name |
| `Catalogs` | `servers/catalog` | `catalog-server` / `catalog_server` |
| `Backend` | `crates/backend` | `epub-svelte` / `epub_svelte_lib` |
| `BokheimAccountContract` | `crates/account-contract` | `account-contract` / `account_contract` |
| `BokheimAlgorithms` | `crates/algorithms` | `algorithms` / `algorithms` |
| `BokheimAppContract` | `crates/app-contract` | `app-contract` / `app_contract` |
| `BokheimCatalogProvider` | `crates/catalog-provider` | `catalog-provider` / `catalog_provider` |
| `BokheimDomain` | `crates/domain` | `domain` / `domain` |
| `BokheimIdentifiers` | `crates/identifiers` | `identifiers` / `identifiers` |
| `BokheimLibraryFiles` | `crates/library-files` | `library-files` / `library_files` |
| `BokheimOpds` | `crates/opds` | `opds` / `opds` |
| `BokheimSyncClient` | `crates/sync-client` | `sync-client` / `sync_client` |
| `BokheimWire` | `crates/wire` | `wire` / `wire` |
| `SyncCommon` | `crates/sync-common` | `sync-common` / `sync_common` |
| `AudiobookLibrary` | `crates/audiobook-library` | `audiobook-library` / `audiobook_library` |
| `Backend/crates/book-metadata` | `crates/book-metadata` | `book-metadata` / `book_metadata` |
| `Backend/crates/library-registry` | `crates/library-registry` | `library-registry` / `library_registry` |
| `Backend/crates/projection` | `crates/projection` | `projection` / `projection` |
| `Backend/crates/remote-reader` | `crates/remote-reader` | `remote-reader` / `remote_reader` |
| `Backend/crates/storage-sqlite` | `crates/storage-sqlite` | `storage-sqlite` / `storage_sqlite` |
| `Backend/crates/thumbnail` | `crates/thumbnail` | `thumbnail` / `thumbnail` |
| `HtmlRenderer` | `renderers/html` | `html-renderer` / `html_renderer` |
| `PdfRenderer` | `renderers/pdf/core` | `pdf-reader-core` / `pdf_reader_core` |
| `PdfViewCommon` | `renderers/pdf/view-common` | `pdf-view-common` / `pdf_view_common` |
| `ReaderStyle` | `renderers/shared/reader-style` | `reader-style` / `reader_style` |
| `GPUI/ui-components` | `ui/gpui/components` | `ui-components` / `ui_components` |
| `GPUI/ui-runtime` | `ui/gpui/runtime` | `ui-runtime` / `ui_runtime` |
| `GPUI/browser-ui` | `ui/gpui/browser` | `browser-ui` / `browser_ui` |
| `GPUI/pdf-view-gpui` | `ui/gpui/pdf-view` | `pdf-view-gpui` / `pdf_view_gpui` |
| `GPUI/reader-ui` | `ui/gpui/reader` | `reader-ui` / `reader_ui` |
| `data-analysis` | `tools/data-analysis` | `data-analysis` / `data_analysis` |
| `PdfMarginTrim` | `tools/pdf-margin-trim` | retain its established package name |
| `plans` | `docs/plans` | not a Rust crate |

`Bokheim` is not part of the target directory, Cargo package, or Rust crate
names. The rename is intentional and must be performed as a coordinated API
migration: update every Cargo dependency key/package override, Rust `use`,
`extern crate`, binary name, script, CI entry, documentation link, and generated
binding reference in the same phase. Do not retain compatibility aliases with
the old `bokheim_*` crate names after the migration.

Local runtime paths such as `books/`, `thumbnails/`, `tmp/`, `web-data/`, local
SQLite files, and large private test books move beneath `var/`. Checked-in test
fixtures remain under lowercase `testdata/`. Code must obtain runtime locations
from configuration rather than assuming the repository root.

## Migration phases

Each phase gets its own commit. A phase must leave the repository buildable and
must not begin until the previous phase passes its validation.

### 1. Establish invariants and inventory references

1. Record the package set and manifest paths from `cargo metadata --no-deps`.
2. Search tracked files for every path in the mapping table.
3. Identify CI, release, deployment, Android, shell, and documentation paths
   that Cargo metadata cannot discover.
4. Confirm which root data directories are generated, checked-in fixtures, or
   user-owned state.
5. Add temporary architecture checks if needed to prevent new root-level crates
   during the migration.

### 2. Move reusable core crates

Move the standalone reusable crates and `Backend` into `crates/`. Rename every
`Bokheim*` directory and every `bokheim-*` Cargo package / `bokheim_*` Rust
crate according to the mapping table. Update:

- root workspace members;
- all Cargo path dependencies;
- architecture tests;
- build and release scripts;
- documentation links and examples.

Treat each package/crate rename as an API migration within this repository:
update Cargo dependency declarations and Rust imports together, then validate
the root workspace before continuing.

### 3. Move servers

Move the three independently deployable processes into `servers/`. Preserve the
sync server as an independent Cargo workspace with its own lockfile. Update its
relative dependencies, migrations, deployment files, systemd/Caddy examples,
backup scripts, CI paths, and documentation.

The directory name `servers/` is intentional: these entries are deployable
server programs, not generic in-process services.

### 4. Move applications and reusable UI crates

Move mobile and desktop entry points into `apps/`, and move reusable GPUI crates
into `ui/gpui/`. Update Tauri, Vite, Svelte, Android/Gradle, asset-generation,
desktop packaging, and workspace paths. Preserve the existing mobile vendor
submodule relationship.

Move the static site only after confirming its deployment workflow and asset
references.

### 5. Move rendering subsystems

After an equivalent stable renderer checkpoint, move the HTML renderer as one
subsystem. Move the PDF and reader-style crates into their target locations.
Update WPT tooling, benchmark/profile scripts, smoke applications, test-support
paths, documentation, and UI/backend dependencies.

Do not combine internal renderer API refactoring with this path-only phase.

### 6. Move tools and documentation

Move development-only programs beneath `tools/` and merge `plans/` into
`docs/plans/`. Repair relative links in the plan index and README files. Keep
repository-wide automation in root `scripts/`; subsystem-specific scripts stay
with their subsystem.

Do not keep empty placeholder directories. If `PdfMarginTrim` has no maintained
source or documented purpose at migration time, remove the empty directory
rather than moving it.

### 7. Isolate local state

Introduce ignored `var/` subdirectories for local application state and large
private fixtures. Update defaults and examples without moving or deleting user
data automatically. Provide an explicit, recoverable migration command or
documentation for developers who want to relocate existing local state.

Keep secrets and machine-local environment files ignored. Do not weaken the
existing ignore rules for books, databases, signing material, or test corpora.

### 8. Normalize documentation and guard the layout

1. Replace the root README layout section with the final directory tree and
   ownership rules.
2. Document why `servers/sync` is an excluded nested Cargo workspace.
3. Add a lightweight check that every maintained Cargo manifest is either a
   declared root member or belongs to the explicitly excluded sync-server
   workspace.
4. Remove obsolete path aliases and compatibility shims after all consumers use
   the new paths.

## Validation

Run after each applicable phase:

```sh
cargo metadata --no-deps --format-version 1
cargo check --workspace
cargo test -p epub-svelte --test architecture
cargo test --manifest-path servers/sync/Cargo.toml --test architecture
```

Run the mobile checks from its new directory after the application phase:

```sh
npm ci
npm run check
npm run test:cfi
npm run test:catalog
npm run test:semantic-links
npm run test:browsing-contract
npm run test:account-contract
```

Also run subsystem-specific checks documented in the moved README and plan
files. Before completing the plan:

- compare the before/after Cargo package-name sets;
- verify both Cargo lockfiles are still intentional and reproducible;
- verify Git recognizes path changes as moves where practical;
- search for stale references to every old top-level path;
- verify CI, deployment, release, backup, profiling, and Android scripts;
- verify no source files or user data were lost during moves;
- confirm the repository root contains no unexplained product directories.

## Completion criteria

- The target ownership groups exist and all maintained code is assigned to one.
- No reusable Bokheim crate remains as a PascalCase root directory.
- No maintained Cargo package or Rust crate name retains the `bokheim-` or
  `bokheim_` prefix.
- Deployable server programs live under `servers/` and client applications under
  `apps/`.
- Cargo metadata reports the same intended package set as before the migration.
- Root and sync-server builds and architecture tests pass.
- Mobile validation passes from its new location.
- Documentation, CI, scripts, deployment files, and submodule paths contain no
  stale references.
- Runtime state is ignored and separated from source without deleting or
  silently relocating user data.
- The final migration is represented by small, reviewable phase commits rather
  than one repository-wide move mixed with behavior changes.
