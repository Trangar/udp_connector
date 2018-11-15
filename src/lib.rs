#[macro_use]
extern crate serde_derive;

mod client;
mod packet;
mod param;
mod server;

#[cfg(test)]
pub mod test;

pub type Result<T> = std::result::Result<T, failure::Error>;

pub use self::client::ClientConnector;
pub use self::packet::Packet;
pub use self::param::{BoundImpl, ConnectorParam};
pub use self::server::ServerConnector;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::num::NonZeroU64;

/// Contains data about the sending half of this connector
#[derive(Debug)]
struct ConnectorSend<TParam: ConnectorParam> {
    /// Contains a list of messages that are send but are not confirmed yet.
    unconfirmed_message_cache: HashMap<NonZeroU64, CachedPacket<TParam::TSend>>,

    /// Contains the last Id that was send to the peer connector.
    next_message_id: Option<NonZeroU64>,

    /// Last time a ping was send
    last_ping_time: f64,
}

impl<TParam: ConnectorParam> Default for ConnectorSend<TParam> {
    fn default() -> Self {
        ConnectorSend {
            unconfirmed_message_cache: HashMap::new(),
            next_message_id: None,
            last_ping_time: 0.,
        }
    }
}

/// Contains data about the receiving half of this connector
#[derive(Debug)]
struct ConnectorReceive {
    /// Contains the last ID we've received from the peer.
    last_message_id: Option<NonZeroU64>,

    /// Contains the IDs that we are requesting from the peer.
    missing_message_id_list: Vec<MissingId>,

    /// Last time a ping was received
    last_ping_time: f64,
}

impl Default for ConnectorReceive {
    fn default() -> Self {
        ConnectorReceive {
            last_message_id: None,
            missing_message_id_list: Vec::new(),
            last_ping_time: 0.,
        }
    }
}

pub struct Connector<TParam: ConnectorParam> {
    /// Contains data about the sending half of this connector
    send: ConnectorSend<TParam>,

    /// Contains data about the receiving half of this connector
    receive: ConnectorReceive,

    /// The address that this connector is associated with
    peer_addr: SocketAddr,
    // /// Additional data stored in this Connector
    // data: TParam::TData,
}

#[derive(Debug)]
struct MissingId {
    pub id: NonZeroU64,
    pub last_request_time: f64,
}

#[derive(Debug)]
struct CachedPacket<TSend> {
    pub packet: Packet<TSend>,
    pub last_emit_time: f64,
}

#[derive(Debug, Eq, PartialEq)]
pub enum NetworkState {
    Connected,
    Disconnected,
    Connecting,
}

impl MissingId {
    pub fn new(id: NonZeroU64) -> MissingId {
        MissingId {
            id,
            last_request_time: 0.,
        }
    }
}

pub trait Socket {
    fn recv_from(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
    fn local_addr(&self) -> SocketAddr;
    fn send_to(&mut self, buffer: &[u8], target: SocketAddr) -> Result<()>;
}

impl Socket for UdpSocket {
    fn recv_from(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, buffer)
    }
    fn local_addr(&self) -> SocketAddr {
        UdpSocket::local_addr(self).unwrap()
    }
    fn send_to(&mut self, buffer: &[u8], target: SocketAddr) -> Result<()> {
        UdpSocket::send_to(self, buffer, target)?;
        Ok(())
    }
}

impl<TParam: ConnectorParam> Connector<TParam> {
    pub fn bound_to(peer_addr: SocketAddr) -> Self {
        Connector {
            send: Default::default(),
            receive: Default::default(),
            peer_addr,
        }
    }
    pub fn bound_addr(&self) -> SocketAddr {
        self.peer_addr
    }
    pub fn connect(&mut self, socket: &mut Socket) -> Result<()> {
        self.send = Default::default();
        self.receive = Default::default();
        self.send_ping(socket)
    }
    pub fn state(&self) -> NetworkState {
        if self.receive.last_ping_time + 2. < time::precise_time_s() {
            if self.send.last_ping_time + 2. > time::precise_time_s() {
                NetworkState::Connecting
            } else {
                NetworkState::Disconnected
            }
        } else {
            NetworkState::Connected
        }
    }

    pub fn receive_from(&mut self, socket: &mut Socket) -> Result<Vec<TParam::TReceive>> {
        let mut buffer = [0u8; 1024];
        let mut result = Vec::new();
        let mut had_message = false;
        loop {
            let receive_result = socket.recv_from(&mut buffer);
            let count = match receive_result {
                Ok((_, addr)) if addr != self.peer_addr => continue, // ignored
                Ok((count, _)) if count == 0 => {
                    if !had_message {
                        return Err(std::io::Error::from(ErrorKind::BrokenPipe).into());
                    } else {
                        return Ok(result);
                    }
                }
                Ok((count, _)) => count,
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => return Ok(result),
                Err(e) => return Err(e.into()),
            };
            had_message = true;
            if let Some(msg) = self.handle_incoming_data(socket, &buffer[..count])? {
                result.push(msg);
            }
        }
    }

    pub fn update_and_receive(&mut self, socket: &mut Socket) -> Result<Vec<TParam::TReceive>> {
        self.update(socket)?;
        self.receive_from(socket)
    }

