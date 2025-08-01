use std::sync::Arc;
use crate::utils::types::events::ShutdownEvent;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
use tokio::sync::broadcast::Receiver;

// Holds all oracles for the bot
#[derive(Debug, Clone)]
pub struct Bot {
    pub shutdown_event: Arc<(Sender<ShutdownEvent>, Receiver<ShutdownEvent>)>
}

impl Bot {

    // creates a new instance of bot holding the oracles
    pub fn new(
    ) -> Self {
        Bot {
            shutdown_event: Arc::new(broadcast::channel::<ShutdownEvent>(1000))
        }
    }
    
}
