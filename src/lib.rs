pub mod client;
mod commands;
mod errors;
mod messaging;
pub mod transport;
pub mod types;

pub use client::Client;
pub use errors::McpError;
pub use types::{NotificationHandler, SamplingHandler};

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
