
use flate2::bufread::ZlibDecoder;
use hashbrown::HashSet;
use serde::{Deserialize, Serialize};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;
use dashmap::DashMap;
use rust_decimal::prelude::*;
use crate::oracles::handle_user_address_oracle::add_user_address_to_index;
use crate::solana_state::SolanaStateManager;
use crate::utils::types::events;
use std::sync::Arc;
use std::ops::Mul;
use std::io::prelude::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StarAtlasCoin {
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub mint: Option<String>,
    pub network: Option<String>,
}

pub async fn create_token_list(state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {

    let response = reqwest::get("https://galaxy.staratlas.com/nfts")
        .await.unwrap().json::<Vec<StarAtlasCoin>>()
        .await;

    if response.is_err() {
        log::error!("error: {}", response.err().unwrap().to_string())
    } else {
        let response_atlas = response.unwrap();

        let mut vec_pk = vec![];
    
        log::info!("initializing star atlas tokens: {}", response_atlas.len());
    
        let _1 = response_atlas.iter().for_each(|f| {
    
            let mint_pk = f.mint.clone();

            if mint_pk.is_some() {
                let pk = Pubkey::try_from(mint_pk.unwrap().as_str());
    
                if pk.is_ok() {
                    let pk_ok = pk.unwrap();

                    add_user_address_to_index(pk_ok.clone(), state.clone(), sol_client.clone());

                    vec_pk.push(pk_ok.to_string());
                }
            }
    
        });
    
        crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_pk.clone());

        log::info!("initialized star atlas tokens: {}", vec_pk.clone().len());
    }

}