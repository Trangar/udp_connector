use serde::{Deserialize, Serialize};

/// Settings that are set up for a Connector. This can be used to tweak your Connector at compile-time
pub trait ConnectorParam {
    /// The type that this connector will be sending. This is usually an enum.
    ///
    /// ```rust
    /// # #[macro_use]
    /// # extern crate serde_derive;
    /// # extern crate serde;
    /// # use udp_connector::ConnectorParam;
    ///
    /// // For the server:
    /// #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
    /// pub enum ServerToClient {
    ///     LoginResult { success: bool },
    /// }
    ///
    /// struct ConnectorConfig;
    ///
    ///
    /// impl ConnectorParam for ConnectorConfig {
    ///     type TSend = ServerToClient;
    ///     // Other fields omitted
    ///     # type TReceive = ServerToClient;
    /// }
    /// # fn main() {}
    /// ```
    type TSend: for<'a> Deserialize<'a> + Serialize;

    /// The type that this connector will be sending. This is usually an enum.
    ///
    /// ```rust
    /// # #[macro_use]
    /// # extern crate serde_derive;
    /// # extern crate serde;
    /// # use udp_connector::ConnectorParam;
    ///
    /// # type AuthenticateParams = u32;
    ///
    /// // For the server:
    /// #[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
    /// pub enum ClientToServer {
    ///     Login { params: AuthenticateParams },
    /// }
    ///
    /// struct ConnectorConfig;
    ///
    /// impl ConnectorParam for ConnectorConfig {
    ///     type TReceive = ClientToServer;
    ///     // Other fields omitted
    ///     # type TSend = ClientToServer;
    /// }
    /// # fn main() {}
    /// ```
    type TReceive: for<'a> Deserialize<'a> + Serialize;

    /// The interval at which pings are being emitted to the other connector. This should be set in relation to `RECEIVE_PING_TIMEOUT_S` and `SEND_PING_TIMEOUT_S`, and how often you expect to lose packets.
    const PING_INTERVAL_S: f64 = 0.5;

    /// The interval at which missing packets are being requested from the connector
    const REQUEST_MISSING_PACKET_INTERVAL_S: f64 = 1.;

    /// The interval at which unconfirmed packets are being send to the other connector
    const EMIT_UNCONFIRMED_PACKET_INTERVAL_S: f64 = 1.;

    /// The time that it takes before this connector assumes it has lost connection to the other connector
    const RECEIVE_PING_TIMEOUT_S: f64 = Self::PING_INTERVAL_S * 3.;

    /// The time that it takes before this connector assumes it has lost connection to the other connector
    const SEND_PING_TIMEOUT_S: f64 = Self::PING_INTERVAL_S * 3.;
}
