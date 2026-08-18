use simple_kv::append_only_file::AppendOnlyFile;
use simple_kv::config::Config;
use simple_kv::server;

const AOF_PATH: &str = "appendonly.aof";

fn main() {
    env_logger::init();

    let config = Config::from_env();
    let addr = format!("{}:{}", config.server_address, config.server_port);
    let aof = AppendOnlyFile::open(AOF_PATH)
        .unwrap_or_else(|e| panic!("failed to open append-only file {AOF_PATH}: {e}"));

    server::run(&addr, aof);
}
