use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;

use log::{info, warn};

use crate::command::Command;
use crate::config::Config;
use crate::key_store::KeyStore;
use crate::request::RequestParser;
use crate::response::Response;
use crate::util::Bytes;

const READ_BUF_SIZE: usize = 8 * 1024;
const DEFAULT_AOF_PATH: &str = "simple-kv.aof";

pub fn run(config: Config) {
    let addr = format!("{}:{}", config.server_address, config.server_port);
    let listener = TcpListener::bind(addr).expect("failed to bind to address");
    serve(listener, config);
}

pub fn serve(listener: TcpListener, config: Config) {
    let addr = listener.local_addr().expect("failed to read bound address");
    let aof_path = config
        .aof_path
        .as_deref()
        .unwrap_or_else(|| Path::new(DEFAULT_AOF_PATH));
    info!(
        "listening on {addr} with {:?} fsync policy and AOF at {}",
        config.fsync_policy,
        aof_path.display()
    );
    let mut key_store = KeyStore::default();
    let mut rqst_parser = RequestParser::default();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match stream.peer_addr() {
                Ok(peer) => {
                    info!("accepted connection from {peer}");
                    handle_connection(stream, peer, &mut rqst_parser, &mut key_store);
                }
                Err(_) => info!("accepted connection from unknown peer"),
            },
            Err(e) => {
                info!("failed to accept connection: {e}");
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    rqst_parser: &mut RequestParser,
    key_store: &mut KeyStore<Bytes, Bytes>,
) {
    let mut buf = [0u8; READ_BUF_SIZE];

    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => {
                info!("{peer} disconnected");
                return;
            }
            Ok(n) => n,
            Err(e) => {
                warn!("failed to read from {peer}: {e}");
                return;
            }
        };

        rqst_parser.push_bytes(&buf[..n]);
        loop {
            match rqst_parser.parse_next() {
                Ok(Some(request)) => match Command::try_from(request) {
                    Ok(command) => {
                        info!("received command from {peer}: {command}");
                        send_response(&mut stream, apply_command(command, key_store));
                    }
                    Err(e) => {
                        warn!("invalid command from {peer}: {e}");
                        send_response(&mut stream, Response::Error(e.to_string()));
                    }
                },
                Ok(None) => break,
                Err(e) => {
                    warn!("failed to parse request from {peer}: {e}");
                    send_response(&mut stream, Response::Error(e.to_string()));
                    return;
                }
            }
        }
    }
}

fn apply_command<'store>(
    command: Command,
    key_store: &'store mut KeyStore<Bytes, Bytes>,
) -> Response<&'store [u8]> {
    match command {
        Command::Get(key) => key_store
            .get(&key)
            .map_or_else(|| Response::Null, |value| Response::Cstr(value.as_slice())),
        Command::Set(key, value) => {
            key_store.insert(key, value);
            Response::Ok
        }
        Command::Del(keys) => {
            let deleted_count = keys.iter().filter_map(|key| key_store.del(key)).count();
            Response::Integer(deleted_count as i64)
        }
    }
}

fn send_response(stream: &mut TcpStream, response: Response<&[u8]>) {
    match stream.write_all(&response.to_bytes()) {
        Ok(()) => info!("sent response: {response}"),
        Err(e) => warn!("failed to send response: {e}"),
    }
}
