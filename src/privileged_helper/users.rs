//! Read the UID-to-user-name mapping from the system account file.
//!
//! # Interface
//! - [`read_user_names`] (`pub(super)`): `fn read_user_names() -> `[`HashMap`]`<u32, `[`String`]`>`.

use std::{collections::HashMap, fs};

pub(super) fn read_user_names() -> HashMap<u32, String> {
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
