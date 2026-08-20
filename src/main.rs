use simple_kv::config::Config;
use simple_kv::server;

fn main() {
    env_logger::init();

    let config = Config::from_env();
    server::run(config);
}
