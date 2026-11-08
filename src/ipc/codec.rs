//! Native-endian binary values for the private, same-machine IPC connection.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`LimitExceeded`] | `pub(crate)` | `struct LimitExceeded` |
//! | [`Encoder`] | `pub(crate)` | `struct Encoder` |
//! | [`Decoder`] | `pub(crate)` | `struct Decoder` |
//! | [`Wire`] | `pub(crate)` | `trait Wire` |
//! | [`blob_encoder`] | `pub(super)` | `fn blob_encoder(state: &mut `[`Encoder`]`, bytes: &[u8]) -> `[`Result`]`<()>` |
//! | [`blob_decoder`] | `pub(super)` | `fn blob_decoder<'a>(state: &mut `[`Decoder`]`<'a>) -> `[`Result`]`<&'a [u8]>` |
//! | [`encode`] | `pub(crate)` | `fn encode<T: `[`Wire`]`>(value: &T, limit: usize) -> `[`Result`]`<`[`Vec`]`<u8>>` |
//! | [`decode`] | `pub(crate)` | `fn decode<T: `[`Wire`]`>(bytes: &[u8]) -> `[`Result`]`<T>` |
//!
//! Struct fields follow their declared wire order; enum variants have explicit
//! byte tags. Strings and collections have u32 lengths. Integers and floats use
//! their native byte representation. Rust padding, pointers and container layouts
//! are never copied. The peers run on the same machine and are built together.

use anyhow::{Result, bail, ensure};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct LimitExceeded;
impl std::fmt::Display for LimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("monitor response exceeds size limit")
    }
}
impl std::error::Error for LimitExceeded {}

pub(crate) struct Encoder {
    bytes: Vec<u8>,
    limit: usize,
}
pub(crate) struct Decoder<'a> {
    bytes: &'a [u8],
}
/// An owned value with an explicit wire representation independent of Rust layout.
pub(crate) trait Wire: Sized {
    fn write(&self, output: &mut Encoder) -> Result<()>;
    fn read(input: &mut Decoder<'_>) -> Result<Self>;
}
fn append_encoder(state: &mut Encoder, bytes: &[u8]) -> Result<()> {
    if bytes.len() > state.limit.saturating_sub(state.bytes.len()) {
        return Err(LimitExceeded.into());
    }
    state.bytes.extend_from_slice(bytes);
    Ok(())
}

fn length_encoder(state: &mut Encoder, length: usize) -> Result<()> {
    u32::try_from(length)
        .map_err(|_| LimitExceeded)?
        .write(state)
}

pub(super) fn blob_encoder(state: &mut Encoder, bytes: &[u8]) -> Result<()> {
    length_encoder(state, bytes.len())?;
    append_encoder(state, bytes)
}
fn take_decoder<'a>(state: &mut Decoder<'a>, length: usize) -> Result<&'a [u8]> {
    ensure!(length <= state.bytes.len(), "truncated monitor value");
    let (bytes, remaining) = state.bytes.split_at(length);
    state.bytes = remaining;
    Ok(bytes)
}

fn length_decoder<'a>(state: &mut Decoder<'a>) -> Result<usize> {
    let length = u32::read(state)? as usize;
    ensure!(
        length <= state.bytes.len(),
        "invalid monitor collection length"
    );
    Ok(length)
}

pub(super) fn blob_decoder<'a>(state: &mut Decoder<'a>) -> Result<&'a [u8]> {
    let length = length_decoder(state)?;
    take_decoder(state, length)
}

pub(crate) fn encode<T: Wire>(value: &T, limit: usize) -> Result<Vec<u8>> {
    let mut output = Encoder {
        bytes: Vec::new(),
        limit: limit.min(u32::MAX as usize),
    };
    value.write(&mut output)?;
    Ok(output.bytes)
}

pub(crate) fn decode<T: Wire>(bytes: &[u8]) -> Result<T> {
    let mut input = Decoder { bytes };
    let value = T::read(&mut input)?;
    ensure!(input.bytes.is_empty(), "trailing monitor bytes");
    Ok(value)
}

