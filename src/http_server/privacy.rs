//! Privacy mode: mask sensitive text before a reply leaves this process.
//!
//! # Interface
//! - [`enable`] (`pub(super)`): `fn enable(level: u8) -> anyhow::Result<()>`.
//! - [`enabled`] (`pub(super)`): `fn enabled() -> bool`.
//! - [`scrub_response`] (`pub(super)`): `fn scrub_response(response: &mut MonitorResponse)`.
//! - [`pseudonym`] (`pub(super)`): `fn pseudonym(value: &str) -> String`.
//!
//! The privileged helper does not read environments or process memory in this
//! mode. Everything else it returns passes through [`scrub_response`]:
//! identifying fields are replaced whole, and other strings are scanned by
//! logblur, with this machine's host name, IDs and login users preloaded. A masked
//! value becomes `░` + 8 hex digits + `░`, a keyed hash that is consistent
//! within this run, so distinct addresses stay distinct, but cannot be linked
//! across runs or hosts. The original text is never sent.
use super::protocol::{MonitorEvent, MonitorResponse};
use anyhow::{Context, Result};
use logblur_core::{Category, Engine, JobConfig, scan::Found};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Read,
    sync::{Mutex, OnceLock},
};

/// Fields that identify a machine or peer whatever their text looks like.
const WHOLE_FIELDS: &[&str] = &["remote_hostname", "gid", "remote_gid", "uuid", "serial"];
/// Cached results; snapshots repeat most strings, so the cache is cleared rather than evicted.
const CACHE_LIMIT: usize = 200_000;

struct Privacy {
    engine: Engine,
    key: [u8; 32],
    cache: Mutex<HashMap<String, String>>,
}

static PRIVACY: OnceLock<Privacy> = OnceLock::new();

