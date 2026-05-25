use crate::kv_store::KvStoreResult;
use std::io::BufReader;
use std::net::TcpStream;

use crate::config::Config;
use crate::kv_store::KvStore;
use crate::tcp_server::TcpServer;
use crate::wire_format::{OperationView, WireFormat};
use base64::Engine;
use log::{debug, error, info};

pub fn run(config: Config) -> () {
    let tcp_server: TcpServer = TcpServer::bind(&config.server_address, config.server_port)
        .unwrap_or_else(|err| {
            error!("Failed to start KV Server: {err}");
            std::process::exit(1);
        });

    let mut kv_store: KvStore = KvStore::new();

    loop {
        let Ok((tcp_stream, peer_socket_address)) = tcp_server
            .accept()
            .inspect_err(|e| error!("Failed to establish connection to peer: {e}"))
        else {
            continue;
        };

        info!("Established a connection with peer {peer_socket_address}");
        let mut buff_reader: BufReader<&TcpStream> = BufReader::new(&tcp_stream);

        let Ok(message) = WireFormat::from_reader(&mut buff_reader)
            .inspect_err(|e| error!("Failed to deserialize message: {e}"))
        else {
            continue;
        };

        let Some(operation) = message.into_command() else {
            error!(
                "Clients cannot send simple strings to the server. Simple strings are reserved for server => client communication."
            );
            continue;
        };

        let op_name = operation.name();
        let debug_info = if log::log_enabled!(log::Level::Debug) {
            Some(match operation.as_view() {
                OperationView::Put { key, value } => (
                    base64::engine::general_purpose::STANDARD.encode(key),
                    Some(base64::engine::general_purpose::STANDARD.encode(value)),
                ),
                OperationView::Get { key } | OperationView::Del { key } => {
                    (base64::engine::general_purpose::STANDARD.encode(key), None)
                }
            })
        } else {
            None
        };

        let ts = chrono::Utc::now().with_timezone(&chrono_tz::America::New_York);
        match operation.execute(&mut kv_store) {
            KvStoreResult::Stored => {
                if let Some((key_b64, Some(value_b64))) = &debug_info {
                    debug!(
                        "[{ts}] [{peer_socket_address}] {op_name}: stored key={key_b64} value={value_b64}"
                    );
                } else {
                    info!("[{ts}] [{peer_socket_address}] {op_name}: stored successfully");
                }
            }
            KvStoreResult::Found(value) => {
                if let Some((key_b64, _)) = &debug_info {
                    let value_b64 = base64::engine::general_purpose::STANDARD.encode(&value);
                    debug!(
                        "[{ts}] [{peer_socket_address}] {op_name}: found key={key_b64} value={value_b64}"
                    );
                } else {
                    info!("[{ts}] [{peer_socket_address}] {op_name}: found value");
                }
            }
            KvStoreResult::Removed(value) => {
                if let Some((key_b64, _)) = &debug_info {
                    let value_b64 = base64::engine::general_purpose::STANDARD.encode(&value);
                    debug!(
                        "[{ts}] [{peer_socket_address}] {op_name}: removed key={key_b64} value={value_b64}"
                    );
                } else {
                    info!("[{ts}] [{peer_socket_address}] {op_name}: removed successfully");
                }
            }
            KvStoreResult::NotFound => {
                if let Some((key_b64, _)) = &debug_info {
                    debug!("[{ts}] [{peer_socket_address}] {op_name}: key not found key={key_b64}");
                } else {
                    info!("[{ts}] [{peer_socket_address}] {op_name}: key not found");
                }
            }
        }
    }
}
