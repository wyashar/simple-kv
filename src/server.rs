use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;

use log::{info, warn};
use thiserror::Error;

use crate::append_only_file::AppendOnlyFile;
use crate::command::{Command, CommandError};
use crate::config::Config;
use crate::key_store::KeyStore;
use crate::request::{RequestParseError, RequestReader};
use crate::response::Response;
use crate::util::Bytes;

const DEFAULT_AOF_PATH: &str = "simple-kv.aof";

#[derive(Debug, Error)]
enum KeyStoreRestoreError {
    #[error("failed to read append-only file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid request in append-only file: {0}")]
    Request(#[from] RequestParseError),
    #[error("invalid command in append-only file: {0}")]
    Command(#[from] CommandError),
}

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
        "Server started on {addr}. FSYNC Policy is {:?}. AOF is at {}",
        config.fsync_policy,
        aof_path.display()
    );

    let mut aof = AppendOnlyFile::open(aof_path).expect("failed to open append-only file");
    let mut key_store =
        restore_key_store(&mut aof).expect("failed to restore key store from append-only file");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match stream.peer_addr() {
                Ok(peer) => {
                    info!("accepted connection from {peer}");
                    handle_connection(stream, peer, &mut key_store, &mut aof);
                }
                Err(_) => info!("accepted connection from unknown peer"),
            },
            Err(e) => {
                info!("failed to accept connection: {e}");
            }
        }
    }
}

fn restore_key_store(
    aof: &mut AppendOnlyFile,
) -> Result<KeyStore<Bytes, Bytes>, KeyStoreRestoreError> {
    let mut requests = RequestReader::new(aof.reader()?);
    let mut key_store = KeyStore::default();

    while let Some(request) = requests.read_next()? {
        let command = Command::try_from(request)?;
        let _ = command.apply(&mut key_store);
    }

    Ok(key_store)
}

fn handle_connection(
    stream: TcpStream,
    peer: SocketAddr,
    key_store: &mut KeyStore<Bytes, Bytes>,
    aof: &mut AppendOnlyFile,
) {
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
                send_response(
                    requests.get_reader_mut().get_mut(),
                    Response::Error(e.to_string()),
                );
                return;
            }
        };

        match Command::try_from(request) {
            Ok(command) => {
                info!("received command from {peer}: {command}");
                send_response(
                    requests.get_reader_mut().get_mut(),
                    apply_command(command, key_store, aof),
                );
            }
            Err(e) => {
                warn!("invalid command from {peer}: {e}");
                send_response(
                    requests.get_reader_mut().get_mut(),
                    Response::Error(e.to_string()),
                );
            }
        }
    }
}

fn apply_command<'store>(
    command: Command,
    key_store: &'store mut KeyStore<Bytes, Bytes>,
    aof: &mut AppendOnlyFile,
) -> Response<&'store [u8]> {
    if command.is_write_op()
        && let Err(err) = aof.append(&command.to_bytes())
    {
        warn!("failed to append command to AOF: {err}");
        return Response::Error(err.to_string());
    }

    command.apply(key_store)
}

fn send_response(stream: &mut TcpStream, response: Response<&[u8]>) {
    match stream.write_all(&response.to_bytes()) {
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
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        let mut key_store = KeyStore::default();
        let command = Command::Set(b"key".to_vec(), b"value".to_vec());
        let expected = command.to_bytes();

        assert_eq!(
            apply_command(command, &mut key_store, &mut aof),
            Response::Ok
        );

        let mut contents = Vec::new();
        aof.reader()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");
        assert_eq!(contents, expected);
        assert_eq!(key_store.get(&b"key".to_vec()), Some(&b"value".to_vec()));
    }

    #[test]
    fn apply_command_does_not_append_reads_to_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        let mut key_store = KeyStore::default();
        key_store.insert(b"key".to_vec(), b"value".to_vec());

        assert_eq!(
            apply_command(Command::Get(b"key".to_vec()), &mut key_store, &mut aof),
            Response::Cstr(b"value".as_slice())
        );

        let mut contents = Vec::new();
        aof.reader()
            .expect("reader creation should work")
            .read_to_end(&mut contents)
            .expect("read should work");
        assert!(contents.is_empty());
    }

    #[test]
    fn restores_empty_key_store_from_empty_aof() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");

        let key_store = restore_key_store(&mut aof).expect("restore should work");

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

        let key_store = restore_key_store(&mut aof).expect("restore should work");

        assert_eq!(key_store.get(&b"kept".to_vec()), Some(&b"new".to_vec()));
        assert!(key_store.get(&b"deleted".to_vec()).is_none());
    }

    #[test]
    fn errors_on_truncated_aof_request() {
        let dir = TempDir::new().expect("temp dir creation should work");
        let mut aof = AppendOnlyFile::open(dir.path().join("test.aof")).expect("open should work");
        aof.append(b"*3\r\n$3\r\nSET\r\n")
            .expect("append should work");

        let err = match restore_key_store(&mut aof) {
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

        let err = match restore_key_store(&mut aof) {
            Ok(_) => panic!("invalid command should fail"),
            Err(err) => err,
        };

        assert!(matches!(err, KeyStoreRestoreError::Command(_)));
    }
}
