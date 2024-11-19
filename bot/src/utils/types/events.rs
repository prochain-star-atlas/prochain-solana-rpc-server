

#[derive(Debug, Clone)]
pub enum ShutdownEvent {
    Shutdown {
        triggered: bool,
    },
}

// New mempool event from the mempool stream
#[derive(Debug, Clone)]
pub enum MemPoolEvent {
    NewTx {
        origin: String,
        vec_trx: Vec<String>
    },
}
