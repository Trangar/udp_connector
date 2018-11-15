mod proxy;

use self::proxy::{ClientToServer, Proxy};
use crate::*;
use std::num::NonZeroU64;
use std::thread;
use std::time::Duration;

#[test]
fn test_timeout() {
    let mut proxy = Proxy::default();

    // Client-server should start connected (else the test_setup failed)
    assert_eq!(NetworkState::Connected, proxy.client.connector.state());
    thread::sleep(Duration::from_secs(3));

    // Client has not received a message in 5 seconds.
    // This means the client should update it's state to Disconnected
    let result = proxy
        .client
        .connector
        .update_and_receive(&mut proxy.client.socket)
        .expect("Could not update client");
    assert!(result.is_empty());
    assert_eq!(NetworkState::Disconnected, proxy.client.connector.state());

    // Attempt to reconnect
    proxy
        .client
        .connector
        .connect(&mut proxy.client.socket)
        .expect("Could not reconnect");
    assert_eq!(NetworkState::Connecting, proxy.client.connector.state());
    let message = proxy.handle_one_message_from_client();
    assert_eq!(
        Packet::Ping {
            last_send_message_id: None
        },
        message
    );

    // Server needs to be polled to answer this message
    let result = proxy
        .server
        .connector
        .receive_from(&mut proxy.server.socket)
        .expect("Could not update server");
    assert!(result.is_empty());
    let message = proxy.handle_one_message_from_server();
    assert_eq!(
        Packet::Pong {
            last_send_message_id: None
        },
        message
    );

    // Client needs to receive this message
    assert_eq!(NetworkState::Connecting, proxy.client.connector.state());
    proxy
        .client
        .connector
        .update_and_receive(&mut proxy.client.socket)
        .expect("Could not update client");

    // Now they should be connected again
    assert_eq!(NetworkState::Connected, proxy.client.connector.state());
    assert!(proxy.client_has_no_pending_messages());
    assert!(proxy.server_has_no_pending_messages());
}

#[test]
fn test_confirmed_message() {
    let mut proxy = Proxy::default();

    proxy
        .client
        .connector
        .send_confirmed(
            &mut proxy.client.socket,
            ClientToServer::SendMessage {
                name: String::from("test"),
            },
        )
        .expect("Could not send message");

    let message = proxy.handle_one_message_from_client();
    assert_eq!(
        Packet::Data {
            message_id: NonZeroU64::new(1),
            data: ClientToServer::SendMessage {
                name: String::from("test"),
            }
        },
        message
    );

    let message = proxy
        .server
        .connector
        .receive_from(&mut proxy.server.socket)
        .expect("Could not receive from server");

    assert_eq!(1, message.len());

    assert_eq!(
        ClientToServer::SendMessage {
            name: String::from("test"),
        },
        message[0]
    );

    let message = proxy.handle_one_message_from_server();
    assert_eq!(
        Packet::ConfirmPacket {
            id: unsafe { NonZeroU64::new_unchecked(1) },
        },
        message
    );

    assert!(proxy.client_has_no_pending_messages());
    assert!(proxy.server_has_no_pending_messages());
}
