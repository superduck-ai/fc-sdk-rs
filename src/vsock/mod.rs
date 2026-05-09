mod dial;
mod listener;

pub use dial::{
    AckError, ConnectMessageError, DialConfig, DialOption, connect_message, dial, dial_with_config,
    dial_with_options, is_temporary_net_error, with_ack_msg_timeout, with_connection_msg_timeout,
    with_dial_timeout, with_retry_interval, with_retry_timeout,
};
pub use listener::{VsockListener, VsockStream, listen, listen_with_config, listen_with_options};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VsockDevice {
    pub id: String,
    pub path: String,
    pub cid: u32,
}
