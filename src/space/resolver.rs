//! Bounded background reverse lookup: topology collection never waits for DNS.
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
struct Entry {
    name: Option<String>,
    expires: Instant,
    pending: bool,
}
pub struct Resolver {
    cache: Arc<Mutex<HashMap<IpAddr, Entry>>>,
    tx: mpsc::SyncSender<IpAddr>,
}
impl Resolver {
    pub fn new() -> Self {
        let cache = Arc::new(Mutex::new(HashMap::<IpAddr, Entry>::new()));
        let (tx, rx) = mpsc::sync_channel::<IpAddr>(4096);
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..4 {
            let (cache, rx) = (cache.clone(), rx.clone());
            std::thread::spawn(move || {
                loop {
                    let ip = match rx.lock().unwrap().recv() {
                        Ok(ip) => ip,
                        Err(_) => break,
                    };
                    let name = reverse(ip);
                    let ttl = if name.is_some() { 300 } else { 60 };
                    cache.lock().unwrap().insert(
                        ip,
                        Entry {
                            name,
                            expires: Instant::now() + Duration::from_secs(ttl),
                            pending: false,
                        },
                    );
                }
            });
        }
        Self { cache, tx }
    }
    pub fn lookup(&self, ip: IpAddr) -> Option<String> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(entry) = cache.get(&ip)
            && (entry.pending || entry.expires > Instant::now())
        {
            return entry.name.clone();
        }
        cache.retain(|_, e| e.pending || e.expires > Instant::now());
        if cache.len() >= 4096 {
            return None;
        }
        if self.tx.try_send(ip).is_ok() {
            cache.insert(
                ip,
                Entry {
                    name: None,
                    expires: Instant::now(),
                    pending: true,
                },
            );
        }
        None
    }
}
fn reverse(ip: IpAddr) -> Option<String> {
    let mut host = [0i8; 1025];
    let addr = SocketAddr::new(ip, 0);
    let result = match addr {
        SocketAddr::V4(a) => {
            let mut sa: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sa.sin_family = libc::AF_INET as _;
            sa.sin_addr.s_addr = u32::from_ne_bytes(a.ip().octets());
            unsafe {
                libc::getnameinfo(
                    (&sa as *const libc::sockaddr_in).cast(),
                    std::mem::size_of_val(&sa) as _,
                    host.as_mut_ptr(),
                    host.len() as _,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
        SocketAddr::V6(a) => {
            let mut sa: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sa.sin6_family = libc::AF_INET6 as _;
            sa.sin6_addr.s6_addr = a.ip().octets();
            unsafe {
                libc::getnameinfo(
                    (&sa as *const libc::sockaddr_in6).cast(),
                    std::mem::size_of_val(&sa) as _,
                    host.as_mut_ptr(),
                    host.len() as _,
                    std::ptr::null_mut(),
                    0,
                    libc::NI_NAMEREQD,
                )
            }
        }
    };
    if result != 0 {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr(host.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    (!name.is_empty() && name.parse::<IpAddr>().is_err()).then_some(name)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queues_once_and_retries_expired_negative_entries() {
        let (tx, rx) = mpsc::sync_channel(4096);
        let r = Resolver {
            cache: Arc::new(Mutex::new(HashMap::new())),
            tx,
        };
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(r.lookup(ip), None);
        assert_eq!(rx.try_recv().unwrap(), ip);
        assert_eq!(r.lookup(ip), None);
        assert!(rx.try_recv().is_err());
        r.cache.lock().unwrap().insert(
            ip,
            Entry {
                name: None,
                expires: Instant::now() - Duration::from_secs(1),
                pending: false,
            },
        );
        assert_eq!(r.lookup(ip), None);
        assert_eq!(rx.try_recv().unwrap(), ip);
        for i in 0..4096u32 {
            r.cache.lock().unwrap().insert(
                IpAddr::V4(i.into()),
                Entry {
                    name: None,
                    expires: Instant::now() + Duration::from_secs(60),
                    pending: false,
                },
            );
        }
        assert_eq!(r.lookup("192.0.2.1".parse().unwrap()), None);
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn cached_positive_negative_and_pending() {
        let r = Resolver::new();
        for (ip, name, pending) in [
            ("192.0.2.1", Some("example.test".to_string()), false),
            ("2001:db8::1", None, false),
            ("192.0.2.2", None, true),
        ] {
            let ip = ip.parse().unwrap();
            r.cache.lock().unwrap().insert(
                ip,
                Entry {
                    name: name.clone(),
                    expires: Instant::now() + Duration::from_secs(60),
                    pending,
                },
            );
            assert_eq!(r.lookup(ip), name);
        }
    }
}
