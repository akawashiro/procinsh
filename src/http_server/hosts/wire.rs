//! Agent link framing: `u32` big-endian length, one encoding byte, then a JSON
//! [`Message`]. Larger messages are gzip-compressed; snapshots compress well.
//!
//! The hub sends the same closed [`MonitorRequest`]s the HTTP layer sends to its
//! own helper. The agent processes acquisition replies on its host and returns
//! an [`AgentResponse`] ready for HTTP output. JSON framing supports links
//! between hosts with different architectures.
//!
//! # Interface
//! - [`AgentResponse`] (`pub(super)`): `type AgentResponse = Result<crate::http_server::protocol::MonitorResponse, String>`.
//! - [`PROTOCOL`] (`pub(super)`): `const PROTOCOL: u32`.
//! - [`Message`] (`pub(super)`): `enum Message`.
//! - [`read_host_message`] (`pub(super)`): `async fn read_host_message(reader: &mut (impl `[`AsyncRead`]` + `[`Unpin`]`)) -> `[`Result`]`<`[`Message`]`>`.
//! - [`write_host_message`] (`pub(super)`): `async fn write_host_message( writer: &mut (impl `[`AsyncWrite`]` + `[`Unpin`]`), message: `[`Message`]`, ) -> `[`Result`]`<()>`.
//! - [`same_secret`] (`pub(super)`): `fn same_secret(expected: &str, actual: &str) -> bool`.

use crate::ipc::schema::{MAX_RESPONSE, MonitorRequest};
use anyhow::{Context, Result, bail, ensure};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) type AgentResponse = Result<crate::http_server::protocol::MonitorResponse, String>;

pub(super) const PROTOCOL: u32 = 2;
/// The largest message, compressed or not, including its encoding byte.
const MAX_FRAME_BYTES: usize = MAX_RESPONSE + 64 * 1024;
/// Smaller messages are sent as plain JSON.
const COMPRESS_ABOVE: usize = 1024;
/// Larger messages are encoded and decoded off the async workers.
const BLOCKING_ABOVE: usize = 64 * 1024;
const PLAIN: u8 = 1;
const GZIP: u8 = 2;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Message {
    /// The agent's first message. The token is compared in constant time.
    Hello {
        protocol: u32,
        host: String,
        machine_id: String,
        version: String,
        kernel: Option<String>,
        arch: String,
        token: Option<String>,
    },
    Welcome {
        protocol: u32,
        version: String,
    },
    /// The hub refuses the agent and closes the link.
    Reject {
        reason: String,
    },
    /// A helper request for the agent; `id` matches the response.
    Request {
        id: u64,
        request: MonitorRequest,
    },
    Response {
        id: u64,
        response: AgentResponse,
    },
    Ping,
    Pong,
}

fn encode_host_message(message: &Message) -> Result<(u8, Vec<u8>)> {
    let json = serde_json::to_vec(message)?;
    if json.len() <= COMPRESS_ABOVE {
        return Ok((PLAIN, json));
    }
    let mut encoder = GzEncoder::new(Vec::with_capacity(json.len() / 4), Compression::fast());
    encoder.write_all(&json)?;
    Ok((GZIP, encoder.finish()?))
}

fn decode_host_message(encoding: u8, payload: &[u8]) -> Result<Message> {
    match encoding {
        PLAIN => Ok(serde_json::from_slice(payload)?),
        GZIP => {
            let mut json = Vec::new();
            GzDecoder::new(payload)
                .take(MAX_FRAME_BYTES as u64 + 1)
                .read_to_end(&mut json)
                .context("invalid compressed agent message")?;
            ensure!(json.len() <= MAX_FRAME_BYTES, "agent message is too large");
            Ok(serde_json::from_slice(&json)?)
        }
        code => bail!("unknown agent message encoding {code}"),
    }
}

pub(super) async fn read_host_message(reader: &mut (impl AsyncRead + Unpin)) -> Result<Message> {
    let length = reader.read_u32().await? as usize;
    ensure!(
        (2..=MAX_FRAME_BYTES).contains(&length),
        "agent frame length {length} is out of range"
    );
    let encoding = reader.read_u8().await?;
    let mut payload = vec![0; length - 1];
    reader.read_exact(&mut payload).await?;
    if payload.len() > BLOCKING_ABOVE {
        tokio::task::spawn_blocking(move || decode_host_message(encoding, &payload)).await?
    } else {
        decode_host_message(encoding, &payload)
    }
}

pub(super) async fn write_host_message(
    writer: &mut (impl AsyncWrite + Unpin),
    message: Message,
) -> Result<()> {
    let large = matches!(&message, Message::Response { .. });
    let (encoding, payload) = if large {
        tokio::task::spawn_blocking(move || encode_host_message(&message)).await??
    } else {
        encode_host_message(&message)?
    };
    ensure!(
        payload.len() < MAX_FRAME_BYTES,
        "agent message is too large"
    );
    writer.write_u32(payload.len() as u32 + 1).await?;
    writer.write_u8(encoding).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Compares secrets without an early exit on the first differing byte.
pub(super) fn same_secret(expected: &str, actual: &str) -> bool {
    let (expected, actual) = (expected.as_bytes(), actual.as_bytes());
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
}

#[cfg(test)]
mod tests {
    use {super::*, crate::http_server::protocol::MonitorResponse, serde_json::json};

    #[tokio::test]
    async fn messages_round_trip_compressed_and_reject_bad_frames() {
        let mut buffer = Vec::new();
        write_host_message(&mut buffer, Message::Ping)
            .await
            .unwrap();
        let mut request = Vec::new();
        write_host_message(
            &mut request,
            Message::Request {
                id: 3,
                request: MonitorRequest::Poll { session: 42 },
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_host_message(&mut request.as_slice()).await.unwrap(),
            Message::Request {
                id: 3,
                request: MonitorRequest::Poll { session: 42 }
            }
        ));
        let large = json!({"processes": vec!["x".repeat(100); 100]});
        write_host_message(
            &mut buffer,
            Message::Response {
                id: 7,
                response: Ok(MonitorResponse::Data(large.clone())),
            },
        )
        .await
        .unwrap();
        assert_eq!(buffer[4], PLAIN);
        assert!(buffer.len() < 2000, "the repeated response is compressed");
        let mut reader = buffer.as_slice();
        assert!(matches!(
            read_host_message(&mut reader).await.unwrap(),
            Message::Ping
        ));
        let Message::Response {
            id: 7,
            response: Ok(MonitorResponse::Data(data)),
        } = read_host_message(&mut reader).await.unwrap()
        else {
            panic!("response expected");
        };
        assert_eq!(data, large);
        assert!(
            read_host_message(&mut reader).await.is_err(),
            "end of stream"
        );
        for frame in [
            [0u8, 0, 0, 1, PLAIN].to_vec(),
            ((MAX_FRAME_BYTES + 1) as u32).to_be_bytes().to_vec(),
            [0, 0, 0, 3, 9, b'{', b'}'].to_vec(),
            [0, 0, 0, 3, GZIP, b'{', b'}'].to_vec(),
            [0, 0, 0, 2, PLAIN, b'{'].to_vec(),
            [0, 0, 0, 8, PLAIN, b'"', b'n', b'o', b'p', b'e', b'"', b' '].to_vec(),
        ] {
            assert!(read_host_message(&mut frame.as_slice()).await.is_err());
        }
    }

    #[test]
    fn secrets_compare_whole_values() {
        assert!(same_secret("token", "token"));
        assert!(!same_secret("token", "tokem"));
        assert!(!same_secret("token", "token2"));
        assert!(!same_secret("", "x"));
    }
}
