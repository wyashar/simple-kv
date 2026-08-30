use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use log::{info, warn};
use thiserror::Error;

use crate::append_only_file::AppendOnlyFile;
use crate::command::{Command, CommandError, StoredValue};
use crate::config::Config;
use crate::key_store::KeyStore;
use crate::request::{RequestParseError, RequestReader};
use crate::response::Response;
use crate::util::Bytes;

struct ServerState {
    pub aof: AppendOnlyFile,
    pub key_store: KeyStore<Bytes, StoredValue>,
}

impl ServerState {
    pub const DEFAULT_AOF_PATH: &str = "simple-kv.aof";

    fn new(aof_path: &Path) -> Self {
        let mut aof =
            AppendOnlyFile::open(aof_path).expect("should be able to open append-only file");
        let key_store = Self::keystore_from_aof(&mut aof)
            .expect("should be able to restore key store from append-only file");

        Self { aof, key_store }
    }

    fn clone_aof(&self) -> AppendOnlyFile {
        self.aof
            .try_clone()
            .expect("should be able to clone append-only file")
    }

    fn spawn_sync_thread(&self, sync_interval: Duration) {
        let sync_aof = self.clone_aof();

        thread::spawn(move || {
            loop {
                thread::sleep(sync_interval);
                sync_aof.sync();
                info!("synced AOF");
            }
        });
    }

    // TODO: need to handle the case where partial writes are in the AOF
    fn keystore_from_aof(
        aof: &mut AppendOnlyFile,
    ) -> Result<KeyStore<Bytes, StoredValue>, KeyStoreRestoreError> {
        let mut requests = RequestReader::new(aof.get_file_content_from_start()?);
        let mut key_store = KeyStore::default();

        while let Some(request) = requests.read_next()? {
            let command = Command::try_from(request)?;
            let _ = command.apply(&mut key_store);
        }

        Ok(key_store)
    }

    fn apply_command(&mut self, command: Command) -> Response<Bytes> {
        if command.is_write_op()
            && let Err(err) = self.aof.append(&command.to_bytes())
        {
            warn!("failed to append command to AOF: {err}");
            return Response::Error(err.to_string());
        }

        command.apply(&mut self.key_store)
    }
}

#[derive(Debug, Error)]
enum KeyStoreRestoreError {
    #[error("failed to read append-only file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid request in append-only file: {0}")]
    Request(#[from] RequestParseError),
    #[error("invalid command in append-only file: {0}")]
    Command(#[from] CommandError),
}

pub async fn run(config: Config) {
    let addr = format!("{}:{}", config.server_address, config.server_port);
    let listener = TcpListener::bind(addr)
        .await
        .expect("should be able to bind to address");
    serve(listener, config).await;
}

pub async fn serve(listener: TcpListener, config: Config) {
    let addr = listener
        .local_addr()
        .expect("should be able to read local address");
    let aof_path = config
        .aof_path
        .as_deref()
        .unwrap_or_else(|| Path::new(ServerState::DEFAULT_AOF_PATH));

    info!(
        "Server started on {addr}. FSYNC Policy is {:?}. AOF is at {}",
        config.fsync_policy,
        aof_path.display()
    );

    let server_state = Arc::new(Mutex::new(ServerState::new(aof_path)));

    loop {
        if let Ok((stream, addr)) = listener.accept().await {
            info!("Client {addr} connected");
            tokio::spawn(handle_client_connection(
                stream,
                addr,
                Arc::clone(&server_state),
            ));
        } else {
            warn!("Unable to accept client connection");
            return;
        }
    }
}

async fn handle_client_connection(
    stream: TcpStream,
    peer: SocketAddr,
    ss: Arc<Mutex<ServerState>>,
) {
    let (incoming, mut outbound) = stream.into_split();
    let mut request_reader = RequestReader::new(BufReader::new(incoming));

    loop {
        let request = match request_reader.read_next_async().await {
            Ok(Some(request)) => request,
            Ok(None) => {
                info!("Client disconnected");
                return;
            }
            Err(e) => {
                warn!("Client {peer}: invalid RESP bytes: {e}");
                send_response(&mut outbound, Response::Error(e.to_string())).await;
                return;
            }
        };

        match Command::try_from(request) {
            Ok(command) => {
                info!("Received command from {peer}: {command}");
                let response = {
                    let mut ss = ss.lock().await;
                    ss.apply_command(command)
                };
                send_response(&mut outbound, response).await;
            }
            Err(e) => {
                warn!("Invalid command from {peer}: {e}");
                send_response(&mut outbound, Response::Error(e.to_string())).await;
            }
        }
    }
}

async fn send_response(outbound: &mut OwnedWriteHalf, response: Response<Bytes>) {
    match outbound.write_all(&response.to_bytes()).await {
        Ok(()) => info!("sent response: {response}"),
        Err(e) => warn!("failed to send response: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tempfile::TempDir;

    #[test]
    fn apply_command_appends_writes_to_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut server_state = ServerState::new(&dir.path().join("test.aof"));
        let command = Command::Set(b"key".to_vec(), b"value".to_vec());
        let expected = command.to_bytes();

        assert_eq!(server_state.apply_command(command), Response::Ok);

        let mut contents = Vec::new();
        server_state
            .aof
            .get_file_content_from_start()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");
        assert_eq!(contents, expected);
        assert_eq!(
            server_state.key_store.get(&b"key".to_vec()),
            Some(&StoredValue::new(b"value".to_vec()))
        );
    }

    #[test]
    fn apply_command_persists_expire_as_expire_at() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut server_state = ServerState::new(&dir.path().join("test.aof"));
        server_state
            .key_store
            .insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));
        let before = crate::util::get_unix_timestamp();
        let expire = Command::try_from(crate::request::Request::from_args(vec![
            b"EXPIRE".to_vec(),
            b"key".to_vec(),
            b"60".to_vec(),
        ]))
        .expect("EXPIRE should parse");

        assert_eq!(server_state.apply_command(expire), Response::Integer(1));

        let mut requests = RequestReader::new(
            server_state
                .aof
                .get_file_content_from_start()
                .expect("reader creation should work"),
        );
        let persisted = Command::try_from(
            requests
                .read_next()
                .expect("request should be valid")
                .expect("request should be present"),
        )
        .expect("command should be valid");

        let Command::ExpireAt(key, timestamp) = persisted else {
            panic!("EXPIRE should be persisted as EXPIREAT");
        };
        assert_eq!(key, b"key");
        assert!(timestamp >= before + 60);
        assert!(timestamp <= crate::util::get_unix_timestamp() + 60);
    }

