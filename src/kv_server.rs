use std::io::BufReader;
use std::net::TcpStream;

use crate::config::Config;
use crate::kv_request::{KvCommand, KvRequest};
use crate::kv_store::KvStore;
use crate::tcp_server::TcpServer;
use log::info;

pub fn run(config: Config) {
    let tcp_server: TcpServer = TcpServer::bind(&config.server_address, config.server_port)
        .expect("expected to bind to server address and port");

    loop {
        let Ok((stream, client_addr)) = tcp_server.accept() else {
            info!("Failed to connect to peer!");
            continue;
        };
        info!("Connected to peer: {client_addr}");

        let mut kv: KvStore<Vec<u8>, Vec<u8>> = KvStore::new();
        let mut buf_reader: BufReader<&TcpStream> = BufReader::new(&stream);

        match KvRequest::from_reader(&mut buf_reader) {
            Err(e) => {
                info!("Failed to deserialize KV Request: {:?}", e);
                continue;
            }
            Ok(request) => {
                info!("{}", request.command);
                match request.command {
                    KvCommand::Del(key) => {
                        kv.del(&key);
                    }
                    KvCommand::Get(key) => {
                        kv.get(&key);
                    }
                    KvCommand::Put(key, value) => {
                        kv.put(key, value);
                    }
                }
            }
        }
    }
}
