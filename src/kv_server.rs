use crate::kv_store::KvStoreResult;
use std::io::{BufReader, BufWriter};
use std::net::TcpStream;

use crate::config::Config;
use crate::kv_store::KvStore;
use crate::tcp_server::TcpServer;
use crate::wire_format::WireFormat;
use log::{error, info};

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

        info!("Connected to peer: {peer_socket_address}");
        let mut buff_reader: BufReader<&TcpStream> = BufReader::new(&tcp_stream);

        let Ok(request) = WireFormat::from_reader(&mut buff_reader)
            .inspect_err(|e| error!("Failed to deserialize message: {e}"))
        else {
            continue;
        };

        info!("Received request: {request}");

        let WireFormat::Cmd(operation) = request else {
            error!(
                "Clients cannot send simple strings to the server. Simple strings are reserved for server => client communication."
            );
            continue;
        };

        let buf_writer: BufWriter<&TcpStream> = BufWriter::new(&tcp_stream);
        let op_name = operation.name();
        match operation.execute(&mut kv_store) {
            KvStoreResult::Stored => {
                info!("[{peer_socket_address}] {op_name}: stored successfully");
            }
            KvStoreResult::Found(value) => {
                info!("[{peer_socket_address}] {op_name}: found value");
            }
            KvStoreResult::Removed(value) => {
                info!("[{peer_socket_address}] {op_name}: removed successfully");
            }
            KvStoreResult::NotFound => {
                info!("[{peer_socket_address}] {op_name}: key not found");
            }
        }
    }
}
