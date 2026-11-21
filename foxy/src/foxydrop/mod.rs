//! FoxyDrop: локальный сервер приёма файлов (без авторизации и без облачных сервисов).
mod network;
mod page;
pub mod qr;
mod server;
mod upload;

pub use network::local_ipv4_addresses;
pub use server::DropService;

use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Stopped,
    Starting,
    Running,
    Stopping,
}

pub fn url(addr: SocketAddr) -> String {
    format!("http://{}", addr)
}
