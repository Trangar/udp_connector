use crate::*;
use std::io::ErrorKind;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

impl Socket for TcpStream {
    fn recv_from(&mut self, buffer: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let count = self.read(buffer)?;
        Ok((count, self.peer_addr().unwrap()))
    }
    fn local_addr(&self) -> SocketAddr {
        TcpStream::local_addr(self).unwrap()
    }
    fn send_to(&mut self, buffer: &[u8], target: SocketAddr) -> Result<()> {
        assert_eq!(target, self.peer_addr().unwrap());
        TcpStream::write(self, buffer)?;
        Ok(())
    }
}

pub struct ServerConnector {
    pub connector: Connector<Server>,
    pub socket: TcpStream,
}

pub struct ClientConnector {
    pub connector: Connector<Client>,
    pub socket: TcpStream,
}

pub struct Server;
impl ConnectorParam for Server {
    type TReceive = ClientToServer;
    type TSend = ServerToClient;
}

pub struct Client;
impl ConnectorParam for Client {
    type TSend = ClientToServer;
    type TReceive = ServerToClient;
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ServerToClient {}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ClientToServer {
    SendMessage { name: String },
}

pub struct Proxy {
    pub server: ServerConnector,
    pub client: ClientConnector,
    server_socket: TcpStream,
    client_socket: TcpStream,
}

fn get_tcp_stream_pair(listener: &TcpListener) -> (TcpStream, TcpStream) {
    let socket =
        TcpStream::connect(listener.local_addr().unwrap()).expect("Could not connect to listener");
    let (other_socket, addr) = listener.accept().expect("Could not accept socket");
    assert_eq!(socket.local_addr().unwrap(), addr);

    socket
        .set_nonblocking(true)
        .expect("Could not set non-blocking");
    other_socket
        .set_nonblocking(true)
        .expect("Could not set non-blocking");

    (socket, other_socket)
}
impl Default for Proxy {
    fn default() -> Proxy {
        const LOCALHOST: &str = "127.0.0.1:0";

        let listener = TcpListener::bind(LOCALHOST).expect("Could not create listener");

        let server_socket_pair = get_tcp_stream_pair(&listener);
        let client_socket_pair = get_tcp_stream_pair(&listener);

        let server = ServerConnector {
            connector: Connector::bound_to(server_socket_pair.1.local_addr().unwrap()),
            socket: server_socket_pair.0,
        };
        let client = ClientConnector {
            connector: Connector::bound_to(client_socket_pair.1.local_addr().unwrap()),
            socket: client_socket_pair.0,
        };

        for (name, socket) in &[
            ("Client socket", &client.socket),
            ("Client proxy", &client_socket_pair.1),
            ("Server proxy", &server_socket_pair.1),
            ("Server socket", &server.socket),
        ] {
            println!("{} bound on {:?}", name, socket.local_addr().unwrap());
        }

        let mut proxy = Proxy {
            server,
            client,
            server_socket: server_socket_pair.1,
            client_socket: client_socket_pair.1,
        };

        proxy
            .client
            .connector
            .connect(&mut proxy.client.socket)
            .expect("Could not connect to server");

        assert_eq!(NetworkState::Connected, proxy.client.connector.state());

        let message = proxy.handle_one_message_from_client();
        assert_eq!(
            Packet::Ping {
                last_send_message_id: None
            },
            message
        );

        println!("server receiving from");
        proxy
            .server
            .connector
            .receive_from(&mut proxy.server.socket)
            .expect("Could not update server");
        println!("Handling one server message");
        let message = proxy.handle_one_message_from_server();
        assert_eq!(
            Packet::Pong {
                last_send_message_id: None
            },
            message
        );

        proxy
            .client
            .connector
            .update_and_receive(&mut proxy.client.socket)
            .expect("Could not update client");

        assert!(proxy.client_has_no_pending_messages());
        assert!(proxy.server_has_no_pending_messages());
        assert_eq!(NetworkState::Connected, proxy.client.connector.state());

        proxy
    }
}

impl Proxy {
    pub fn handle_one_message_from_client(&mut self) -> Packet<ClientToServer> {
        thread::sleep(Duration::from_millis(100));
        println!(
            "Reading data from {:?}",
            self.client_socket.local_addr().unwrap()
        );
        let mut data = [0u8; 1024];
        let (count, addr) = self
            .client_socket
            .recv_from(&mut data)
            .expect("Could not receive data from client");
        assert_eq!(self.client.socket.local_addr().unwrap(), addr);
        assert!(count != 0);
        let packet: Packet<ClientToServer> =
            bincode::deserialize(&data[..count]).expect("Could not deserialize packet");

        println!(
            " - Relaying to {:?} (-> {:?})",
            self.server_socket.local_addr().unwrap(),
            self.server.socket.local_addr().unwrap(),
        );

        self.server_socket
            .send_to(&data[..count], self.server.socket.local_addr().unwrap())
            .expect("Could not relay message to server");

        thread::sleep(Duration::from_millis(100));
        packet
    }
    pub fn handle_one_message_from_server(&mut self) -> Packet<ServerToClient> {
        thread::sleep(Duration::from_millis(100));
        println!(
            "Reading data from {:?}",
            self.server_socket.local_addr().unwrap()
        );
        let mut data = [0u8; 1024];
        let (count, _addr) = self
            .server_socket
            .recv_from(&mut data)
            .expect("Could not receive data from server");
        assert!(count != 0);
        let packet: Packet<ServerToClient> =
            bincode::deserialize(&data[..count]).expect("Could not deserialize packet");
        println!(
            " - Relaying to {:?} (-> {:?})",
            self.client_socket.local_addr().unwrap(),
            self.client.socket.local_addr().unwrap(),
        );

        self.client_socket
            .send_to(&data[..count], self.client.socket.local_addr().unwrap())
            .expect("Could not relay message to client");

        thread::sleep(Duration::from_millis(100));
        packet
    }

    pub fn client_has_no_pending_messages(&mut self) -> bool {
        let mut data = [0u8; 1024];
        match self.client_socket.recv_from(&mut data) {
            Ok((count, _)) => {
                println!("Buffer left in client socket: {:?}", &data[..count]);
                false
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => true,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
    pub fn server_has_no_pending_messages(&mut self) -> bool {
        let mut data = [0u8; 1024];
        match self.server_socket.recv_from(&mut data) {
            Ok((count, _)) => {
                println!("Buffer left in client socket: {:?}", &data[..count]);
                false
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => true,
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
}
