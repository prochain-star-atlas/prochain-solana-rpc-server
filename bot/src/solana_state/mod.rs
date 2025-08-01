pub mod error;

use dashmap::DashMap;
use log::info;
use solana_account::Account;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use utoipa::ToSchema;
use yellowstone_grpc_proto::geyser::SubscribeUpdateAccountInfo;
use core::str;
use parking_lot::{Mutex};
use std::{
    env, str::FromStr, sync::Arc, time::Duration
};
use solana_sdk::{
    account::ReadableAccount, clock::Epoch
};
use static_init::dynamic;

use chrono::Utc;

use crate::{services::subscription_deletion_service::SubscriptionDeletionService, utils::helpers::load_env_vars};

#[dynamic] 
static SOLANA_STATE_ENCAPSULATOR: Arc<SolanaStateManager> = Arc::new(SolanaStateManager::new());

pub fn get_solana_state() -> Arc<SolanaStateManager> {
    let state = SOLANA_STATE_ENCAPSULATOR.clone();
    return state;
}

impl Eq for ProchainAccountInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct ProchainAccountInfo {
    pub pubkey: Pubkey,
    pub lamports: u64,
    pub owner: Pubkey,
    pub executable: bool,
    pub rent_epoch: u64,
    pub data: Vec<u8>,
    pub slot: u64,
    pub write_version: u64,
    pub txn_signature: Option<Vec<u8>>,
    pub last_update: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema, PartialEq)]
pub struct ProchainAccountInfoSchema {
    pub pubkey: String,
    pub lamports: u64,
    pub owner: String,
    pub executable: bool,
    pub rent_epoch: u64,
    pub data: Vec<u8>,
    pub slot: u64,
    pub write_version: u64,
    pub txn_signature: Option<Vec<u8>>,
    pub last_update: chrono::DateTime<Utc>,
}

impl ReadableAccount for ProchainAccountInfo {
    fn lamports(&self) -> u64 {
        self.lamports as u64
    }

    fn data(&self) -> &[u8] {
        &self.data
    }

    fn owner(&self) -> &Pubkey {
        &self.owner
    }

    fn executable(&self) -> bool {
        self.executable
    }

    fn rent_epoch(&self) -> Epoch {
        self.rent_epoch as u64
    }
}

// TODO: bench this with a dashmap
pub type StateSpace = DashMap<Pubkey, ProchainAccountInfo>;
pub type StateOwnerSpace = DashMap<Pubkey, Vec<Pubkey>>;
pub type StateProgramOwnerSpace = DashMap<Pubkey, Vec<Pubkey>>;
pub type SlotSpace = DashMap<String, u64>;
pub type BlockhashSpace = DashMap<String, String>;
pub type BlockHeightSpace = DashMap<String, u64>;

#[derive(Clone)]
pub struct SolanaStateManager {
    state_account: Arc<StateSpace>,
    state_owner: Arc<StateOwnerSpace>,
    state_program_owner: Arc<StateProgramOwnerSpace>,
    slot: Arc<SlotSpace>,
    blockhash: Arc<BlockhashSpace>,
    blockheight: Arc<BlockHeightSpace>,
    sol_client: Arc<RpcClient>
}

impl SolanaStateManager
{
    pub fn new() -> Self {

        let path = env::current_dir().unwrap();
        let _res = load_env_vars(&path);

        let sol_rpc_url = std::env::var("SOL_RPC_URL").unwrap();

        let state_a: DashMap<Pubkey, ProchainAccountInfo> = DashMap::new();
        let state_o: DashMap<Pubkey, Vec<Pubkey>> = DashMap::new();
        let state_p: DashMap<Pubkey, Vec<Pubkey>> = DashMap::new();
        let slot_a: DashMap<String, u64> = DashMap::new();
        let blockhash_b: DashMap<String, String> = DashMap::new();
        let blockheight_b: DashMap<String, u64> = DashMap::new();

        slot_a.insert(String::from("confirmed"), 0);
        blockhash_b.insert(String::from("confirmed"), String::from(""));
        blockheight_b.insert(String::from("confirmed"), 0);

        Self {
            state_account: Arc::new(state_a),
            state_owner: Arc::new(state_o),
            state_program_owner: Arc::new(state_p),
            slot: Arc::new(slot_a),
            blockhash: Arc::new(blockhash_b),
            blockheight: Arc::new(blockheight_b),
            sol_client: Arc::new(solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(sol_rpc_url, Duration::from_secs(240), CommitmentConfig::confirmed()))
        }
    }

    pub fn get_sol_client(&self) -> Arc<RpcClient> {
        return self.sol_client.clone();
    }

    pub fn get_all_account_info_pubkey(&self) -> Vec<Pubkey> {
        let vec_t: Vec<Pubkey> = self.state_account.iter().filter(|p| { p.lamports > 0 }).map(|e| { e.key().clone() }).collect();
        return vec_t;
    }

