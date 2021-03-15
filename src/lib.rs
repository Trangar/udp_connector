#![deny(missing_docs)]
//! A generic eventually-correct framework over unreliable connections, such as UDP.
//!
//! This crate uses a struct called `Connector` that eventually guarantees that all persistent messages
//! arrive at the other end, or it will disconnect.
//!
//! This crate differentiates between two message types:
//! * Confirmed: This is a message that is:
//! * * Guaranteed to arrive ***at some point***
//! * * Not guaranteed to arrive in the correct order
//! * Unconfirmed: This is a message that is not guaranteed to arrive
//!
//! Use cases can be:
//! * Sending player data does not always have to arrive, because the location is updated 10 times a second (unconfirmed)
//! * Login information should always arrive, but this can take a second (confirmed)

#[macro_use]
extern crate serde_derive;

mod packet;
mod param;

#[cfg(test)]
pub mod test;

/// The result that is used in this type. It is a simple wrapper around `Result<T, failure::Error>`
pub type Result<T> = std::result::Result<T, failure::Error>;

use self::packet::Packet;
pub use self::param::ConnectorParam;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::{num::NonZeroU64, time::Instant};

/// Contains data about the sending half of this connector
#[derive(Debug)]
struct ConnectorSend<TParam: ConnectorParam> {
    /// Contains a list of messages that are send but are not confirmed yet.
    unconfirmed_message_cache: HashMap<NonZeroU64, CachedPacket<TParam::TSend>>,

    /// Contains the last Id that was send to the peer connector.
    next_message_id: Option<NonZeroU64>,

    /// Last time a ping was send
    last_ping: Instant,
}

impl<TParam: ConnectorParam> Default for ConnectorSend<TParam> {
    fn default() -> Self {
        ConnectorSend {
            unconfirmed_message_cache: HashMap::new(),
            next_message_id: None,
            last_ping: Instant::now(),
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
    last_ping: Instant,
}

impl Default for ConnectorReceive {
    fn default() -> Self {
        ConnectorReceive {
            last_message_id: None,
            missing_message_id_list: Vec::new(),
            last_ping: Instant::now(),
        }
    }
}

/// The connector is used to handle handshakes and timeouts with a different, remote connector
///
/// For client-side applications, we recommend calling `update_and_receive` at a frequent rate
///
/// For server-side applications, we recommend dealing with your own UdpSocket receiving logic, looking up the connector based on a SocketAddr, and then calling `handle_incoming_data`.
///
/// The connector struct has a lot of config settings. All these settings can be found in `ConnectorParam`
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
    pub last_request: Instant,
}

#[derive(Debug)]
struct CachedPacket<TSend> {
    pub packet: Packet<TSend>,
    pub last_emit: Instant,
}

/// The state of the connector. This is based on when the last ping was send or received. Changing your ConnectorParam will greatly affect the results of `Connector.state()`, returning this value.
#[derive(Debug, Eq, PartialEq)]
pub enum NetworkState {
    /// We received a ping a reasonable amount of time ago, so we're connected. See `ConnectorParam::PING_INTERVAL_S` for more info.
    Connected,

    /// We have not received a ping for a while, and we are not connecting at this point in time. See `ConnectorParam::RECEIVE_PING_TIMEOUT_S` for more info.
    Disconnected,

