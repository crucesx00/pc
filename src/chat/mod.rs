use crate::error::{Error, Result};
use std::net::{SocketAddr, ToSocketAddrs};

pub mod client;
pub mod server;

pub fn parse_socket_addr(host: &str, port: &str) -> Result<SocketAddr> {
    let addr_opt = format!("{}:{}", host, port).to_socket_addrs()?.next();
    if addr_opt.is_none() {
        return Err(Error::Error(format!(
            "Error parsing addrss {}:{}",
            host, port
        )));
    }

    Ok(addr_opt.unwrap())
}
