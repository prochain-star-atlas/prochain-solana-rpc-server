use thiserror::Error;

#[derive(Error, Debug)]
pub enum ExecutionError {
    #[error("Pool Error")]
    PoolError(String),
    #[error("Transaction error")]
    TransactionReceptError(String),
}

#[derive(Error, Debug)]
pub enum OrderBookError {
    #[error("Error during snapshot creation")]
    SnapshotError(String),
    #[error("Error during fetching ob")]
    ObError(String)
}
