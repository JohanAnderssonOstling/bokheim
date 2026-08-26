//! Native handle adapters over the shared SQLite library backend.

pub use library_sqlite::database::{assets::*, audiobooks::*, books::*, browse::*, catalog::*, external_metadata::*, links::*, marks::*, metadata::*, placements::*, reading::*, remote_reducer::*, sync_state::*};
pub use library_sqlite::*;

use app_contract::library::{BookCardRow, Dir};
use library_model::{AuthorId, EpubCfi, ReadProgress};
use sync_common::{ContentHash, DirId};

mod audiobooks;
mod browse;
mod links;
mod marks;
mod reading;
