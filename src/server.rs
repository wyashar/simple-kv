use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};

use log::{info, warn};

use crate::command::Command;
use crate::key_store::KeyStore;
use crate::request::RequestParser;
use crate::response::Response;
use crate::util::Bytes;

const READ_BUF_SIZE: usize = 8 * 1024;

pub fn run(addr: &str) {
    let listener = TcpListener::bind(addr).expect("failed to bind to address");
    serve(listener);
}

pub fn serve(listener: TcpListener) {
    let addr = listener.local_addr().expect("failed to read bound address");
    info!("listening on {addr}");
    let mut key_store = KeyStore::default();

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

fn handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    key_store: &mut KeyStore<Bytes, Bytes>,
) {
    let mut parser = RequestParser::default();
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

        parser.push_bytes(&buf[..n]);
        loop {
            match parser.parse_next() {
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
