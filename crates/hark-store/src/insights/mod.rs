//! Local bounded insights. Call queries on the storage worker, never from an
//! egui paint pass. Numeric events contain no transcript content.

mod patterns;
mod persistence;
mod query;
mod types;

pub(crate) use persistence::{backfill, prune, record};
pub use types::*;
