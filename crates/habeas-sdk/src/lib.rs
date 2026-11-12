//! Shared source machinery for evidence tooling.
//!
//! Consumers provide configuration and assessment semantics. This initial
//! extraction preserves Habeas's source profiles and access conventions;
//! graph and harness APIs will be introduced separately.

pub mod access;
pub mod outline;
pub mod redaction;
pub mod search;
pub mod structural;
