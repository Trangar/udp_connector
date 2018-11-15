use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
pub enum Packet<TContent> {
    Ping {
        last_send_message_id: Option<NonZeroU64>,
    },
    Pong {
        last_send_message_id: Option<NonZeroU64>,
    },
    PacketNotFound {
        id: NonZeroU64,
    },
    RequestPacket {
        id: NonZeroU64,
    },
    ConfirmPacket {
        id: NonZeroU64,
    },
    Data {
        message_id: Option<NonZeroU64>,
        #[serde(bound(deserialize = "TContent: Serialize + for<'a> Deserialize<'a>"))]
        data: TContent,
    },
}