    pub fn update(&mut self, socket: &mut Socket) -> Result<()> {
        if NetworkState::Disconnected == self.state() {
            return Ok(());
        }
        if self.send.last_ping_time + 1. < time::precise_time_s() {
            self.send_ping(socket)?;
        }
        for missing_packet in &mut self.receive.missing_message_id_list {
            if missing_packet.last_request_time + 1. < time::precise_time_s() {
                send_packet_to::<TParam::TSend>(
                    self.peer_addr,
                    socket,
                    &Packet::RequestPacket {
                        id: missing_packet.id,
                    },
                )?;
                missing_packet.last_request_time = time::precise_time_s();
            }
        }
        for unconfirmed_packet in self.send.unconfirmed_message_cache.values_mut() {
            if unconfirmed_packet.last_emit_time + 1. < time::precise_time_s() {
                unconfirmed_packet.last_emit_time = time::precise_time_s();
                send_packet_to(self.peer_addr, socket, &unconfirmed_packet.packet)?;
            }
        }
        Ok(())
    }

    pub fn handle_incoming_data(
        &mut self,
        socket: &mut Socket,
        data: &[u8],
    ) -> Result<Option<TParam::TReceive>> {
        let packet: Packet<_> = bincode::deserialize(data)?;
        println!("Received {:?}", packet);
        Ok(match packet {
            Packet::Ping {
                last_send_message_id,
            } => {
                if let Some(last_send_message_id) = last_send_message_id {
                    self.request_message_up_to(last_send_message_id.get());
                }
                self.receive.last_ping_time = time::precise_time_s();
                send_packet_to::<TParam::TSend>(
                    self.peer_addr,
                    socket,
                    &Packet::Pong {
                        last_send_message_id: self.send.next_message_id,
                    },
                )?;
                None
            }
            Packet::RequestPacket { id } => {
                if let Some(packet) = self.send.unconfirmed_message_cache.get_mut(&id) {
                    packet.last_emit_time = time::precise_time_s();
                    send_packet_to(self.peer_addr, socket, &packet.packet)?;
                } else {
                    send_packet_to::<TParam::TSend>(
                        self.peer_addr,
                        socket,
                        &Packet::PacketNotFound { id },
                    )?;
                }
                None
            }
            Packet::ConfirmPacket { id } => {
                self.send.unconfirmed_message_cache.remove(&id);
                None
            }
            Packet::PacketNotFound { id } => {
                self.receive.missing_message_id_list.retain(|i| i.id != id);
                None
            }
            Packet::Pong {
                last_send_message_id,
            } => {
                if let Some(last_send_message_id) = last_send_message_id {
                    self.request_message_up_to(last_send_message_id.get());
                }
                self.receive.last_ping_time = time::precise_time_s();
                None
            }
            Packet::Data { message_id, data } => {
                if let Some(message_id) = message_id {
                    self.request_message_up_to(message_id.get() - 1);
                    send_packet_to::<TParam::TSend>(
                        self.peer_addr,
                        socket,
                        &Packet::ConfirmPacket { id: message_id },
                    )?;
                }
                self.receive.last_message_id = message_id;
                Some(data)
            }
        })
    }

    fn send_ping(&mut self, socket: &mut Socket) -> Result<()> {
        self.send.last_ping_time = time::precise_time_s();
        send_packet_to::<TParam::TSend>(
            self.peer_addr,
            socket,
            &Packet::Ping {
                last_send_message_id: self
                    .send
                    .next_message_id
                    .map(|id| unsafe { NonZeroU64::new_unchecked(id.get() - 1) }),
            },
        )
    }

    fn request_message_up_to(&mut self, id: u64) {
        let mut start = if let Some(id) = self.receive.last_message_id {
            id
        } else {
            unsafe { NonZeroU64::new_unchecked(1) }
        };
        while start.get() <= id {
            if self
                .receive
                .missing_message_id_list
                .iter()
                .find(|id| id.id == start)
                .is_none()
            {
                self.receive
                    .missing_message_id_list
                    .push(MissingId::new(start));
            }
            start = unsafe { NonZeroU64::new_unchecked(start.get() + 1) };
        }
        self.receive.last_message_id = NonZeroU64::new(id);
    }

    pub fn send_unconfirmed(&mut self, socket: &mut Socket, msg: TParam::TSend) -> Result<()> {
        send_packet_to(
            self.peer_addr,
            socket,
            &Packet::Data {
                data: msg,
                message_id: None,
            },
        )?;
        Ok(())
    }

    pub fn send_confirmed(&mut self, socket: &mut Socket, msg: TParam::TSend) -> Result<()> {
        let sending_id = if let Some(id) = self.send.next_message_id {
            id
        } else {
            unsafe { NonZeroU64::new_unchecked(1) }
        };
        let data = Packet::Data {
            data: msg,
            message_id: Some(sending_id),
        };
        send_packet_to(self.peer_addr, socket, &data)?;
        self.send.unconfirmed_message_cache.insert(
            sending_id,
            CachedPacket {
                packet: data,
                last_emit_time: time::precise_time_s(),
            },
        );
        self.send.next_message_id = NonZeroU64::new(sending_id.get() + 1);
        Ok(())
    }
}

pub fn send_packet_to<TSend: serde::Serialize>(
    peer_addr: SocketAddr,
    socket: &mut Socket,
    packet: &Packet<TSend>,
) -> Result<()> {
    let bytes = bincode::serialize(packet)?;
    println!(
        "Sending {:?} from {:?} to {:?}",
        bytes,
        socket.local_addr(),
        peer_addr
    );
    socket.send_to(&bytes, peer_addr)?;
    Ok(())
}
