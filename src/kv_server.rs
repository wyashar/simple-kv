use std::borrow::Cow;
use std::io::{BufReader, BufWriter, Error, Write};
use std::net::SocketAddr;
use std::net::TcpStream;

use crate::config::Config;
use crate::kv_request::{KvCommand, KvRequest};
use crate::kv_response::KvResponse;
use crate::kv_store::KvStore;
use crate::tcp_server::TcpServer;
use log::{info, warn};

pub fn run(config: Config) {
    let tcp_server: TcpServer = TcpServer::bind(&config.server_address, config.server_port)
        .expect("expected to bind to server address and port");

    loop {
        let (stream, client_addr) = match tcp_server.accept() {
            Ok(pair) => pair,
            Err(e) => {
                warn!("Failed to accept connection: {e}");
                continue;
            }
        };

        info!("Connected to peer: {client_addr}");

        if let Err(e) = handle_connection(stream, client_addr) {
            warn!("Failed to send response to {client_addr}: {e}. Terminating connection");
        }
    }
}

fn handle_connection(stream: TcpStream, client_addr: SocketAddr) -> Result<(), Error> {
    let mut reader: BufReader<&TcpStream> = BufReader::new(&stream);
    let mut writer: BufWriter<&TcpStream> = BufWriter::new(&stream);
    let mut kv: KvStore<Vec<u8>, Vec<u8>> = KvStore::new();

    loop {
        let request = match KvRequest::from_reader(&mut reader) {
            Ok(r) => r,
            Err(e) => {
                warn!("Bad request from {client_addr}: {e}");
                send_response(&mut writer, KvResponse::Error(e.to_string()), client_addr)?;
                return Ok(());
            }
        };

        info!("Received: [{request}] from {client_addr}");

        match request.command {
            KvCommand::Put(key, value) => {
                kv.put(key, value);
                send_response(&mut writer, KvResponse::Okay, client_addr)?;
            }
            KvCommand::Del(key) => {
                kv.del(&key);
                send_response(&mut writer, KvResponse::Okay, client_addr)?;
            }
            KvCommand::Get(key) => {
                let res = match kv.get(&key) {
                    None => KvResponse::NotFound,
                    Some(val) => KvResponse::Value(Cow::Borrowed(val)),
                };

                send_response(&mut writer, res, client_addr)?;
            }
        }
    }
}

fn send_response(
    buf_writer: &mut BufWriter<&TcpStream>,
    res: KvResponse,
    client_addr: SocketAddr,
) -> Result<(), std::io::Error> {
    res.write_to(buf_writer)?;
    buf_writer.flush()?;
    info!("Sent: [{res}] to {client_addr}");

    Ok(())
}
