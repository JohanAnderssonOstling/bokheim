//! Browser SQLite VFS bootstrap. All higher-level behavior remains in the
//! regular backend; this module only installs durable browser file semantics.

use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder};

const VFS_NAME: &str = "bokheim-opfs-sahpool-v1";
const VFS_DIRECTORY: &str = ".bokheim-opfs-sahpool-v1";

pub async fn initialize() -> Result<(), String> {
    let config = OpfsSAHPoolCfgBuilder::new().vfs_name(VFS_NAME).directory(VFS_DIRECTORY).clear_on_init(false).initial_capacity(8).build();
    install::<rusqlite::ffi::WasmOsCallback>(&config, true).await.map(|_| ()).map_err(|error| error.to_string())
}
