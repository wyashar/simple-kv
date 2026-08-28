use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use log::info;
use tempfile::TempDir;

use simple_kv::command::Command;
use simple_kv::config::{Config, FsyncPolicy};
use simple_kv::response::{Response, ResponseReader};
use simple_kv::server;

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const TEST_SERVER_ADDR: &str = "127.0.0.1:0";

struct ServerProcess {
    child: Option<Child>,
    addr: SocketAddr,
}

impl ServerProcess {
    fn spawn(aof_path: &Path) -> Self {
        let listener = TcpListener::bind(TEST_SERVER_ADDR).expect("failed to reserve test address");
        let addr = listener.local_addr().expect("failed to read test address");
        drop(listener);

        let child = ProcessCommand::new(env!("CARGO_BIN_EXE_simple-kv"))
            .env("SERVER_ADDRESS", addr.ip().to_string())
            .env("SERVER_PORT", addr.port().to_string())
            .env("FSYNC_POLICY", "ONE_MIN")
            .env("AOF_PATH", aof_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start test server process");

        let server = Self {
            child: Some(child),
            addr,
        };
        server.wait_until_ready();
        server
    }

    fn wait_until_ready(&self) {
        let deadline = Instant::now() + IO_TIMEOUT;
        loop {
            if TcpStream::connect_timeout(&self.addr, Duration::from_millis(50)).is_ok() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "server did not start at {}",
                self.addr
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            child.wait().expect("failed to wait for test server");
        }
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
}

fn spawn_server_thread() -> SocketAddr {
    init_logging();

    let listener = TcpListener::bind(TEST_SERVER_ADDR).expect("failed to bind test server");
    let addr = listener.local_addr().expect("failed to read bound address");
    let aof_dir = TempDir::new().expect("failed to create temporary AOF directory");
    let config = Config {
        server_address: addr.ip().to_string(),
        server_port: addr.port(),
        fsync_policy: FsyncPolicy::OneMin,
        aof_path: Some(aof_dir.path().join("test.aof")),
    };
    thread::spawn(move || {
        let _aof_dir = aof_dir;
        server::serve(listener, config);
    });
    info!("test server running on {addr}");
    addr
}

fn connect(addr: SocketAddr) -> TcpStream {
    let stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT)
        .unwrap_or_else(|e| panic!("failed to connect to {addr}: {e}"));
    stream.set_nodelay(true).expect("set_nodelay");
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .expect("read timeout");
    stream
        .set_write_timeout(Some(IO_TIMEOUT))
        .expect("write timeout");
    stream
}

fn send_request(addr: SocketAddr, command: &Command) -> Response {
    info!("sent command to {addr}: {command}");
    send_raw(addr, &command.to_bytes())
}

fn send_raw(addr: SocketAddr, bytes: &[u8]) -> Response {
    let mut stream = connect(addr);
    stream.write_all(bytes).expect("failed to write request");

    let response = deserialize_response(&mut stream);
    info!("received response from {addr}: {response}");
    response
}

fn deserialize_response(stream: &mut TcpStream) -> Response {
    ResponseReader::new(BufReader::new(stream))
        .read_next()
        .expect("failed to parse server response")
        .expect("server closed the connection before sending a response")
}

#[test]
fn get_returns_cstr() {
    let addr = spawn_server_thread();
    let key = b"mykey".to_vec();
    let value = b"myval".to_vec();

    assert_eq!(
        send_request(addr, &Command::Set(key.clone(), value.clone())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Get(key)),
        Response::Cstr(value)
    );
}

#[test]
fn set_returns_ok() {
    let addr = spawn_server_thread();
    assert_eq!(
        send_request(addr, &Command::Set(b"mykey".to_vec(), b"myval".to_vec())),
        Response::Ok
    );
}

#[test]
fn expire_at_makes_expired_key_unavailable() {
    let addr = spawn_server_thread();
    let key = b"mykey".to_vec();
    assert_eq!(
        send_request(addr, &Command::Set(key.clone(), b"myval".to_vec())),
        Response::Ok
    );

    assert_eq!(
        send_request(addr, &Command::ExpireAt(key.clone(), 0)),
        Response::Integer(1)
    );
    assert_eq!(send_request(addr, &Command::Get(key)), Response::Null);
}

#[test]
fn expire_makes_key_unavailable() {
    let addr = spawn_server_thread();
    let key = b"mykey".to_vec();
    assert_eq!(
        send_request(addr, &Command::Set(key.clone(), b"myval".to_vec())),
        Response::Ok
    );

    assert_eq!(
        send_raw(addr, b"*3\r\n$6\r\nEXPIRE\r\n$5\r\nmykey\r\n$1\r\n0\r\n"),
        Response::Integer(1)
    );
    assert_eq!(send_request(addr, &Command::Get(key)), Response::Null);
}

#[test]
fn expiration_commands_return_zero_for_missing_keys() {
    let addr = spawn_server_thread();

    assert_eq!(
        send_raw(addr, b"*3\r\n$6\r\nEXPIRE\r\n$7\r\nmissing\r\n$2\r\n60\r\n"),
        Response::Integer(0)
    );
    assert_eq!(
        send_request(addr, &Command::ExpireAt(b"missing".to_vec(), u64::MAX)),
        Response::Integer(0)
    );
}

#[test]
fn set_clears_existing_expiration() {
    let addr = spawn_server_thread();
    let key = b"mykey".to_vec();
    assert_eq!(
        send_request(addr, &Command::Set(key.clone(), b"old".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::ExpireAt(key.clone(), 0)),
        Response::Integer(1)
    );

    assert_eq!(
        send_request(addr, &Command::Set(key.clone(), b"new".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Get(key)),
        Response::Cstr(b"new".to_vec())
    );
}

#[test]
fn del_returns_integer() {
    let addr = spawn_server_thread();
    assert_eq!(
        send_request(addr, &Command::Set(b"k1".to_vec(), b"v1".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Set(b"k2".to_vec(), b"v2".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Del(vec![b"k1".to_vec(), b"k2".to_vec()])),
        Response::Integer(2)
    );
}

#[test]
fn mset_stores_keys_retrievable_with_mget() {
    let addr = spawn_server_thread();
    let entries = vec![
        (b"k1".to_vec(), b"v1".to_vec()),
        (b"k2".to_vec(), b"v2".to_vec()),
        (b"k3".to_vec(), b"v3".to_vec()),
        (b"k4".to_vec(), b"v4".to_vec()),
    ];

    assert_eq!(
        send_request(addr, &Command::MSet(entries.clone())),
        Response::Ok
    );
    assert_eq!(
        send_request(
            addr,
            &Command::MGet(entries.iter().map(|(key, _)| key.clone()).collect())
        ),
        Response::Array(
            entries
                .into_iter()
                .map(|(_, value)| Response::Cstr(value))
                .collect()
        )
    );
}

#[test]
fn getall_returns_all_key_value_pairs() {
    let addr = spawn_server_thread();
    let entries = vec![
        (b"k1".to_vec(), b"v1".to_vec()),
        (b"k2".to_vec(), b"v2".to_vec()),
    ];
    assert_eq!(
        send_request(addr, &Command::MSet(entries.clone())),
        Response::Ok
    );

    let Response::Array(pairs) = send_request(addr, &Command::GetAll) else {
        panic!("GETALL should return an array");
    };

    assert_eq!(pairs.len(), entries.len());
    for (key, value) in entries {
        assert!(pairs.contains(&Response::Array(vec![
            Response::Cstr(key),
            Response::Cstr(value),
        ])));
    }
}

#[test]
fn mixed_commands_preserve_consistent_key_store_state() {
    let addr = spawn_server_thread();

    assert_eq!(
        send_request(addr, &Command::Get(b"missing".to_vec())),
        Response::Null
    );
    assert_eq!(
        send_request(addr, &Command::Set(b"alpha".to_vec(), b"one".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(
            addr,
            &Command::MSet(vec![
                (b"alpha".to_vec(), b"two".to_vec()),
                (b"beta".to_vec(), b"two".to_vec()),
                (b"gamma".to_vec(), b"three".to_vec()),
            ])
        ),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Get(b"alpha".to_vec())),
        Response::Cstr(b"two".to_vec())
    );
    assert_eq!(
        send_request(
            addr,
            &Command::MGet(vec![
                b"gamma".to_vec(),
                b"missing".to_vec(),
                b"beta".to_vec(),
            ])
        ),
        Response::Array(vec![
            Response::Cstr(b"three".to_vec()),
            Response::Null,
            Response::Cstr(b"two".to_vec()),
        ])
    );
    assert_eq!(
        send_request(
            addr,
            &Command::Del(vec![b"beta".to_vec(), b"missing".to_vec()])
        ),
        Response::Integer(1)
    );
    assert_eq!(
        send_request(
            addr,
            &Command::MSet(vec![
                (b"delta".to_vec(), b"four".to_vec()),
                (b"epsilon".to_vec(), b"five".to_vec()),
            ])
        ),
        Response::Ok
    );
    assert_eq!(
        send_request(
            addr,
            &Command::Del(vec![
                b"alpha".to_vec(),
                b"gamma".to_vec(),
                b"epsilon".to_vec(),
            ])
        ),
        Response::Integer(3)
    );
    assert_eq!(
        send_request(
            addr,
            &Command::MGet(vec![
                b"alpha".to_vec(),
                b"beta".to_vec(),
                b"gamma".to_vec(),
                b"delta".to_vec(),
                b"epsilon".to_vec(),
            ])
        ),
        Response::Array(vec![
            Response::Null,
            Response::Null,
            Response::Null,
            Response::Cstr(b"four".to_vec()),
            Response::Null,
        ])
    );
    assert_eq!(
        send_request(addr, &Command::Set(b"delta".to_vec(), b"updated".to_vec())),
        Response::Ok
    );
    assert_eq!(
        send_request(addr, &Command::Get(b"delta".to_vec())),
        Response::Cstr(b"updated".to_vec())
    );
}

#[test]
fn handles_multiple_client_connections_concurrently() {
    const CLIENT_COUNT: usize = 8;

    let addr = spawn_server_thread();
    let barrier = Arc::new(Barrier::new(CLIENT_COUNT));
    let clients: Vec<_> = (0..CLIENT_COUNT)
        .map(|client_id| {
            let barrier = Arc::clone(&barrier);

            thread::spawn(move || {
                let mut stream = connect(addr);
                let key = format!("key-{client_id}").into_bytes();
                let value = format!("value-{client_id}").into_bytes();

                barrier.wait();

                stream
                    .write_all(&Command::Set(key.clone(), value.clone()).to_bytes())
                    .expect("failed to write SET request");
                assert_eq!(deserialize_response(&mut stream), Response::Ok);

                stream
                    .write_all(&Command::Get(key.clone()).to_bytes())
                    .expect("failed to write GET request");
                assert_eq!(
                    deserialize_response(&mut stream),
                    Response::Cstr(value.clone())
                );

                (key, value)
            })
        })
        .collect();

    let entries: Vec<_> = clients
        .into_iter()
        .map(|client| client.join().expect("client thread panicked"))
        .collect();

    assert_eq!(
        send_request(
            addr,
            &Command::MGet(entries.iter().map(|(key, _)| key.clone()).collect())
        ),
        Response::Array(
            entries
                .into_iter()
                .map(|(_, value)| Response::Cstr(value))
                .collect()
        )
    );
}

#[test]
fn unrecognized_command_returns_error() {
    let addr = spawn_server_thread();
    let Response::Error(msg) = send_raw(addr, b"*2\r\n$3\r\nFOO\r\n$5\r\nmykey\r\n") else {
        panic!("expected an error response");
    };
    assert!(
        msg.contains("unrecognized command name: FOO"),
        "unexpected error: {msg}"
    );
}

#[test]
fn malformed_request_returns_error() {
    let addr = spawn_server_thread();
    let Response::Error(msg) = send_raw(addr, b"&2\r\n$3\r\nGET\r\n$5\r\nmykey\r\n") else {
        panic!("expected an error response");
    };
    assert!(
        msg.contains("expected top-level array"),
        "unexpected error: {msg}"
    );
}

#[test]
fn empty_aof_starts_with_empty_key_store() {
    let dir = TempDir::new().expect("failed to create temporary AOF directory");
    let aof_path = dir.path().join("empty.aof");
    std::fs::write(&aof_path, []).expect("failed to create empty AOF");
    let server = ServerProcess::spawn(&aof_path);

    assert_eq!(
        send_request(server.addr, &Command::Get(b"missing".to_vec())),
        Response::Null
    );
}

#[test]
fn key_store_state_survives_server_restart() {
    let dir = TempDir::new().expect("failed to create temporary AOF directory");
    let aof_path = dir.path().join("restart.aof");
    let key = b"persistent-key".to_vec();
    let value = b"persistent-value".to_vec();

    let mut first_server = ServerProcess::spawn(&aof_path);
    assert_eq!(
        send_request(first_server.addr, &Command::Set(key.clone(), value.clone())),
        Response::Ok
    );
    first_server.stop();

    let restarted_server = ServerProcess::spawn(&aof_path);
    assert_eq!(
        send_request(restarted_server.addr, &Command::Get(key)),
        Response::Cstr(value)
    );
}

#[test]
fn expire_deadline_survives_server_restart() {
    let dir = TempDir::new().expect("failed to create temporary AOF directory");
    let aof_path = dir.path().join("expire-restart.aof");
    let key = b"expiring-key".to_vec();

    let mut first_server = ServerProcess::spawn(&aof_path);
    assert_eq!(
        send_request(
            first_server.addr,
            &Command::Set(key.clone(), b"value".to_vec())
        ),
        Response::Ok
    );
    assert_eq!(
        send_raw(
            first_server.addr,
            b"*3\r\n$6\r\nEXPIRE\r\n$12\r\nexpiring-key\r\n$1\r\n1\r\n"
        ),
        Response::Integer(1)
    );
    first_server.stop();

    thread::sleep(Duration::from_secs(2));

    let restarted_server = ServerProcess::spawn(&aof_path);
    assert_eq!(
        send_request(restarted_server.addr, &Command::Get(key)),
        Response::Null
    );
}

#[test]
fn server_restores_expire_at_state_from_prepopulated_aof() {
    let dir = TempDir::new().expect("failed to create temporary AOF directory");
    let aof_path = dir.path().join("expire-at.aof");
    let mut contents = Command::Set(b"live".to_vec(), b"value".to_vec()).to_bytes();
    contents.extend(Command::ExpireAt(b"live".to_vec(), u64::MAX).to_bytes());
    contents.extend(Command::Set(b"expired".to_vec(), b"value".to_vec()).to_bytes());
    contents.extend(Command::ExpireAt(b"expired".to_vec(), 0).to_bytes());
    std::fs::write(&aof_path, contents).expect("failed to write AOF commands");

    let server = ServerProcess::spawn(&aof_path);

    assert_eq!(
        send_request(server.addr, &Command::Get(b"live".to_vec())),
        Response::Cstr(b"value".to_vec())
    );
    assert_eq!(
        send_request(server.addr, &Command::Get(b"expired".to_vec())),
        Response::Null
    );
}

#[test]
fn server_restores_state_from_prepopulated_aof() {
    let dir = TempDir::new().expect("failed to create temporary AOF directory");
    let aof_path = dir.path().join("prepopulated.aof");
    let mut contents = Command::Set(b"kept".to_vec(), b"value".to_vec()).to_bytes();
    contents.extend(Command::Set(b"deleted".to_vec(), b"value".to_vec()).to_bytes());
    contents.extend(Command::Del(vec![b"deleted".to_vec()]).to_bytes());
    std::fs::write(&aof_path, contents).expect("failed to write AOF commands");

    let server = ServerProcess::spawn(&aof_path);

    assert_eq!(
        send_request(server.addr, &Command::Get(b"kept".to_vec())),
        Response::Cstr(b"value".to_vec())
    );
    assert_eq!(
        send_request(server.addr, &Command::Get(b"deleted".to_vec())),
        Response::Null
    );
}
