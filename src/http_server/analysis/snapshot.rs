//! Interpret captured registers, stack bytes and verified ELF bytes in main.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`architecture`] | `pub(in crate::http_server)` | `mod architecture` |
//! | [`registers`] | `pub` | `mod registers` |
//! | [`stack`] | `pub` | `mod stack` |
//! | [`symbol`] | `pub` | `mod symbol` |
//! | [`unwind`] | `pub` | `mod unwind` |

pub(in crate::http_server) mod architecture;
pub mod registers;
pub mod stack;
pub mod symbol;
pub mod unwind;
