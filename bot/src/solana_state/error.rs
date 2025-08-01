use thiserror::Error;

#[derive(Error, Debug)]
pub enum StateSpaceError {
    #[error("Error wallet")]
    WalletError,
    #[error("Insufficient wallet funds for execution")]
    InsufficientWalletFunds(),
    #[error(transparent)]
    StateChangeError(#[from] StateChangeError),
    #[error("Block number not found")]
    BlockNumberNotFound,
    #[error("Already listening for state changes")]
    AlreadyListeningForStateChanges,
    #[error(transparent)]
    JoinError(#[from] tokio::task::JoinError),
}

#[derive(Error, Debug)]
pub enum StateChangeError {
    #[error("No state changes in cache")]
    NoStateChangesInCache,
    #[error("Error when removing a state change from the front of the deque")]
    PopFrontError,
    #[error("State change cache capacity error")]
    CapacityError
}
