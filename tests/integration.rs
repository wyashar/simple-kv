use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use log::info;
use tempfile::TempDir;

use simple_kv::append_only_file::AppendOnlyFile;
use simple_kv::command::Command;
use simple_kv::response::{Response, ResponseParser};
use simple_kv::server;

const IO_TIMEOUT: Duration = Duration::from_secs(2);
const TEST_SERVER_ADDR: &str = "127.0.0.1:0";

fn init_logging() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .is_test(true)
        .try_init();
}

fn spawn_server() -> (SocketAddr, TempDir) {
    init_logging();

    let listener = TcpListener::bind(TEST_SERVER_ADDR).expect("failed to bind test server");
    let addr = listener.local_addr().expect("failed to read bound address");
    let aof_dir = TempDir::new().expect("temp dir creation should work");
    let aof = AppendOnlyFile::open(aof_dir.path().join("appendonly.aof"))
        .expect("open should create the AOF");
    thread::spawn(move || server::serve(listener, aof));
    info!("test server running on {addr}");
    (addr, aof_dir)
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
    let (addr, _aof_dir) = spawn_server();
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
    let (addr, _aof_dir) = spawn_server();
    assert_eq!(
        send_request(addr, &Command::Set(b"mykey".to_vec(), b"myval".to_vec())),
        Response::Ok
    );
}

#[test]
fn del_returns_integer() {
    let (addr, _aof_dir) = spawn_server();
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
    let (addr, _aof_dir) = spawn_server();
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
    let (addr, _aof_dir) = spawn_server();
    let Response::Error(msg) = send_raw(addr, b"&2\r\n$3\r\nGET\r\n$5\r\nmykey\r\n") else {
        panic!("expected an error response");
    };
    assert!(
        msg.contains("expected top-level array"),
        "unexpected error: {msg}"
    );
}
