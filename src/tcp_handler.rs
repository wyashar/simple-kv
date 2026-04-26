use std::io::{BufRead, BufReader};
use std::net::{TcpListener, SocketAddr, TcpStream};
use crate::wire_format::{WireFormatOperation, WireFormat};
use crate::kv_store::KvStore;

#[derive(thiserror::Error, Debug)]
pub enum ListenError {
    #[error("invalid address: {0}")]
    AddrParse(#[from] std::net::AddrParseError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub fn listen_on(addr: &str) -> Result<(), ListenError> {
    let parsed_addr = addr.parse::<SocketAddr>()?;
    let listener = TcpListener::bind(parsed_addr)?;
    let local_addr = listener.local_addr()?;
    println!("Listening on {}:{}", local_addr.ip(), local_addr.port());
    listener
        .incoming()
        .for_each( | peer| {
            match peer {
                Ok(stream) => handle_connection(&stream),
                Err(err) => eprintln!("Failed to establish a connection to peer: {:?}", err),
            }
        });

    Ok(())
}

fn handle_connection(stream: &TcpStream) -> () {
    println!("Connecting to peer...");

    let peer_addr = match stream.peer_addr() {
        Ok(peer_addr) => peer_addr,
        Err(err) => {
            eprintln!("Unable to connect to peer address: {}", err);
            return;
        }
    };


    println!("Connected to peer: {}", peer_addr);

    for line in BufReader::new(stream).lines() {
        match line {
            Ok(l) => {
                match l.parse::<WireFormat>() {
                    Ok(wire_format) => {
                        println!("Received a wire format: {:?}", wire_format);
                    },
                    Err(err) => eprintln!("Peer {} sent an incorrect wire format: {:?}", peer_addr, err)
                }
            },
            Err(err) => {
                eprintln!("Peer {} disconnected with error: {}", peer_addr, err);
                return;
            }
        }
    }

    println!("Client {} successfully disconnected", peer_addr);
}