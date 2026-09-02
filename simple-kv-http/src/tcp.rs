use std::io;

use simple_kv::request::Request;
use simple_kv::response::{ParseError, Response, ResponseReader};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpStream, ToSocketAddrs};

pub struct KvTcpConnection {
    incoming: ResponseReader<OwnedReadHalf>,
    outgoing: OwnedWriteHalf,
}

impl KvTcpConnection {
    pub async fn new(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        let (incoming, outgoing) = stream.into_split();

        Ok(Self {
            incoming: ResponseReader::new(incoming),
            outgoing,
        })
    }

    pub async fn send(&mut self, request: &Request) -> Result<Response, ParseError> {
        self.outgoing.write_all(&request.to_bytes()).await?;
        self.outgoing.flush().await?;

        self.incoming
            .read_next_async()
            .await?
            .ok_or(ParseError::UnexpectedEof)
    }
}
