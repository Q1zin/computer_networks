mod connection;
mod dns;
mod handler;
mod server;
mod socks5;
mod util;

use log::error;
use server::Server;
use std::{env, io::Result};

fn main() -> Result<()> {
    env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        error!("Usage: {} <port>", args[0]);
        std::process::exit(1);
    }

    let port = match args[1].parse() {
        Ok(p) => p,
        Err(_) => {
            error!("Invalid port number: {}", args[1]);
            std::process::exit(1);
        }
    };

    let mut server = Server::new(port)?;
    server.run()
}
