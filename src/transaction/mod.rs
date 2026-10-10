//! What makes a mutating run restorable: the project lock and the backup
//! snapshot taken under it, the witness and manifest written after it, and the
//! signal traps armed before the first write.

pub mod backup;
pub mod interrupt;
pub mod lock;
pub mod manifest;
pub mod witness;
