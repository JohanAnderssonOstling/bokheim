/// The character used to mask password input fields.
pub(super) const MASK_CHAR: char = '•';

mod blink_cursor;
mod change;
mod clear_button;
mod content_type;
mod cursor;
mod decorations;
mod display_map;
mod element;
mod indent;
mod input;
mod mask_pattern;
mod mode;
mod movement;
#[cfg(target_os = "macos")]
mod native;
mod number_input;
mod otp_input;
mod position;
mod rope_ext;
#[cfg(feature = "input-search")]
mod search;
mod selection;
mod state;

pub(crate) use clear_button::*;
pub use content_type::*;
pub use cursor::*;
pub use decorations::*;
pub use display_map::{BufferPoint, DisplayMap, DisplayPoint, FoldRange};
pub use indent::TabSize;
pub use input::*;
pub use mask_pattern::MaskPattern;
pub use number_input::{NumberInput, NumberInputEvent, NumberStep, StepAction};
pub use otp_input::*;
pub use position::Position;
pub use rope_ext::{Point, RopeExt, RopeLines};
pub use ropey::Rope;
pub use state::*;
