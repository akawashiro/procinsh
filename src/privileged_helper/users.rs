//! Read the UID-to-user-name mapping from the system account file.
//!
//! # Interface
//! - [`read_uid_to_username_map`] (`pub(super)`): `fn read_uid_to_username_map() -> `[`HashMap`]`<u32, `[`String`]`>`.

use std::{collections::HashMap, fs};

pub(super) fn read_uid_to_username_map() -> HashMap<u32, String> {
    {
        fs::read_to_string("/etc/passwd")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let p: Vec<_> = line.split(':').collect();
                Some((p.get(2)?.parse().ok()?, p.first()?.to_string()))
            })
            .collect()
    }
}
