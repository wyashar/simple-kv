use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;

use log::{info, warn};

use crate::command::Command;
use crate::config::Config;
use crate::key_store::KeyStore;
use crate::request::RequestReader;
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

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match stream.peer_addr() {
                Ok(peer) => {
                    info!("accepted connection from {peer}");
                    handle_connection(stream, peer, &mut key_store);
                }
                Err(_) => info!("accepted connection from unknown peer"),
            },
            Err(e) => {
                info!("failed to accept connection: {e}");
            }
        }
    }
}

fn handle_connection(stream: TcpStream, peer: SocketAddr, key_store: &mut KeyStore<Bytes, Bytes>) {
    let mut requests = RequestReader::new(BufReader::new(stream));

    loop {
        let request = match requests.read_next() {
            Ok(Some(request)) => request,
            Ok(None) => {
                info!("{peer} disconnected");
                return;
            }
            Err(e) => {
                warn!("failed to read request from {peer}: {e}");
                send_response(requests.get_mut().get_mut(), Response::Error(e.to_string()));
                return;
            }
        };

        match Command::try_from(request) {
            Ok(command) => {
                info!("received command from {peer}: {command}");
                send_response(requests.get_mut().get_mut(), command.apply(key_store));
            }
            Err(e) => {
                warn!("invalid command from {peer}: {e}");
                send_response(requests.get_mut().get_mut(), Response::Error(e.to_string()));
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
