[Documentation](https://docs.rs/udp_connector)

Provides a reasonable guarantee that messages being send over UDP arrive at the other connector.

This crate is a work in progress and should not be used in production yet.

This crate uses a struct called `Connector` that eventually guarantees that all persistent messages
arrive at the other end, or it will disconnect.

This crate differentiates between two message types:
* Confirmed: This is a message that is:
* * Guaranteed to arrive ***at some point***
* * Not guaranteed to arrive in the correct order
* Unconfirmed: This is a message that is not guaranteed to arrive

Use cases can be:
* Sending player data does not always have to arrive, because the location is updated 10 times a second (unconfirmed)
* Login information should always arrive, but this can take a second (confirmed)
