//! Interpret captured registers, stack bytes and verified ELF bytes in main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`architecture`] | `pub(in crate::http_server)` | `mod architecture` |
//! | [`registers`] | `pub(crate)` | `mod registers` |
//! | [`stack`] | `pub(crate)` | `mod stack` |
//! | [`symbol`] | `pub(crate)` | `mod symbol` |
//! | [`unwind`] | `pub(crate)` | `mod unwind` |

pub(in crate::http_server) mod architecture;
pub(crate) mod registers;
pub(crate) mod stack;
pub(crate) mod symbol;
pub(crate) mod unwind;