    /// We have not received a ping for a while but we did try to connect. See `ConnectorParam::SEND_PING_TIMEOUT_S` for more info.
    Connecting,
}

impl MissingId {
    pub fn new(id: NonZeroU64) -> MissingId {
        MissingId {
            id,
            last_request: Instant::now(),
        }
    }
}

/// A generic trait over a socket. This is automatically implemented for `UdpSocket` but can be implemented for your own connector as well.
pub trait Socket {
    /// Receive data from any remote, returning the amount of bytes read, and the SocketAddr that the data was received from
    fn recv_from(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;

    /// The local SocketAddr we're listening on
    fn local_addr(&self) -> SocketAddr;

    /// Send data to the given SocketAddr
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
    /// Create a Connector that is bound to the given remote SocketAddr
    pub fn bound_to(peer_addr: SocketAddr) -> Self {
        Connector {
            send: Default::default(),
            receive: Default::default(),
            peer_addr,
        }
    }

    /// Get the socket address that this connector is paired with
    pub fn bound_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    /// Connect to the `bound_addr`. This will reset the internal state of the connector, and start up the connection handshake
    pub fn connect(&mut self, socket: &mut dyn Socket) -> Result<()> {
        self.send = Default::default();
        self.receive = Default::default();
        self.send_ping(socket)
    }

    /// Get the current state of this connector. This is dependent on a couple of settings in ConnectorParam:
    /// * If we have received a ping since `ConnectorParam::RECEIVE_PING_TIMEOUT_S` ago, we're connected
    /// * If we have send a ping since `ConnectorParam::SEND_PING_TIMEOUT_S` ago, we're connecting
    /// * Else we're disconnected
    pub fn state(&self) -> NetworkState {
        if self.receive.last_ping.elapsed().as_secs_f64() > TParam::RECEIVE_PING_TIMEOUT_S {
            if self.send.last_ping.elapsed().as_secs_f64() > TParam::SEND_PING_TIMEOUT_S {
                NetworkState::Connecting
            } else {
                NetworkState::Disconnected
            }
        } else {
            NetworkState::Connected
        }
    }

    /// Receive data from the other connector. This will call `handle_incoming_data` internally.
    ///
    /// Ideally you would never need this function. Use `update_and_receive` on clients, and `handle_incoming_data` on servers.
    pub fn receive_from(&mut self, socket: &mut dyn Socket) -> Result<Vec<TParam::TReceive>> {
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

    /// Update this connector and receive data from the remote connector.
    pub fn update_and_receive(&mut self, socket: &mut dyn Socket) -> Result<Vec<TParam::TReceive>> {
        self.update(socket)?;
        self.receive_from(socket)
    }

    /// Update this connector. This will make sure the connection is still intact and requests any potentially missing packets.
    pub fn update(&mut self, socket: &mut dyn Socket) -> Result<()> {
        if NetworkState::Disconnected == self.state() {
            return Ok(());
        }
        if self.send.last_ping.elapsed().as_secs_f64() > TParam::PING_INTERVAL_S {
            self.send_ping(socket)?;
        }
        for missing_packet in &mut self.receive.missing_message_id_list {
            if missing_packet.last_request.elapsed().as_secs_f64()
                > TParam::REQUEST_MISSING_PACKET_INTERVAL_S
            {
                send_packet_to::<TParam::TSend>(
                    self.peer_addr,
                    socket,
                    &Packet::RequestPacket {
                        id: missing_packet.id,
                    },
                )?;
                missing_packet.last_request = Instant::now();
            }
        }
        for unconfirmed_packet in self.send.unconfirmed_message_cache.values_mut() {
            if unconfirmed_packet.last_emit.elapsed().as_secs_f64()
                > TParam::EMIT_UNCONFIRMED_PACKET_INTERVAL_S
            {
                unconfirmed_packet.last_emit = Instant::now();
                send_packet_to(self.peer_addr, socket, &unconfirmed_packet.packet)?;
            }
        }
        Ok(())
    }

    /// Resolve an incoming ping or ping.
    /// This will request all the messages up to this message, as well as set the last received time.
    fn resolve_incoming_ping(&mut self, id: Option<NonZeroU64>) {
        if let Some(last_send_message_id) = id {
            self.request_message_up_to(last_send_message_id.get());
        }
        self.receive.last_ping = Instant::now();
    }

    /// Handles incoming data. This will perform internal logic to make sure data is being transmitted correctly,
    /// and requests missing packets.
    ///
    /// Any actual data that was received, will be returned from this function.
    pub fn handle_incoming_data(
        &mut self,
        socket: &mut dyn Socket,
        data: &[u8],
    ) -> Result<Option<TParam::TReceive>> {
        let packet: Packet<_> = bincode::deserialize(data)?;
        Ok(match packet {
            Packet::Ping {
                last_send_message_id,
            } => {
                self.resolve_incoming_ping(last_send_message_id);
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
                    packet.last_emit = Instant::now();
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
                self.resolve_incoming_ping(last_send_message_id);
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

    fn send_ping(&mut self, socket: &mut dyn Socket) -> Result<()> {
        self.send.last_ping = Instant::now();
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

    /// Send an unconfirmed message to the other connector. It is not guaranteed that this message will ever arrive.
    ///
    /// This is useful for data that does not have to arrive. Think of things like player movements, frames of a lossy video stream, etc.
    pub fn send_unconfirmed<T: Into<TParam::TSend>>(
        &mut self,
        socket: &mut dyn Socket,
        msg: T,
    ) -> Result<()> {
        send_packet_to(
            self.peer_addr,
            socket,
            &Packet::Data {
                data: msg.into(),
                message_id: None,
            },
        )?;
        Ok(())
    }

    /// Send a confirmed message to the other connector. The connector will try to make sure this message arrives. It is not guaranteed that messages will arrive in the same order at the other side.
    pub fn send_confirmed<T: Into<TParam::TSend>>(
        &mut self,
        socket: &mut dyn Socket,
        msg: T,
    ) -> Result<()> {
        let sending_id = if let Some(id) = self.send.next_message_id {
            id
        } else {
            unsafe { NonZeroU64::new_unchecked(1) }
        };
        let data = Packet::Data {
            data: msg.into(),
            message_id: Some(sending_id),
        };
        send_packet_to(self.peer_addr, socket, &data)?;
        self.send.unconfirmed_message_cache.insert(
            sending_id,
            CachedPacket {
                packet: data,
                last_emit: Instant::now(),
            },
        );
        self.send.next_message_id = NonZeroU64::new(sending_id.get() + 1);
        Ok(())
    }
}

fn send_packet_to<TSend: serde::Serialize>(
    peer_addr: SocketAddr,
    socket: &mut dyn Socket,
    packet: &Packet<TSend>,
) -> Result<()> {
    let bytes = bincode::serialize(packet)?;
    socket.send_to(&bytes, peer_addr)?;
    Ok(())
}
