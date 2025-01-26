pub mod transport;
pub mod types;
pub mod client;
mod commands;
mod messaging;

pub use client::Client;
pub use types::{SamplingHandler, NotificationHandler};

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
