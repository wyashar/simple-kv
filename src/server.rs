use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;

use log::{info, warn};

use crate::command::Command;
use crate::config::Config;
use crate::key_store::KeyStore;
use crate::request::RequestParser;
use crate::request::{Request, RequestParseError};
use crate::response::Response;
use crate::util::Bytes;

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
    let mut key_store: KeyStore<Bytes, Bytes> = KeyStore::default();
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
    stream: TcpStream,
    peer: SocketAddr,
    rqst_parser: &mut RequestParser,
    key_store: &mut KeyStore<Bytes, Bytes>,
) {
    let mut reader = BufReader::new(stream);

    loop {
        let n = match reader.fill_buf() {
            Ok([]) => {
                info!("{peer} disconnected");
                return;
            }
            Ok(buf) => {
                rqst_parser.push_bytes(buf);
                buf.len()
            }
            Err(e) => {
                warn!("failed to read from {peer}: {e}");
                return;
            }
        };
        reader.consume(n);

        loop {
            match rqst_parser.parse_next() {
                Ok(Some(request)) => match Command::try_from(request) {
                    Ok(command) => {
                        info!("received command from {peer}: {command}");
                        send_response(reader.get_mut(), command.apply(key_store));
                    }
                    Err(e) => {
                        warn!("invalid command from {peer}: {e}");
                        send_response(reader.get_mut(), Response::Error(e.to_string()));
                    }
                },
                Ok(None) => break,
                Err(e) => {
                    warn!("failed to parse request from {peer}: {e}");
                    send_response(reader.get_mut(), Response::Error(e.to_string()));
                    return;
                }
            }
        }
    }
}

fn send_response(stream: &mut TcpStream, response: Response<&[u8]>) {
    match stream.write_all(&response.to_bytes()) {
        Ok(()) => info!("sent response: {response}"),
        Err(e) => warn!("failed to send response: {e}"),
    }
}
