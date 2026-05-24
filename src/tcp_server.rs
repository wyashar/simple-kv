use log::info;
use std::{
    fmt,
    net::{SocketAddr, TcpListener, TcpStream},
};

pub enum TcpServerError {
    InvalidSocketAddressFormat(String),
    InvalidPort(u16),
    IoError(std::io::Error),
}

impl fmt::Display for TcpServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSocketAddressFormat(socket_address) => {
                write!(f, "Invalid socket adddress format: {socket_address}")
            }
            Self::InvalidPort(port) => write!(f, "Invalid port: {port}"),
            Self::IoError(e) => write!(f, "I/O error while binding tcp server: {e}"),
        }
    }
}

pub struct TcpServer {
    tcp_listener: TcpListener,
}

impl TcpServer {
    pub fn bind(address: &str, port: u16) -> Result<Self, TcpServerError> {
        if port == 0 {
            return Err(TcpServerError::InvalidPort(port));
        }

        let endpoint: SocketAddr = format!("{}:{}", address, port)
            .parse::<SocketAddr>()
            .map_err(|_| {
                TcpServerError::InvalidSocketAddressFormat(format!("{}:{}", address, port))
            })?;

        let tcp_listener: TcpListener =
            TcpListener::bind(endpoint).map_err(TcpServerError::IoError)?;

        info!("Server started on {}:{}", address, port);

        Ok(Self { tcp_listener })
    }

    pub fn accept(&self) -> Result<(TcpStream, SocketAddr), TcpServerError> {
        self.tcp_listener.accept().map_err(TcpServerError::IoError)
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

        assert!(matches!(actual, Err(TcpServerError::InvalidPort(0))));
    }

    #[test]
    fn bind_fails_on_invalid_address() {
        let address: &str = "thisisaninvalidaddress";
        let port: u16 = 8080;
        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind(address, port);

        assert!(matches!(
            actual,
            Err(TcpServerError::InvalidSocketAddressFormat(_))
        ));
    }

    #[test]
    fn bind_fails_when_port_is_already_in_use() {
        let holder: TcpListener = TcpListener::bind("127.0.0.1:0").unwrap();
        let occupied_port: u16 = holder.local_addr().unwrap().port();

        let actual: Result<TcpServer, TcpServerError> = TcpServer::bind("127.0.0.1", occupied_port);

        assert!(matches!(actual, Err(TcpServerError::IoError(_))));
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
