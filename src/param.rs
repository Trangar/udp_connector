use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub trait ConnectorParam {
    type TSend: for<'a> Deserialize<'a> + Serialize + Debug + Eq + PartialEq;
    type TReceive: for<'a> Deserialize<'a> + Serialize + Debug + Eq + PartialEq;
    // type TData: BoundImpl;
}

pub trait BoundImpl {
    fn ping_received(&mut self);
}
