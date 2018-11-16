use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub trait ConnectorParam {
    type TSend: for<'a> Deserialize<'a> + Serialize + Debug + Eq + PartialEq;
    type TReceive: for<'a> Deserialize<'a> + Serialize + Debug + Eq + PartialEq;

    const CONNECTION_TIMEOUT_S: f64 = 1.;
    const REQUEST_MISSING_PACKET_INTERVAL_S: f64 = 1.;
    const EMIT_UNCONFIRMED_PACKET_INTERVAL_S: f64 = 1.;

    const RECEIVE_PING_TIMEOUT_S: f64 = Self::CONNECTION_TIMEOUT_S * 2.;
    const SEND_PING_TIMEOUT_S: f64 = Self::CONNECTION_TIMEOUT_S * 2.;
}

pub trait BoundImpl {
    fn ping_received(&mut self);
}
