//! Native handle adapters over the shared SQLite library backend.

pub use library_sqlite::database::{assets::*, audiobooks::*, books::*, browse::*, catalog::*, external_metadata::*, marks::*, metadata::*, placements::*, reading::*, remote_reducer::*, sync_state::*};
pub use library_sqlite::*;

use library_model::{AuthorId, EpubCfi, ReadProgress};
use library_model::{BookCardRow, Dir};
use sync_common::{ContentHash, DirId};

mod audiobooks;
mod browse;
mod marks;
mod reading;