macro_rules! number {
    ($($type:ty),+ $(,)?) => {$(
        impl Wire for $type {
            fn write(&self, output: &mut Encoder) -> Result<()> { append_encoder(&mut *output, &self.to_ne_bytes()) }
            fn read(input: &mut Decoder<'_>) -> Result<Self> {
                Ok(Self::from_ne_bytes(take_decoder(&mut *input, std::mem::size_of::<Self>())?.try_into()?))
            }
        }
    )+};
}
number!(u8, u16, u32, u64, i32, i64, f64, usize);
impl Wire for bool {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        u8::from(*self).write(output)
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        match u8::read(input)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => bail!("invalid monitor boolean"),
        }
    }
}
impl Wire for String {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        blob_encoder(&mut *output, self.as_bytes())
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        Ok(std::str::from_utf8(blob_decoder(&mut *input)?)?.to_owned())
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        length_encoder(&mut *output, self.len())?;
        for value in self {
            value.write(output)?;
        }
        Ok(())
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        let length = length_decoder(&mut *input)?;
        let mut values = Vec::with_capacity(length.min(4096));
        for _ in 0..length {
            values.push(T::read(input)?);
        }
        Ok(values)
    }
}
impl<T: Wire, const N: usize> Wire for [T; N] {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        for value in self {
            value.write(output)?;
        }
        Ok(())
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        let mut values = Vec::with_capacity(N);
        for _ in 0..N {
            values.push(T::read(input)?);
        }
        values
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid monitor array"))
    }
}
impl<T: Wire> Wire for Option<T> {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        match self {
            None => 0u8.write(output),
            Some(value) => {
                1u8.write(output)?;
                value.write(output)
            }
        }
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        match u8::read(input)? {
            0 => Ok(None),
            1 => Ok(Some(T::read(input)?)),
            _ => bail!("invalid monitor option"),
        }
    }
}
impl<T: Wire, E: Wire> Wire for std::result::Result<T, E> {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        match self {
            Ok(value) => {
                0u8.write(output)?;
                value.write(output)
            }
            Err(error) => {
                1u8.write(output)?;
                error.write(output)
            }
        }
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        match u8::read(input)? {
            0 => Ok(Ok(T::read(input)?)),
            1 => Ok(Err(E::read(input)?)),
            _ => bail!("invalid monitor result"),
        }
    }
}
impl<T: Wire> Wire for Arc<T> {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        self.as_ref().write(output)
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        Ok(Arc::new(T::read(input)?))
    }
}
macro_rules! tuple {
    ($($type:ident:$index:tt),+)=>{
        impl<$($type:Wire),+> Wire for ($($type,)+) {
            fn write(&self,output:&mut Encoder)->Result<()> {$(self.$index.write(output)?;)+Ok(())}
            fn read(input:&mut Decoder<'_>)->Result<Self> {Ok(($($type::read(input)?,)+))}
        }
    };
}
tuple!(A:0,B:1);
tuple!(A:0,B:1,C:2);
tuple!(A:0,B:1,C:2,D:3,E:4);
impl Wire for std::net::IpAddr {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        match self {
            Self::V4(ip) => {
                4u8.write(output)?;
                ip.octets().write(output)
            }
            Self::V6(ip) => {
                6u8.write(output)?;
                ip.octets().write(output)
            }
        }
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        match u8::read(input)? {
            4 => Ok(Self::V4(<[u8; 4]>::read(input)?.into())),
            6 => Ok(Self::V6(<[u8; 16]>::read(input)?.into())),
            _ => bail!("invalid monitor IP family"),
        }
    }
}
impl Wire for std::net::SocketAddr {
    fn write(&self, output: &mut Encoder) -> Result<()> {
        self.ip().write(output)?;
        self.port().write(output)?;
        if let Self::V6(address) = self {
            address.flowinfo().write(output)?;
            address.scope_id().write(output)?;
        }
        Ok(())
    }
    fn read(input: &mut Decoder<'_>) -> Result<Self> {
        let ip = std::net::IpAddr::read(input)?;
        let port = u16::read(input)?;
        Ok(match ip {
            std::net::IpAddr::V4(ip) => Self::V4(std::net::SocketAddrV4::new(ip, port)),
            std::net::IpAddr::V6(ip) => Self::V6(std::net::SocketAddrV6::new(
                ip,
                port,
                u32::read(input)?,
                u32::read(input)?,
            )),
        })
    }
}

