use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};

use log::{info, warn};

use crate::command::Command;
use crate::request::{ParseError, Request, RequestParser};

const READ_BUF_SIZE: usize = 8 * 1024;

pub fn run(addr: &str) {
    let listener = TcpListener::bind(addr).expect("failed to bind to address");
    info!("listening on {addr}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match stream.peer_addr() {
                Ok(peer) => {
                    info!("accepted connection from {peer}");
                    handle_connection(stream, peer);
                }
                Err(_) => info!("accepted connection from unknown peer"),
            },
            Err(e) => {
                info!("failed to accept connection: {e}");
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream, peer: SocketAddr) {
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
                    Ok(command) => info!("received command from {peer}: {command}"),
                    Err(e) => warn!("invalid command from {peer}: {e}"),
                },
                Ok(None) => break,
                Err(e) => {
                    warn!("failed to parse request from {peer}: {e}");
                    return;
                }
            }
        }
    }
}
