//! Storage scanning for the Overview: what fills a drive, by category.
//!
//! Two scanners feed one aggregator: `walk` (a folder walk with the user's
//! rights) and `mft` (the NTFS master file table, read by the helper service
//! with admin rights: seconds instead of minutes). Results are cached per
//! drive.

pub mod aggregate;
pub mod cache;
pub mod classify;
pub mod client;
pub mod find;
pub mod ipc;
pub mod mft;
pub mod model;
pub mod walk;

pub use model::{Category, Cleanup, CleanupKind, Event, Item, Mode, Progress, ScanResult, Totals};
