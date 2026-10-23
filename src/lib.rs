//! prdiff: what actually changes when `head` lands on `base`, judged by content rather than commit subjects.

pub mod analyze;
pub mod cache;
pub mod check;
pub mod classify;
pub mod commits;
pub mod conflicts;
pub mod error;
pub mod git;
pub mod glob;
pub mod hunks;
pub mod net;
pub mod patchid;
pub mod rawdiff;
pub mod refs;
pub mod render;
pub mod report;
