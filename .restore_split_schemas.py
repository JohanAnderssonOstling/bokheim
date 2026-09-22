from pathlib import Path
import re
import subprocess

root = Path("client/library-database/src")
groups = {
    "browse": ["browse/browse.sql", "browse/folder_cache.sql", "browse/progress.sql", "browse/text.sql"],
    "sync": ["sync/sync_apply.sql", "sync/sync_state.sql"],
}

for module, sources in groups.items():
    blocks = []
    for source in sources:
        text = subprocess.check_output(["git", "show", f":client/library-database/src/{source}"], text=True)
        markers = list(re.finditer(r"(?m)^-- name: ([^ ]+)", text))
        for index, marker in enumerate(markers):
            if not marker.group(1).startswith("schema_"):
                continue
            end = markers[index + 1].start() if index + 1 < len(markers) else len(text)
            blocks.append(text[marker.start():end].rstrip())
    (root / module / "sql" / "schema.sql").write_text("\n\n".join(blocks) + "\n")

