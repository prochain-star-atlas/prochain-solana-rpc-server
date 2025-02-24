use std::{net::{IpAddr, Ipv4Addr, SocketAddr}, sync::Arc};
use solana_client::rpc_client::RpcClient;

use crate::{rpc::rpc_service::{JsonRpcConfig, JsonRpcService}, solana_state::SolanaStateManager};

pub const MAX_MULTIPLE_ACCOUNTS: usize = 100;

pub async fn run(json_rpc_config: JsonRpcConfig, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {


    tokio::spawn(async move {

        let local_state = state.clone();
        let local_sol_client = sol_client.clone();

        loop {

            log::info!("starting rpc server ...");

            let rpc_bind_address = Ipv4Addr::new(0, 0, 0, 0);
        
            let rpc_port: u16 = 14555;
            let rpc_addr = SocketAddr::new(IpAddr::V4(rpc_bind_address), rpc_port);
        
            
        
            let json_rpc_service = JsonRpcService::new(rpc_addr, json_rpc_config.clone(), local_state.clone(), local_sol_client.clone());
            json_rpc_service.join().unwrap();

        }

    });

}