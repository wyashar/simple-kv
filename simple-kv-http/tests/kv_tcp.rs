use std::time::Duration;

use simple_kv::config::Config;
use simple_kv::request::Request;
use simple_kv::response::Response;
use simple_kv::server;
use simple_kv_http::tcp::KvTcpConnection;
use tempfile::TempDir;
use tokio::net::TcpListener;

async fn spawn_kv_server() -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let addr = listener.local_addr().expect("failed to read bound address");
    let aof_dir = TempDir::new().expect("failed to create temporary AOF directory");
    let config = Config {
        server_address: addr.ip().to_string(),
        server_port: addr.port(),
        sync_interval: Duration::from_secs(60),
        ttl_cleanup_interval: Duration::from_secs(30),
        wal_path: Some(aof_dir.path().join("test.aof")),
    };
    tokio::spawn(async move {
        let _aof_dir = aof_dir;
        server::serve(listener, config).await;
    });
    addr
}

#[tokio::test]
async fn send_set_then_get_on_the_same_connection() {
    let addr = spawn_kv_server().await;
    let mut kv_tcp = KvTcpConnection::new(addr)
        .await
        .expect("should connect to simple-kv");

    let set = Request::from_args(vec![b"SET".to_vec(), b"key".to_vec(), b"value".to_vec()]);
    assert_eq!(
        kv_tcp.send(&set).await.expect("SET should get a response"),
        Response::Ok
    );

    let get = Request::from_args(vec![b"GET".to_vec(), b"key".to_vec()]);
    assert_eq!(
        kv_tcp.send(&get).await.expect("GET should get a response"),
        Response::Cstr(b"value".to_vec())
    );
}
