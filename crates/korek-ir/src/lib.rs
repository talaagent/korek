//! korek's intermediate representation ([`ir`]) and plugin protocol
//! ([`protocol`]) types: pure serde data with minimal dependencies, so
//! plugins compile fast. Helper methods live in a separate module.

extern crate alloc;

pub mod codes;
mod impls;
pub mod ir;
pub mod naming;
pub mod protocol;
pub mod value;

pub use impls::{ext_as, format_number};
pub use ir::*;
pub use protocol::{Diagnostic, File, GenerateRequest, GenerateResponse, Label, Severity};

/// The plugin protocol version. Bumped on breaking changes to the request,
/// the response or the IR.
pub const PROTOCOL_VERSION: u32 = 2;
