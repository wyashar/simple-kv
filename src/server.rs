use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

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
    pub wal: AppendOnlyFile,
    pub key_store: KeyStore<Bytes, StoredValue>,
}

impl ServerState {
    pub const DEFAULT_WAL_PATH: &str = "simple-kv.wal";

    fn from_wal(path: &Path) -> Self {
        let mut wal = AppendOnlyFile::open(path).expect("should be able to open append-only file");
        let key_store = Self::restore_keystore(&mut wal)
            .expect("should be able to restore key store from append-only file");

        Self { wal, key_store }
    }

    fn restore_keystore(
        wal: &mut AppendOnlyFile,
    ) -> Result<KeyStore<Bytes, StoredValue>, KeyStoreRestoreError> {
        let mut requests = RequestReader::new(wal.get_file_content_from_start()?);
        let mut key_store = KeyStore::default();

        while let Some(request) = requests.read_next()? {
            let command = Command::try_from(request)?;
            let _ = command.apply_write(&mut key_store);
        }

        Ok(key_store)
    }

    fn write(&mut self, command: Command) -> Response<Bytes> {
        if let Err(err) = self.wal.append(&command.to_bytes()) {
            warn!("failed to append command to AOF: {err}");
            return Response::Error(err.to_string());
        }

        command.apply_write(&mut self.key_store)
    }

    fn read(&self, command: Command) -> Response<Bytes> {
        command.apply_read(&self.key_store)
    }

    fn remove_expired(&mut self) -> usize {
        let expired_keys: Vec<_> = self
            .key_store
            .iter()
            .filter(|(_, value)| value.is_expired())
            .map(|(key, _)| key.clone())
            .collect();
        let removed = expired_keys.len();

        for key in expired_keys {
            let _ = self.key_store.del(&key);
        }

        removed
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
    let wal_path = config
        .wal_path
        .as_deref()
        .unwrap_or_else(|| Path::new(ServerState::DEFAULT_WAL_PATH));

    info!(
        "Server started on {addr}. Sync interval is {:?}. WAL is at {}",
        config.sync_interval,
        wal_path.display()
    );

    let server_state = ServerState::from_wal(wal_path);
    spawn_sync_thread(&server_state.wal, config.sync_interval);
    let server_state = Arc::new(RwLock::new(server_state));
    spawn_ttl_cleanup_task(Arc::clone(&server_state), config.ttl_cleanup_interval);

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

fn spawn_sync_thread(wal: &AppendOnlyFile, sync_interval: Duration) {
    let file_handle = wal
        .try_clone()
        .expect("should be able to clone append-only file");

    thread::spawn(move || {
        loop {
            thread::sleep(sync_interval);
            file_handle.sync();
            info!("synced AOF");
        }
    });
}

fn spawn_ttl_cleanup_task(ss: Arc<RwLock<ServerState>>, cleanup_interval: Duration) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(cleanup_interval).await;

            let removed = {
                let mut ss = ss.write().await;
                ss.remove_expired()
            };

            info!("removed {removed} expired keys");
        }
    });
}

async fn handle_client_connection(
    stream: TcpStream,
    peer: SocketAddr,
    ss: Arc<RwLock<ServerState>>,
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
                let response = if command.is_write_op() {
                    let mut ss = ss.write().await;
                    ss.write(command)
                } else {
                    let ss = ss.read().await;
                    ss.read(command)
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
        let mut server_state = ServerState::from_wal(&dir.path().join("test.aof"));
        let command = Command::Set(b"key".to_vec(), b"value".to_vec());
        let expected = command.to_bytes();

        assert_eq!(server_state.write(command), Response::Ok);

        let mut contents = Vec::new();
        server_state
            .wal
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
        let mut server_state = ServerState::from_wal(&dir.path().join("test.aof"));
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

        assert_eq!(server_state.write(expire), Response::Integer(1));

        let mut requests = RequestReader::new(
            server_state
                .wal
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
        let mut server_state = ServerState::from_wal(&dir.path().join("test.aof"));
        server_state
            .key_store
            .insert(b"key".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(
            server_state.read(Command::Get(b"key".to_vec())),
            Response::Cstr(b"value".to_vec())
        );

        let mut contents = Vec::new();
        server_state
            .wal
            .get_file_content_from_start()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");
        assert!(contents.is_empty());
    }

    #[test]
    fn cleanup_removes_expired_entries_only() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut server_state = ServerState::from_wal(&dir.path().join("test.aof"));
        server_state.key_store.insert(
            b"expired".to_vec(),
            StoredValue {
                bytes: b"old".to_vec(),
                expires_at: Some(0),
            },
        );
        server_state.key_store.insert(
            b"live-with-ttl".to_vec(),
            StoredValue {
                bytes: b"current".to_vec(),
                expires_at: Some(u64::MAX),
            },
        );
        server_state
            .key_store
            .insert(b"live".to_vec(), StoredValue::new(b"value".to_vec()));

        assert_eq!(server_state.remove_expired(), 1);
        assert!(server_state.key_store.get(&b"expired".to_vec()).is_none());
        assert!(
            server_state
                .key_store
                .get(&b"live-with-ttl".to_vec())
                .is_some()
        );
        assert!(server_state.key_store.get(&b"live".to_vec()).is_some());
    }

    #[test]
    fn restores_empty_key_store_from_empty_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");

        let key_store = ServerState::restore_keystore(&mut aof).expect("restore should work");

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

        let key_store = ServerState::restore_keystore(&mut aof).expect("restore should work");

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

        let key_store = ServerState::restore_keystore(&mut aof).expect("restore should work");

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

        let err = match ServerState::restore_keystore(&mut aof) {
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

        let err = match ServerState::restore_keystore(&mut aof) {
            Ok(_) => panic!("invalid command should fail"),
            Err(err) => err,
        };

        assert!(matches!(err, KeyStoreRestoreError::Command(_)));
    }
}
