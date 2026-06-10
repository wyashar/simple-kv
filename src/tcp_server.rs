use log::info;
use std::net::{SocketAddr, TcpListener, TcpStream};

#[derive(Debug, thiserror::Error)]
pub enum TcpServerError {
    #[error("bad socket address. could not parse address {0} as SocketAddr")]
    BadSocketAddr(String),
    #[error("port {0} is not allowed")]
    BadPort(u16),
    #[error("could not bind to address {0:?}")]
    Io(#[from] std::io::Error),
}

pub struct TcpServer {
    tcp_listener: TcpListener,
}

impl TcpServer {
    pub fn bind(address: &str, port: u16) -> Result<Self, TcpServerError> {
        if port == 0 {
            return Err(TcpServerError::BadPort(port));
        }

        let addr: String = format!("{}:{}", address, port);
        let endpoint: SocketAddr = addr
            .parse::<SocketAddr>()
            .map_err(|_| TcpServerError::BadSocketAddr(addr))?;

        let tcp_listener: TcpListener = TcpListener::bind(endpoint)?;

        info!("Server started on {}:{}", address, port);

        Ok(Self { tcp_listener })
    }

    pub fn accept(&self) -> Result<(TcpStream, SocketAddr), TcpServerError> {
        Ok(self.tcp_listener.accept()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_fails_on_zero_port() {
        let address: &str = "127.0.0.1";
        let port: u16 = 0;
        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind(address, port);

        assert!(matches!(actual, Err(TcpServerError::BadPort(0))));
    }

    #[test]
    fn bind_fails_on_invalid_address() {
        let address: &str = "thisisaninvalidaddress";
        let port: u16 = 8080;
        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind(address, port);

        assert!(matches!(actual, Err(TcpServerError::BadSocketAddr(_))));
    }

    #[test]
    fn bind_fails_when_port_is_already_in_use() {
        let holder: TcpListener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port: u16 = holder.local_addr().unwrap().port();

        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind("127.0.0.1", occupied_port);

        assert!(matches!(actual, Err(TcpServerError::Io(_))));
    }

    #[test]
    fn bind_succeeds_on_valid_address_and_port() {
        let port: u16 = free_port();
        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind("127.0.0.1", port);

        assert!(actual.is_ok());
    }

    #[test]
    fn accept_returns_a_connected_stream() {
        let port: u16 = free_port();
        let server: TcpServer = TcpServer::bind("127.0.0.1", port).unwrap();

        let client =
            std::thread::spawn(move || TcpStream::connect(format!("127.0.0.1:{port}")).unwrap());

        let (_stream, peer_addr) = server.accept().unwrap();
        assert!(peer_addr.ip().is_loopback());

        client.join().unwrap();
    }

    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
}
