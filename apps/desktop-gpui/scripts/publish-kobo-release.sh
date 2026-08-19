#!/bin/sh
# Build the Kobo package on this machine and upload it to the shared draft
# release. The ARMv7 musl cross toolchain is only available from musl.cc, a
# single volunteer-run host that is regularly unreachable, so the packaging
# runs here against the toolchain already cached in GPUI-Fork instead of
# being fetched again by every CI run. The USB installer workflow downloads
# the package this script publishes, so run it before pushing the release tag.
#
#   apps/desktop-gpui/scripts/publish-kobo-release.sh            # upload
#   apps/desktop-gpui/scripts/publish-kobo-release.sh --dry-run  # verify only
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../../.." && pwd)
RELEASE_REPOSITORY=${BOKHEIM_RELEASE_REPOSITORY:-JohanAnderssonOstling/bokheim-release}

DRY_RUN=0
case "${1:-}" in
    --dry-run) DRY_RUN=1 ;;
    "") ;;
    *) printf 'Usage: %s [--dry-run]\n' "$0" >&2; exit 2 ;;
esac

cd "$REPO_ROOT"

# A locally built binary carries none of CI's provenance, so the commit it was
# built from must at least be clean and published: a released artifact nobody
# can trace back to a public commit is worse than a failed build.
if [ -n "$(git status --porcelain)" ]; then
    printf 'The working tree has uncommitted changes; commit or stash them first.\n' >&2
    git status --short >&2
    exit 1
fi
COMMIT=$(git rev-parse HEAD)
if [ -z "$(git branch --remotes --contains "$COMMIT" 2>/dev/null)" ]; then
    printf 'HEAD (%s) has not been pushed; push it so the release can be traced to it.\n' "$COMMIT" >&2
    exit 1
fi

# The cross build takes minutes, so everything the upload needs is checked
# before it starts: discovering a missing CLI or a read-only token after the
# package is built wastes the whole build.
check_upload_prerequisites() {
    if ! command -v gh >/dev/null 2>&1; then
        printf 'The GitHub CLI (gh) is not installed; it is required to upload the package.\n' >&2
        return 1
    fi
    if ! gh auth status >/dev/null 2>&1; then
        printf 'The GitHub CLI is not authenticated; run gh auth login.\n' >&2
        return 1
    fi
    if [ "$(gh api "repos/$RELEASE_REPOSITORY" --jq .permissions.push 2>/dev/null)" != true ]; then
        printf 'The authenticated account cannot write to %s.\n' "$RELEASE_REPOSITORY" >&2
        return 1
    fi
}

if [ "$DRY_RUN" = 1 ]; then
    # A dry run never uploads, so a missing CLI is worth reporting but is not
    # a reason to refuse to build and verify the package.
    check_upload_prerequisites || printf 'Continuing: the dry run does not upload.\n' >&2
else
    check_upload_prerequisites || exit 1
fi

VERSION=$(sed -n 's/^version = "\([^"]*\)"/\1/p' apps/desktop-gpui/Cargo.toml | head -n 1)
test -n "$VERSION"
TAG="v$VERSION"

printf 'Building the Kobo package for %s at %s\n' "$TAG" "$COMMIT"
KOBO_CPU_PROFILE=${KOBO_CPU_PROFILE:-libra2}
export KOBO_CPU_PROFILE
ARCHIVE=$("$SCRIPT_DIR/package-kobo.sh")
test -s "$ARCHIVE"

STAGING="$REPO_ROOT/release"
VERSIONED="$STAGING/Bokheim-v$VERSION-Kobo-Libra2-armv7.tar.gz"
# The installer builds embed the unversioned name, so both are published.
CURRENT="$STAGING/Bokheim-Kobo-Libra2-armv7.tar.gz"
mkdir -p "$STAGING"
cp "$ARCHIVE" "$VERSIONED"

# The same checks the CI job applied to its own output: a package missing the
# launcher or holding a host-architecture binary bricks nothing, but it wastes
# a release and a device round trip to discover.
listing=$(tar -tzf "$VERSIONED")
for required in \
    ./.adds/bokheim/desktop-gpui-kobo \
    ./.adds/bokheim/fbink \
    ./.adds/bokheim/run.sh \
    ./.adds/bokheim/run-monitored.sh \
    ./.adds/nm/desktop-gpui-kobo; do
    printf '%s\n' "$listing" | grep -Fxq "$required" || {
        printf 'The package is missing %s\n' "$required" >&2
        exit 1
    }
done
VERIFY_DIR=$(mktemp -d)
trap 'rm -rf "$VERIFY_DIR"' EXIT
tar -xzf "$VERSIONED" -C "$VERIFY_DIR" ./.adds/bokheim/desktop-gpui-kobo
file "$VERIFY_DIR/.adds/bokheim/desktop-gpui-kobo" | grep -Eq 'ELF 32-bit.*ARM' || {
    printf 'The packaged binary is not a 32-bit ARM executable:\n' >&2
    file "$VERIFY_DIR/.adds/bokheim/desktop-gpui-kobo" >&2
    exit 1
}

cp "$VERSIONED" "$CURRENT"
(
    cd "$STAGING"
    sha256sum "$(basename "$VERSIONED")" > "$(basename "$VERSIONED").sha256"
    sha256sum "$(basename "$CURRENT")" > "$(basename "$CURRENT").sha256"
)

printf 'Verified %s (%s)\n' "$VERSIONED" "$(cd "$STAGING" && cut -d ' ' -f 1 < "$(basename "$VERSIONED").sha256")"
if [ "$DRY_RUN" = 1 ]; then
    printf 'Dry run: nothing was uploaded.\n'
    exit 0
fi

if ! gh release view "$TAG" --repo "$RELEASE_REPOSITORY" >/dev/null 2>&1; then
    gh release create "$TAG" \
        --repo "$RELEASE_REPOSITORY" \
        --target main \
        --draft \
        --generate-notes \
        --title "Bokheim $VERSION" >/dev/null ||
        gh release view "$TAG" --repo "$RELEASE_REPOSITORY" >/dev/null
fi
gh release upload "$TAG" \
    "$VERSIONED" "$VERSIONED.sha256" "$CURRENT" "$CURRENT.sha256" \
    --repo "$RELEASE_REPOSITORY" \
    --clobber

printf 'Published the Kobo package to %s %s from %s\n' "$RELEASE_REPOSITORY" "$TAG" "$COMMIT"
printf 'Run the Kobo release workflow to build the USB installers against it.\n'