    #[test]
    fn apply_command_does_not_append_reads_to_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut server_state = ServerState::new(&dir.path().join("test.aof"));
        server_state
            .key_store
            .insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(
            server_state.apply_command(Command::Get(b"key".to_vec())),
            Response::Cstr(b"value".to_vec())
        );

        let mut contents = Vec::new();
        server_state
            .aof
            .get_file_content_from_start()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");
        assert!(contents.is_empty());
    }

    #[test]
    fn restores_empty_key_store_from_empty_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");

        let key_store = ServerState::keystore_from_aof(&mut aof).expect("restore should work");

        assert!(key_store.get(&b"missing".to_vec()).is_none());
    }

    #[test]
    fn replays_aof_commands_in_order() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");

        for command in [
            Command::Set(b"kept".to_vec(), b"old".to_vec()),
            Command::Set(b"kept".to_vec(), b"new".to_vec()),
            Command::Set(b"deleted".to_vec(), b"value".to_vec()),
            Command::Del(vec![b"deleted".to_vec()]),
        ] {
            aof.append(&command.to_bytes()).expect("append should work");
        }

        let key_store = ServerState::keystore_from_aof(&mut aof).expect("restore should work");

        assert_eq!(
            key_store.get(&b"kept".to_vec()),
            Some(&StoredValue::new(b"new".to_vec()))
        );
        assert!(key_store.get(&b"deleted".to_vec()).is_none());
    }

    #[test]
    fn restores_absolute_expiration_from_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        let expires_at = crate::util::get_unix_timestamp() + 60;

        for command in [
            Command::Set(b"key".to_vec(), b"value".to_vec()),
            Command::ExpireAt(b"key".to_vec(), expires_at),
        ] {
            aof.append(&command.to_bytes()).expect("append should work");
        }

        let key_store = ServerState::keystore_from_aof(&mut aof).expect("restore should work");

        assert_eq!(
            key_store
                .get(&b"key".to_vec())
                .and_then(|value| value.expires_at),
            Some(expires_at)
        );
    }

    #[test]
    fn errors_on_truncated_aof_request() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        aof.append(b"*3\r\n$3\r\nSET\r\n")
            .expect("append should work");

        let err = match ServerState::keystore_from_aof(&mut aof) {
            Ok(_) => panic!("truncated request should fail"),
            Err(err) => err,
        };

        assert!(matches!(
            err,
            KeyStoreRestoreError::Request(RequestParseError::UnexpectedEof)
        ));
    }

    #[test]
    fn errors_on_invalid_aof_command() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        aof.append(b"*2\r\n$3\r\nFOO\r\n$3\r\nkey\r\n")
            .expect("append should work");

        let err = match ServerState::keystore_from_aof(&mut aof) {
            Ok(_) => panic!("invalid command should fail"),
            Err(err) => err,
        };

        assert!(matches!(err, KeyStoreRestoreError::Command(_)));
    }
}