macro_rules! wire_struct {
    ($type:ty { $($field:ident),* $(,)? })=>{
        impl $crate::ipc::codec::Wire for $type {
            fn write(&self,output:&mut $crate::ipc::codec::Encoder)->anyhow::Result<()> {$( $crate::ipc::codec::Wire::write(&self.$field,output)?; )*Ok(())}
            fn read(input:&mut $crate::ipc::codec::Decoder<'_>)->anyhow::Result<Self> {Ok(Self {$($field:$crate::ipc::codec::Wire::read(input)?,)*})}
        }
    };
}
macro_rules! wire_enum {
    ($type:ty { $($tag:literal => $variant:ident $(($($tuple:ident),+))? $({$($field:ident),*})? ),* $(,)? })=>{
        impl $crate::ipc::codec::Wire for $type {
            fn write(&self,output:&mut $crate::ipc::codec::Encoder)->anyhow::Result<()> {
                match self {$(Self::$variant $(($($tuple),+))? $({$($field),*})? => {
                    $crate::ipc::codec::Wire::write(&($tag as u8),output)?;
                    $($( $crate::ipc::codec::Wire::write($tuple,output)?; )+)?
                    $($( $crate::ipc::codec::Wire::write($field,output)?; )*)?
                    Ok(())
                }),*}
            }
            fn read(input:&mut $crate::ipc::codec::Decoder<'_>)->anyhow::Result<Self> {
                match <u8 as $crate::ipc::codec::Wire>::read(input)? {
                    $($tag=>Ok(Self::$variant $(($({let $tuple=$crate::ipc::codec::Wire::read(input)?;$tuple}),+))? $({$($field:$crate::ipc::codec::Wire::read(input)?),*})?),)*
                    _=>anyhow::bail!("invalid monitor enum tag"),
                }
            }
        }
    };
}
pub(crate) use {wire_enum, wire_struct};

#[cfg(test)]
mod tests {
    use {super::*, crate::ipc::schema::*};
    #[test]
    fn native_numbers_strings_and_socket_scope_roundtrip() {
        let scalar = 0x0102030405060708u64;
        let bytes = encode(&scalar, 128).unwrap();
        assert_eq!(bytes, scalar.to_ne_bytes());
        assert_eq!(decode::<u64>(&bytes).unwrap(), scalar);
        let text = "日本語\0literal".to_owned();
        assert_eq!(
            decode::<String>(&encode(&text, 1024).unwrap()).unwrap(),
            text
        );
        let address = std::net::SocketAddr::V6(std::net::SocketAddrV6::new(
            "fe80::1".parse().unwrap(),
            9090,
            123,
            7,
        ));
        assert_eq!(
            decode::<std::net::SocketAddr>(&encode(&address, 128).unwrap()).unwrap(),
            address
        );
        assert!(decode::<bool>(&[2]).is_err());
        assert!(decode::<Vec<u64>>(&u32::MAX.to_ne_bytes()).is_err());
        assert!(decode::<String>(&[1, 0, 0, 0, 255]).is_err());
        assert!(encode(&text, 8).unwrap_err().is::<LimitExceeded>());
    }
    #[test]
    fn closed_requests_reject_truncation_unknown_tags_and_trailing_fields() {
        let request = MonitorRequest::Process {
            id: ProcessId {
                pid: 42,
                start_time_ticks: u64::MAX,
            },
            resource: ProcessResource::Maps,
        };
        let bytes = encode(&request, MAX_REQUEST).unwrap();
        assert!(matches!(
            decode::<MonitorRequest>(&bytes).unwrap(),
            MonitorRequest::Process {
                resource: ProcessResource::Maps,
                ..
            }
        ));
        for n in 0..bytes.len() {
            assert!(decode::<MonitorRequest>(&bytes[..n]).is_err());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(decode::<MonitorRequest>(&extra).is_err());
        assert!(decode::<MonitorRequest>(&[255]).is_err());
    }
    #[test]
    fn captured_snapshots_remain_typed_until_main_processing() {
        use crate::ipc::rdma::parse_ib_gid;

        let snapshot = crate::ipc::model::system::SystemSnapshot {
            captured_at: 123,
            rdma_ports: vec![crate::ipc::rdma::RdmaPortInfo {
                device: "mlx5_0".into(),
                port: 1,
                link_layer: Some("InfiniBand".into()),
                state: Some("ACTIVE".into()),
                lid: Some(42),
                gid: parse_ib_gid("fe80:0000:0000:0000:0000:0000:0000:0051"),
                rate: Some("100 Gb/sec".into()),
            }],
            raw_sockets: vec![(
                10,
                crate::ipc::model::sockets::SocketInfo {
                    protocol: crate::ipc::model::socket_types::SocketProtocol::Tcp {
                        family: crate::ipc::model::socket_types::AddressFamily::Ipv6,
                    },
                    state: Some(crate::ipc::model::socket_types::SocketState::UnknownInet(
                        99,
                    )),
                    local: Some("[::1]:1234".parse().unwrap()),
                    remote: None,
                    path: None,
                    peer_inode: None,
                },
            )],
            ..Default::default()
        };
        let event = MonitorEvent::AcquiredSnapshot(Arc::new(snapshot));
        let bytes = encode(&event, MAX_RESPONSE).unwrap();
        let MonitorEvent::AcquiredSnapshot(decoded) = decode::<MonitorEvent>(&bytes).unwrap()
        else {
            panic!()
        };
        assert_eq!(decoded.captured_at, 123);
        assert_eq!(decoded.rdma_ports[0].device, "mlx5_0");
        assert_eq!(decoded.rdma_ports[0].lid, Some(42));
        assert_eq!(
            decoded.rdma_ports[0].gid.unwrap().to_string(),
            "fe80:0000:0000:0000:0000:0000:0000:0051"
        );
        assert_eq!(decoded.raw_sockets[0].0, 10);
        assert_eq!(
            decoded.raw_sockets[0].1.state,
            Some(crate::ipc::model::socket_types::SocketState::UnknownInet(
                99
            ))
        );
    }
}
