# Deletion and recovery

Removing a placement keeps the book live while another placement remains.
Removing the last placement moves the book to Trash and preserves its metadata,
reading state, and last folder. Explicitly trashing a book removes all of its
current placements.

Native scanner deletions require a readable scan and the original storage
identity marker before and after the scan. Missing or mismatched root/folder
markers, unreadable entries, unsupported symbolic links, and interrupted hash
work prevent deletion inference. The existing two-generation grace window stays
in place. Only previously observed physical files are candidates; virtual and
never-materialized placements are not deleted by scanning.

Explicit native deletion commits durable file work with the metadata, then moves
matching files into `.bokheim/trash/<content hash>/`. The final placement also
archives the cached publication. Files with unexpected content are preserved.
Queued operations replay during local maintenance, including while signed out.
WASM retains its one hash-addressed object while the book is in Trash; it does not
create physical folder copies.

Restore chooses the last folder or its nearest available ancestor, falling back
to the library root. It restores from retained local bytes or leaves a durable
download request until remote bytes arrive. A destination occupied by another
file receives a unique filename. A queued restore is discarded if the book or
placement has since been removed again. Purging a book reclaims its managed
trash bytes as well as cached assets.

External deletion cannot recover bytes that neither this device nor the server
retains. The metadata remains recoverable independently.

## Native placement and save location

Native imports and completed downloads publish verified bytes at every live
folder placement. Copy and move operations use the same durable file work;
folder moves preserve the folder's other files. Local path snapshots distinguish
metadata moves from missing files that the scanner must observe. Existing user
files and folders receive no overwrites: conflicting incoming placements receive
unique names. Publication failures remain queued for maintenance and restart.

Desktop libraries default to `Documents/Bokheim/<library name>/`. Settings →
Library storage changes the root for future created and downloaded libraries.
Existing registered libraries retain their roots, and adding a directory keeps
its original placement. Browser folders remain metadata only, with one shared
hash-addressed publication object. Native managed hash storage remains a cache
for verified publication and recovery.
