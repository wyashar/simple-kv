use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::info;
use tempfile::TempDir;

use simple_kv::command::Command;
use simple_kv::config::{Config, FsyncPolicy};
use simple_kv::response::{Response, ResponseParser};
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

fn spawn_server() -> SocketAddr {
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
    let mut parser = ResponseParser::default();
    let mut buf = [0u8; 1024];

    loop {
        match parser.parse_next() {
            Ok(Some(response)) => return response,
            Ok(None) => {}
            Err(e) => panic!("failed to parse server response: {e}"),
        }

        match stream.read(&mut buf) {
            Ok(0) => panic!("server closed the connection before sending a response"),
            Ok(n) => parser.push_bytes(&buf[..n]),
            Err(e) => panic!("failed to read from server: {e}"),
        }
    }
}

#[test]
fn get_returns_cstr() {
    let addr = spawn_server();
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
    let addr = spawn_server();
    assert_eq!(
        send_request(addr, &Command::Set(b"mykey".to_vec(), b"myval".to_vec())),
        Response::Ok
    );
}

#[test]
fn del_returns_integer() {
    let addr = spawn_server();
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
fn unrecognized_command_returns_error() {
    let addr = spawn_server();
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
    let addr = spawn_server();
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
