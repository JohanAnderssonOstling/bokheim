/// Opaque MARC relator code carried by persistence and synchronization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct MarcRelatorCode(pub [u8; 3]);
