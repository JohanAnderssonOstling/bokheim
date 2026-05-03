#[cfg(all(feature = "ui-own", feature = "ui-shadcn"))]
compile_error!("Features `ui-own` and `ui-shadcn` are mutually exclusive. Enable only one.");

#[cfg(not(any(feature = "ui-own", feature = "ui-shadcn")))]
compile_error!("No UI backend selected. Enable one of: `ui-own` or `ui-shadcn`.");

#[cfg_attr(feature = "ui-shadcn", path = "shadcn/buttons.rs")]
#[cfg_attr(not(feature = "ui-shadcn"), path = "own/buttons.rs")]
pub mod buttons;

#[cfg_attr(feature = "ui-shadcn", path = "shadcn/labels.rs")]
#[cfg_attr(not(feature = "ui-shadcn"), path = "own/labels.rs")]
pub mod labels;

#[cfg_attr(feature = "ui-shadcn", path = "shadcn/stacks.rs")]
#[cfg_attr(not(feature = "ui-shadcn"), path = "own/stacks.rs")]
pub mod stacks;

pub use buttons::*;
pub use labels::*;
pub use stacks::*;
