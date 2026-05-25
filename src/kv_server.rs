use std::io::{BufRead, BufReader, Read};
use std::net::TcpStream;

use crate::config::Config;
use crate::kv_store::{self, KvStore};
use crate::tcp_server::{self, TcpServer};
use crate::wire_format::{WireFormat, WireFormatParseError};
use log::{error, info};

pub fn run(config: Config) -> () {
    let tcp_server: TcpServer = TcpServer::bind(&config.server_address, config.server_port)
        .unwrap_or_else(|err| {
            error!("Failed to start KV Server: {err}");
            std::process::exit(1);
        });

    let mut kv_store: KvStore = KvStore::new();

    loop {
        match tcp_server.accept() {
            Ok((tcp_stream, socket_address)) => {
                info!("Established a connection on {socket_address}");
                let mut buff_reader: BufReader<&TcpStream> = BufReader::new(&tcp_stream);

                match WireFormat::from_reader(&mut buff_reader) {
                    Ok(message) => todo!(),
                    Err(err) => error!("Failed to deserialize message: {err}"),
                }
            }
            Err(error) => error!("Failed to establish connection to peer: {error}"),
        }
    }
}
