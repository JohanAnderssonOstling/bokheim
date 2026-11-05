# Kobo packages

Shared by `client/updates/kobo` and the standalone `apps/kobo-installer` utility.
This crate owns GitHub release parsing, release limits, artifact metadata,
archive paths, protected user data, required files, duplicate detection, and
destination checks. Its archive visitor is shared by validation, USB installation,
and on-device staging. HTTP transport and installation orchestration stay with
each host; durable file writes use `shared/update-files`.

The USB policy accepts older installation packages, limited to Bokheim's own
directories and its menu entry so packages cannot replace other applications
or shared NickelMenu configurations. The on-device policy limits
replacement to program files and additionally requires the recovery helper and
build information. Both policies preserve libraries, preferences, and updater
state. Release serialization preserves the on-device `ready.json` format.

Both callers verify downloaded artifacts through `update-protocol`; on-device
downloads and native signed-manifest updates use its shared `stage_verified`
function to publish only complete, verified files. Kobo release discovery continues to use
GitHub release metadata; it does not consume the signed native update feed.