/// Users that log in (UID 1000 and above, except `nobody`) are personal.
fn login_users(passwd: &str) -> Vec<String> {
    passwd
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let uid: u32 = fields.nth(1)?.parse().ok()?;
            (uid >= 1000 && uid != 65534 && !name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

/// This machine's IDs, which programs such as dbus-broker pass in arguments.
fn machine_ids() -> Vec<String> {
    ["/etc/machine-id", "/proc/sys/kernel/random/boot_id"]
        .into_iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect()
}

fn host_names() -> Vec<String> {
    let Ok(name) = std::fs::read_to_string("/proc/sys/kernel/hostname") else {
        return Vec::new();
    };
    let name = name.trim();
    let mut names = vec![name.to_owned()];
    if let Some((short, _)) = name.split_once('.') {
        names.push(short.to_owned());
    }
    names
}

impl Privacy {
    fn new(level: u8, learned: Vec<(Category, String)>, key: [u8; 32]) -> Result<Self> {
        let config = JobConfig {
            level,
            ..JobConfig::default()
        };
        config.validate()?;
        let mut engine = Engine::new(&config)?;
        engine.set_learned(learned)?;
        Ok(Self {
            engine,
            key,
            cache: Mutex::new(HashMap::new()),
        })
    }

    fn pseudonym(&self, value: &[u8]) -> String {
        let digest = Sha256::new()
            .chain_update(self.key)
            .chain_update(value)
            .finalize();
        let hex: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
        format!("░{hex}░")
    }

    /// Scans `items` as one space-separated text, so `-p` and its password in
    /// separate arguments are seen together, and masks each item's share of a hit.
    fn scan(&self, items: &[&str]) -> Vec<String> {
        // Arguments cannot contain NUL, and pseudonyms never do.
        let key = items.join("\0");
        if let Some(masked) = self.cache.lock().unwrap().get(&key) {
            return masked.split('\0').map(str::to_owned).collect();
        }
        let text = items.join(" ");
        let bytes = text.as_bytes();
        let mut found: Vec<Found> = Vec::new();
        self.engine.find(bytes, &mut found);
        let mut masked = Vec::with_capacity(bytes.len());
        let mut start = 0;
        for (index, item) in items.iter().enumerate() {
            if index > 0 {
                masked.push(0);
            }
            let end = start + item.len();
            let mut done = start;
            for hit in found
                .iter()
                .filter(|hit| hit.start < end && hit.end > start)
            {
                let (from, to) = (hit.start.max(start), hit.end.min(end));
                masked.extend_from_slice(&bytes[done..from]);
                masked.extend_from_slice(self.pseudonym(&bytes[from..to]).as_bytes());
                done = to;
            }
            masked.extend_from_slice(&bytes[done..end]);
            start = end + 1;
        }
        let masked = String::from_utf8_lossy(&masked).into_owned();
        let mut cache = self.cache.lock().unwrap();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, masked.clone());
        masked.split('\0').map(str::to_owned).collect()
    }

    fn scrub(&self, value: &mut Value, field: Option<&str>) {
        match value {
            Value::String(text) if text.is_empty() => {}
            Value::String(text) => {
                let whole = match field {
                    Some("ip") => text
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| !ip.is_loopback() && !ip.is_unspecified()),
                    Some(field) => WHOLE_FIELDS.contains(&field),
                    None => false,
                };
                *text = if whole {
                    self.pseudonym(text.as_bytes())
                } else {
                    self.scan(&[text]).remove(0)
                };
            }
            Value::Array(items)
                if items.len() > 1
                    && !field.is_some_and(|field| WHOLE_FIELDS.contains(&field))
                    && items.iter().all(Value::is_string) =>
            {
                let texts: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
                let masked = self.scan(&texts);
                for (item, masked) in items.iter_mut().zip(masked) {
                    *item = Value::String(masked);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.scrub(item, field);
                }
            }
            Value::Object(fields) => {
                for (name, item) in fields {
                    self.scrub(item, Some(name));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
}

/// Turns privacy mode on for this process. `level` is logblur's 1–4.
pub(super) fn enable(level: u8) -> Result<()> {
    let mut key = [0; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| random.read_exact(&mut key))
        .context("read a random privacy key")?;
    let mut learned: Vec<_> = host_names()
        .into_iter()
        .map(|name| (Category::Hostname, name))
        .chain(
            machine_ids()
                .into_iter()
                .map(|id| (Category::MachineId, id)),
        )
        .collect();
    if let Ok(passwd) = std::fs::read_to_string("/etc/passwd") {
        learned.extend(
            login_users(&passwd)
                .into_iter()
                .map(|name| (Category::Username, name)),
        );
    }
    let privacy = Privacy::new(level, learned, key).context("start privacy mode")?;
    let _ = PRIVACY.set(privacy);
    Ok(())
}

pub(super) fn enabled() -> bool {
    PRIVACY.get().is_some()
}

/// The masked form of a whole value, such as the machine ID a host advertises.
pub(super) fn pseudonym(value: &str) -> String {
    match PRIVACY.get() {
        Some(privacy) => privacy.pseudonym(value.as_bytes()),
        None => value.to_owned(),
    }
}

/// Masks every string in a reply when privacy mode is on.
pub(super) fn scrub_response(response: &mut MonitorResponse) {
    let Some(privacy) = PRIVACY.get() else {
        return;
    };
    let event = match response {
        MonitorResponse::Data(value) => return privacy.scrub(value, None),
        MonitorResponse::Started { initial, .. } => initial,
        MonitorResponse::Event {
            event: Some(event), ..
        } => event,
        MonitorResponse::Event { event: None, .. } | MonitorResponse::Closed => return,
    };
    match event {
        MonitorEvent::Observation(value)
        | MonitorEvent::Snapshot(value)
        | MonitorEvent::Activity(value)
        | MonitorEvent::Gap {
            snapshot: value, ..
        } => privacy.scrub(value, None),
        // Converted to JSON by the observation processor before this point.
        MonitorEvent::AcquiredSnapshot(_) | MonitorEvent::AcquisitionGap { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn privacy() -> Privacy {
        Privacy::new(
            3,
            vec![
                (Category::Hostname, "workstation-7".into()),
                (Category::Username, "alice".into()),
            ],
            [7; 32],
        )
        .unwrap()
    }

    fn masked(text: &str) -> bool {
        text.contains('░')
    }

    #[test]
    fn login_users_skip_system_accounts_and_nobody() {
        let passwd = "root:x:0:0::/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/sh\n\
                      nobody:x:65534:65534::/:/sbin/nologin\nbob:x:1001:1001::/home/bob:/bin/sh\n";
        assert_eq!(login_users(passwd), ["alice", "bob"]);
    }

    #[test]
    fn secrets_users_and_hosts_are_masked_in_any_string() {
        let privacy = privacy();
        let mut value = json!({
            "command_line": ["sshpass", "-p", "hunter2secret", "ssh", "alice@workstation-7"],
            "path": "/home/alice/.ssh/id_ed25519",
            "comm": "sshd-session: alice@pts/4",
            "username": "alice",
            "uid": 1000,
        });
        privacy.scrub(&mut value, None);
        let text = value.to_string();
        for secret in ["hunter2secret", "alice", "workstation-7"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert_eq!(value["uid"], 1000);
        assert!(masked(value["username"].as_str().unwrap()));
        assert_eq!(value["command_line"][0], "sshpass");
    }

    #[test]
    fn identifying_fields_are_masked_whole_and_consistently() {
        let privacy = privacy();
        let mut value = json!([
            {"remote_hostname": "nas", "remote": {"ip": "192.168.2.161", "port": 22}},
            {"remote_hostname": "nas", "remote": {"ip": "192.168.2.162", "port": 22}},
            {"local": {"ip": "127.0.0.1", "port": 8080}, "gid": "fe80::2:c903:10:1234"},
        ]);
        privacy.scrub(&mut value, None);
        assert!(masked(value[0]["remote_hostname"].as_str().unwrap()));
        assert_eq!(value[0]["remote_hostname"], value[1]["remote_hostname"]);
        assert_ne!(value[0]["remote"]["ip"], value[1]["remote"]["ip"]);
        assert_eq!(value[0]["remote"]["port"], 22);
        assert_eq!(value[2]["local"]["ip"], "127.0.0.1");
        assert!(masked(value[2]["gid"].as_str().unwrap()));
    }

    #[test]
    fn ordinary_text_is_unchanged_and_keys_differ_per_run() {
        let privacy = privacy();
        let mut value =
            json!({"comm": "cargo", "command_line": ["make", "-j", "16"], "text": "mov rax, 0x10"});
        let before = value.clone();
        privacy.scrub(&mut value, None);
        assert_eq!(value, before);
        let other = Privacy::new(3, Vec::new(), [8; 32]).unwrap();
        assert_ne!(privacy.pseudonym(b"10.0.0.1"), other.pseudonym(b"10.0.0.1"));
        assert_eq!(
            privacy.pseudonym(b"10.0.0.1"),
            privacy.pseudonym(b"10.0.0.1")
        );
    }
}
