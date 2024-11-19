pub mod error;

use dashmap::DashMap;
use log::info;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use utoipa::ToSchema;
use yellowstone_grpc_proto::geyser::SubscribeUpdateAccountInfo;
use core::str;
use std::{
    sync::Arc, time::Duration
};
use solana_sdk::{
    account::ReadableAccount, clock::Epoch
};

use chrono::Utc;

extern crate lazy_static;
use lazy_static::lazy_static;
use parking_lot::Mutex;

lazy_static! {
    static ref SOLANA_STATE_ENCAPSULATOR: Mutex<Arc<SolanaStateManager>> = 
        Mutex::new(Arc::new(SolanaStateManager::new(Arc::new(RpcClient::new_with_timeout_and_commitment("http://192.168.100.98:18899", Duration::from_secs(240), CommitmentConfig::confirmed())))));
}

pub fn get_solana_state() -> Arc<SolanaStateManager> {
    let state = SOLANA_STATE_ENCAPSULATOR.lock();
    return  state.clone();
}

impl Eq for ProchainAccountInfo {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema, PartialEq)]
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

#[derive(Clone)]
pub struct SolanaStateManager {
    state_account: Arc<StateSpace>,
    state_owner: Arc<StateOwnerSpace>,
    state_program_owner: Arc<StateProgramOwnerSpace>,
    slot: Arc<SlotSpace>,
    sol_client: Arc<RpcClient>
}

impl SolanaStateManager
{
    pub fn new(sol_client: Arc<RpcClient>) -> Self {
        let state_a: DashMap<Pubkey, ProchainAccountInfo> = DashMap::new();
        let state_o: DashMap<Pubkey, Vec<Pubkey>> = DashMap::new();
        let state_p: DashMap<Pubkey, Vec<Pubkey>> = DashMap::new();
        let slot_a: DashMap<String, u64> = DashMap::new();

        slot_a.insert(String::from("confirmed"), 0);

        Self {
            state_account: Arc::new(state_a),
            state_owner: Arc::new(state_o),
            state_program_owner: Arc::new(state_p),
            slot: Arc::new(slot_a),
            sol_client: sol_client
        }
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

    pub fn handle_account_update(&self, ua: SubscribeUpdateAccountInfo) {

        let c: &[u8] = &ua.pubkey;
        let pub_key = Pubkey::try_from(c).unwrap();

        //log::info!("updating account: {}", pub_key);

        let owner_key = Pubkey::try_from(ua.owner).unwrap();

        let tt = ProchainAccountInfo {
            pubkey: pub_key,
            lamports: ua.lamports,
            executable: ua.executable,
            owner: owner_key.clone(),
            rent_epoch: ua.rent_epoch,
            slot: 0,
            write_version: ua.write_version,
            txn_signature: ua.txn_signature,
            data: ua.data,
            last_update: chrono::offset::Utc::now()
        };

        if tt.lamports == 0 {
            self.clean_zero_account(pub_key);
        } else {
            if self.state_account.contains_key(&pub_key) {
                self.state_account.alter(&pub_key, |k, v| {
                    return tt;
                });
            } else {
                self.state_account.insert(pub_key, tt);
            }
    
            if self.state_owner.contains_key(&owner_key.clone()) {
    
                self.state_owner.alter(&owner_key, |k, mut v| {
                    if !v.contains(&pub_key) {
                        v.push(pub_key);
                    }
                    return v;
                });
    
            } else {
                let mut vd: Vec<Pubkey> = vec![];
                vd.push(pub_key);
                self.state_owner.insert(owner_key.clone(), vd);
            }
        }

    }

}
