//! Data Transfer Objects for API responses
//!
//! DTOs are separate from database schema types to allow:
//! - Different field names (snake_case in DB, camelCase in API if needed)
//! - Computed fields
//! - Hiding internal fields
//! - Versioning API responses independently

mod common;
mod rexpump;
mod rexswap;
mod transfers;

pub use common::*;
pub use rexpump::*;
pub use rexswap::*;
pub use transfers::*;
