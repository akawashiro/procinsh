//! Main-only JSON and query representations for IPC identities and acquisition values.

//!
//! # Interface
//! Installs serde implementations; no named types or functions are exported.
#[path = "json/acquisition.rs"]
mod acquisition;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "crate::ipc::schema::ProcessId", deny_unknown_fields)]
struct ProcessIdJson {
    pid: i32,
    start_time_ticks: u64,
}
impl serde::Serialize for crate::ipc::schema::ProcessId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessIdJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for crate::ipc::schema::ProcessId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ProcessIdJson::deserialize(d)
    }
}

#[path = "json/rdma.rs"]
mod rdma;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(remote = "crate::ipc::schema::ProcessResource")]
enum ProcessResourceJson {
    Observation,
    Threads,
    Maps,
    Environment,
    Auxv,
    Fds,
}
impl serde::Serialize for crate::ipc::schema::ProcessResource {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessResourceJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for crate::ipc::schema::ProcessResource {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ProcessResourceJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::schema::MonitorRequest", deny_unknown_fields)]
enum MonitorRequestJson {
    Processes,
    Process {
        id: crate::ipc::schema::ProcessId,
        resource: crate::ipc::schema::ProcessResource,
    },
    #[serde(skip)]
    MappedElf {
        id: crate::ipc::schema::ProcessId,
        mapping_start: u64,
        ranges: Vec<crate::ipc::elf::FileRange>,
    },
    ObserveProcess {
        id: crate::ipc::schema::ProcessId,
    },
    ObserveSystem,
    Poll {
        session: u64,
    },
    Close {
        session: u64,
    },
}
impl serde::Serialize for crate::ipc::schema::MonitorRequest {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MonitorRequestJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for crate::ipc::schema::MonitorRequest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        MonitorRequestJson::deserialize(d)
    }
}
