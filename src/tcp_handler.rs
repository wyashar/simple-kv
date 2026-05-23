use crate::kv_store::KvStore;
use crate::wire_format::WireFormat;
use log::{error, info, warn};
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};

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
    let mut store = KvStore::new();
    info!("Listening on {}:{}", local_addr.ip(), local_addr.port());
    listener.incoming().for_each(|peer| match peer {
        Ok(stream) => handle_connection(stream, &mut store),
        Err(err) => error!("Failed to establish a connection to peer: {:?}", err),
    });

    Ok(())
}

fn handle_connection(mut stream: TcpStream, store: &mut KvStore) {
    let peer_addr = match stream.peer_addr() {
        Ok(addr) => addr,
        Err(err) => {
            error!("Unable to get peer address: {}", err);
            return;
        }
    };

    info!("Connected to peer: {}", peer_addr);

    let mut buffer = Vec::new();
    if let Err(err) = stream.read_to_end(&mut buffer) {
        warn!("Failed to read from peer {}: {}", peer_addr, err);
        return;
    }

    match WireFormat::try_from(buffer.as_slice()) {
        Ok(WireFormat::Cmd(op)) => {
            op.apply(store);
        }
        Ok(WireFormat::SimpleString(_)) => {
            warn!("Peer {} sent unexpected simple string", peer_addr);
        }
        Err(err) => {
            warn!("Peer {} sent invalid wire format: {:?}", peer_addr, err);
        }
    }

    info!("Client {} disconnected", peer_addr);
}