    pub fn show_owner_vec(&self) {
        info!("state_owner: {:?}", self.state_owner);
    }

    pub fn get_slot(&self) -> u64 {
        return self.slot.get("confirmed").unwrap().value().clone();
    }

    pub fn set_slot(&self, s: u64) {
        self.slot.alter(&String::from("confirmed"), |k, v| {
            return s;
        });
    }

    pub fn get_blockhash(&self) -> String {
        return self.blockhash.get("confirmed").unwrap().value().clone();
    }

    pub fn set_blockhash(&self, s: String) {
        self.blockhash.alter(&String::from("confirmed"), |k, v| {
            return s;
        });
    }

    pub fn get_blockheight(&self) -> u64 {

        if !self.blockheight.contains_key("confirmed") {
            return 0;
        }

        return self.blockheight.get("confirmed").unwrap().value().clone();

    }

    pub fn set_blockheight(&self, s: u64) {
        self.blockheight.alter(&String::from("confirmed"), |k, v| {
            return s;
        });
    }

    pub fn get_accounts_by_owner(&self, addr: Pubkey) -> Option<Vec<Pubkey>> {
        let res = self.state_owner.get(&addr);

        if res.is_some() {
            return Some(res.unwrap().value().clone());
        } else {
            return None;
        }
    }

    pub fn get_account_info(&self, addr: Pubkey) -> Option<Result<ProchainAccountInfo, anyhow::Error>> {
        let res = self.state_account.get(&addr);

        if res.is_some() {

            let cloned_res = res.unwrap().value().clone();
            if cloned_res.lamports == 0 {

                return Some(Err(anyhow::Error::msg("empty lamport")));

            } else {

                return Some(Ok(cloned_res));

            }
            
        } else {

            return None;

        }
    }

    pub fn reset_account_info_map(&self) {

        self.state_account.clear();
        self.state_owner.clear();
        self.state_program_owner.clear();

    }

    pub fn add_account_info(&self, pub_key: Pubkey, acc: ProchainAccountInfo) {
        if self.state_account.contains_key(&pub_key) {
            self.state_account.alter(&pub_key, |k, v| {
                return acc.clone();
            });
        } else {
            self.state_account.insert(pub_key, acc.clone());
        }

        let owner_key = &acc.clone().owner;

        if self.state_owner.contains_key(&owner_key.clone()) {           

            self.state_owner.alter(&acc.clone().owner, |k, mut v| {

                if !v.contains(&pub_key) {
                    v.push(pub_key);
                }

                return v;

            });

        } else {
            let mut vd: Vec<Pubkey> = vec![];
            vd.push(pub_key);
            self.state_owner.insert(acc.clone().owner, vd);
        }

    }

    pub fn add_account_info_without_owner(&self, pub_key: Pubkey, acc: ProchainAccountInfo) {
        if self.state_account.contains_key(&pub_key) {
            self.state_account.alter(&pub_key, |k, v| {
                return acc.clone();
            });
        } else {
            self.state_account.insert(pub_key, acc.clone());
        }
    }

    pub fn clean_zero_account(&self, pub_key: Pubkey) {

        if self.state_account.contains_key(&pub_key) {

            let acc_prop = self.state_account.get(&pub_key).unwrap().value().clone();

            if self.state_owner.contains_key(&acc_prop.owner) {

                if self.state_owner.contains_key(&acc_prop.owner) {

                    self.state_owner.alter(&acc_prop.owner, |k, mut v| {
                        if v.contains(&pub_key) {

                            let index = v.iter().position(|x| *x == pub_key).unwrap();
                            v.remove(index);
                        }
                        return v;
                    });

                }

            }

            let _1 = self.state_account.remove(&pub_key);

        }

    }

    pub fn handle_account_update(&self, pubkey: Pubkey, account: Account) {

        let tt = ProchainAccountInfo {
            pubkey: pubkey.clone(),
            lamports: account.lamports,
            executable: account.executable,
            owner: account.owner.clone(),
            rent_epoch: account.rent_epoch,
            slot: 0,
            write_version: 0,
            txn_signature: None,
            data: account.data,
            last_update: chrono::offset::Utc::now()
        };

        if tt.lamports == 0 {
            self.clean_zero_account(pubkey.clone());
        } else {
            if self.state_account.contains_key(&pubkey.clone()) {
                self.state_account.alter(&pubkey.clone(), |k, v| {
                    return tt;
                });
            } else {
                self.state_account.insert(pubkey.clone(), tt);
            }
    
            if self.state_owner.contains_key(&account.owner.clone()) {
    
                self.state_owner.alter(&account.owner.clone(), |k, mut v| {
                    if !v.contains(&pubkey.clone()) {
                        v.push(pubkey.clone());
                    }
                    return v;
                });
    
            } else {
                let mut vd: Vec<Pubkey> = vec![];
                vd.push(pubkey.clone());
                self.state_owner.insert(account.owner.clone(), vd);
            }
        }

    }

}
