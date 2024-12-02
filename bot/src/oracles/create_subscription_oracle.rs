
use std::{str::FromStr, sync::{atomic::{AtomicUsize, Ordering}, Arc}};
use crate::solana_state::{ProchainAccountInfo, SolanaStateManager};
use {
    futures::{sink::SinkExt, stream::StreamExt},
    log::{error},
    maplit::hashmap,

    std::{
        collections::HashMap,
    },

    yellowstone_grpc_client::GeyserGrpcClient,
    yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, SubscribeRequest,
        SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterAccounts
    },
};
use futures_util;
use chrono::Timelike;
use futures::Sink;
use lazy_static::lazy_static;
use parking_lot::{Mutex};
use dashmap::DashMap;
use solana_client::{rpc_client::RpcClient, rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig}};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signer::Signer};
use tracing::info;
use yellowstone_grpc_proto::geyser::{subscribe_request_filter_accounts_filter::Filter, subscribe_request_filter_accounts_filter_lamports::Cmp, SubscribeRequestFilterAccountsFilter, SubscribeRequestFilterAccountsFilterLamports, SubscribeRequestFilterTransactions};
use hashbrown::HashSet;

lazy_static! {
    static ref LIST_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());
    static ref LIST_PROGRAM_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());
    static ref LIST_TOKEN_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());
    static ref LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());
    static ref SPINLOCK_REFRESH: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    static ref SPINLOCK_REFRESH_MESSAGE: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    static ref SPINLOCK_REFRESH_MESSAGE_OWNER: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    static ref SPINLOCK_REFRESH_MESSAGE_TOKENOWNER: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
}

pub fn set_mutex_account_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_account_sub(sub_name: String) -> Vec<String> {
    let map = LIST_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn set_mutex_program_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_PROGRAM_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_program_sub(sub_name: String) -> Vec<String> {
    let map = LIST_PROGRAM_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn set_mutex_token_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_TOKEN_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_token_sub(sub_name: String) -> Vec<String> {
    let map = LIST_TOKEN_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn set_mutex_token_owner_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_token_owner_sub(sub_name: String) -> Vec<String> {
    let map = LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn refresh() {
    SPINLOCK_REFRESH_MESSAGE.swap(1, Ordering::Relaxed);
}

pub fn refresh_owner() {
    SPINLOCK_REFRESH_MESSAGE_OWNER.swap(1, Ordering::Relaxed);
}

pub fn refresh_tokenowner() {
    SPINLOCK_REFRESH_MESSAGE_TOKENOWNER.swap(1, Ordering::Relaxed);
}

pub fn exit_subscription() {
    SPINLOCK_REFRESH.swap(1, Ordering::Relaxed);
    SPINLOCK_REFRESH_MESSAGE.swap(1, Ordering::Relaxed);
}

pub async fn run(state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>, sub_name: String) {

    let local_arc = state.clone();
    let sub_name_local = sub_name.clone();
    
    let programs = get_mutex_program_sub(sub_name.clone());   
    programs.iter().for_each(|prog| {
        log::info!("starting processing program: {}", prog);

        let res = sol_client.get_program_accounts(&Pubkey::try_from(prog.as_str()).unwrap());

        if res.is_ok() {

            let response = res.unwrap();
            let response_len = response.len();
            log::info!("adding account {}, program: {}", response_len, prog);

            let count: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

            response.iter().for_each(|acc| {

                let owner_key = Pubkey::try_from(acc.1.owner).unwrap();
                let tt = ProchainAccountInfo {
                    pubkey: acc.0.clone(),
                    lamports: acc.1.lamports,
                    executable: acc.1.executable,
                    owner: owner_key.clone(),
                    rent_epoch: acc.1.rent_epoch,
                    slot: 0,
                    write_version: 0,
                    txn_signature: None,
                    data: acc.1.data.clone(),
                    last_update: chrono::offset::Utc::now()
                };

                local_arc.add_account_info(acc.0, tt);
                
                let mut data = count.lock();
                *data += 1;

                if *data % 100 == 0 {
                    log::info!("[{}] adding account {} / {}", prog, *data, response_len);
                }
            });

        } else {
            log::error!("error calling get_program_accounts: {}", res.err().unwrap());
        }       

        log::info!("finished processing program: {}", prog);
    }); 

    let accounts = get_mutex_account_sub(sub_name.clone());   
    accounts.iter().for_each(|acc| {
        log::info!("starting processing account: {}", acc);

        let res = sol_client.get_account(&Pubkey::try_from(acc.as_str()).unwrap());   

        if res.is_ok() {

            let response = res.unwrap();
            log::info!("adding account {}", acc);

            let p_key = Pubkey::try_from(acc.as_str()).unwrap();
            let owner_key = Pubkey::try_from(response.owner).unwrap();
            let tt = ProchainAccountInfo {
                pubkey: p_key.clone(),
                lamports: response.lamports,
                executable: response.executable,
                owner: owner_key.clone(),
                rent_epoch: response.rent_epoch,
                slot: 0,
                write_version: 0,
                txn_signature: None,
                data: response.data.clone(),
                last_update: chrono::offset::Utc::now()
            };

            local_arc.add_account_info(p_key, tt);

        } else {
            log::error!("error calling get_account: {}", res.err().unwrap());
        }       

        log::info!("finished processing accounts: {}", acc);
    }); 

    let _1 = tokio::spawn(async move {

        loop {

            log::info!("refresh subscription accounts: {:?}", get_mutex_account_sub(sub_name_local.clone()).len());
            SPINLOCK_REFRESH_MESSAGE.swap(0, Ordering::Relaxed);

            if SPINLOCK_REFRESH.swap(0, Ordering::Relaxed) == 1 {
                break;
            }
            
            let mut client = GeyserGrpcClient::build_from_shared(String::from("http://192.168.100.98:10000")).unwrap().connect().await.unwrap();

            let (mut subscribe_tx, mut stream) = client.subscribe().await.unwrap();           

            let mut hp = HashMap::new();
            hp.insert(sub_name_local.clone() + "_account", SubscribeRequestFilterAccounts {
                account: get_mutex_account_sub(sub_name_local.clone()),
                owner: vec![],
                filters: vec![],
                nonempty_txn_signature: None
            });

            let _res = subscribe_tx
                .send(SubscribeRequest {
                    slots: HashMap::new(),
                    accounts: hp,
                    transactions: HashMap::new(),
                    transactions_status: HashMap::new(),
                    entry: HashMap::new(),
                    blocks: HashMap::new(),
                    blocks_meta: hashmap! { "".to_owned() => SubscribeRequestFilterBlocksMeta {} },
                    commitment: Some(1 as i32),
                    accounts_data_slice: vec![],
                    ping: None,
                })
                .await;

            _res.unwrap();

            let mut counter_int = 0;
            let mut showlog = false;

            while let Some(message) = stream.next().await {

                let date_now = chrono::offset::Utc::now();

                if date_now.second() == 0 {
                    if showlog {
                        showlog = false;
                        info!("[PERF] update from yellowstone rpc subscription by accountid, accounts: {}", counter_int);
                        counter_int = 0;
                    }
                } else {
                    showlog = true;
                }

                if SPINLOCK_REFRESH_MESSAGE.swap(0, Ordering::Relaxed) == 1 {
                    break;
                }

                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(tx)) => {
                                counter_int = counter_int + 1;

                                if let Some(acc) = tx.account {

                                    if acc.lamports == 0 {
                                        local_arc.clean_zero_account(Pubkey::from_str(String::from_utf8(acc.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                    } else {
                                        local_arc.handle_account_update(acc);
                                    }
                                    
                                }
                            }
                            Some(UpdateOneof::BlockMeta(blockmeta)) => {

                                local_arc.set_slot(blockmeta.slot);
                                local_arc.set_blockhash(blockmeta.blockhash);
                                if blockmeta.block_height.is_some() {
                                    local_arc.set_blockheight(blockmeta.block_height.unwrap().block_height);
                                }

                            }
                            Some(UpdateOneof::Block(block)) => {
                                block.accounts.iter().filter(|p| p.lamports == 0).for_each(|p| {
                                    local_arc.clean_zero_account(Pubkey::from_str(String::from_utf8(p.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                });
                            }
                            _ => {}
                        }
                    }
                    Err(error) => {
                        error!("stream error: {error:?}");
                        break;
                    }
                }
            
            }

            let _1 = subscribe_tx.close().await.unwrap();
            info!("closing yellowstone subscription for accounts ...");

        } // end of loop

    });

    let sub_name_local_2 = sub_name.clone();
    let local_arc_2 = state.clone();

    let _2 = tokio::spawn(async move {

        loop {

            log::info!("refresh subscription program_owner: {:?}", get_mutex_program_sub(sub_name_local_2.clone()).len());
            SPINLOCK_REFRESH_MESSAGE_OWNER.swap(0, Ordering::Relaxed);

            if SPINLOCK_REFRESH.swap(0, Ordering::Relaxed) == 1 {
                break;
            }
            
            let mut client = GeyserGrpcClient::build_from_shared(String::from("http://192.168.100.98:10000")).unwrap().connect().await.unwrap();

            let (mut subscribe_tx, mut stream) = client.subscribe().await.unwrap();           

            let mut hp = HashMap::new();
            hp.insert(sub_name_local_2.clone() + "_owner", SubscribeRequestFilterAccounts {
                account: vec![],
                owner: get_mutex_program_sub(sub_name_local_2.clone()),
                filters: vec![],
                nonempty_txn_signature: None
            });

            let _res = subscribe_tx
                .send(SubscribeRequest {
                    slots: HashMap::new(),
                    accounts: hp,
                    transactions: HashMap::new(),
                    transactions_status: HashMap::new(),
                    entry: HashMap::new(),
                    blocks: HashMap::new(),
                    blocks_meta: hashmap! { "".to_owned() => SubscribeRequestFilterBlocksMeta {} },
                    commitment: Some(1 as i32),
                    accounts_data_slice: vec![],
                    ping: None,
                })
                .await;

            _res.unwrap();

            let mut counter_int = 0;
            let mut showlog = false;

            while let Some(message) = stream.next().await {

                let date_now = chrono::offset::Utc::now();

                if date_now.second() == 0 {
                    if showlog {
                        showlog = false;
                        info!("[PERF] update from yellowstone rpc subscription by owner, accounts: {}", counter_int);
                        counter_int = 0;
                    }
                } else {
                    showlog = true;
                }

                if SPINLOCK_REFRESH_MESSAGE_OWNER.swap(0, Ordering::Relaxed) == 1 {
                    break;
                }

                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(tx)) => {
                                counter_int = counter_int + 1;

                                if let Some(acc) = tx.account {
                                    if acc.lamports == 0 {
                                        local_arc_2.clean_zero_account(Pubkey::from_str(String::from_utf8(acc.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                    } else {
                                        local_arc_2.handle_account_update(acc);
                                    }
                                }
                            }
                            Some(UpdateOneof::BlockMeta(block)) => {

                                local_arc_2.set_slot(block.slot);
                                local_arc_2.set_blockhash(block.blockhash);

                                if block.block_height.is_some() {
                                    local_arc_2.set_blockheight(block.block_height.unwrap().block_height);
                                }

                            }
                            _ => {}
                        }
                    }
                    Err(error) => {
                        error!("stream error: {error:?}");
                        break;
                    }
                }
            
            }

            let _1 = subscribe_tx.close().await.unwrap();
            info!("closing yellowstone subscription for owner ...");

        } // end of loop

    });

    let sub_name_local_3 = sub_name.clone();
    let local_arc_3 = state.clone();

    let _3 = tokio::spawn(async move {

        loop {

            log::info!("refresh subscription token owner: {:?}", get_mutex_token_sub(sub_name_local_3.clone()).len());
            SPINLOCK_REFRESH_MESSAGE.swap(0, Ordering::Relaxed);

            if SPINLOCK_REFRESH.swap(0, Ordering::Relaxed) == 1 {
                break;
            }
            
            let mut client = GeyserGrpcClient::build_from_shared(String::from("http://192.168.100.98:10000")).unwrap().connect().await.unwrap();

            let (mut subscribe_tx, mut stream) = client.subscribe().await.unwrap();           

            let mut hp = HashMap::new();
            hp.insert(sub_name_local_3.clone() + "_token", SubscribeRequestFilterAccounts {
                account: get_mutex_token_sub(sub_name_local_3.clone()),
                owner: get_mutex_token_owner_sub(sub_name_local_3.clone()),
                filters: vec![],
                nonempty_txn_signature: None
            });

            let _res = subscribe_tx
                .send(SubscribeRequest {
                    slots: HashMap::new(),
                    accounts: hp,
                    transactions: HashMap::new(),
                    transactions_status: HashMap::new(),
                    entry: HashMap::new(),
                    blocks: HashMap::new(),
                    blocks_meta: hashmap! { "".to_owned() => SubscribeRequestFilterBlocksMeta {} },
                    commitment: Some(1 as i32),
                    accounts_data_slice: vec![],
                    ping: None,
                })
                .await;

            _res.unwrap();

            let mut counter_int = 0;
            let mut showlog = false;

            while let Some(message) = stream.next().await {

                let date_now = chrono::offset::Utc::now();

                if date_now.second() == 0 {
                    if showlog {
                        showlog = false;
                        info!("[PERF] update from yellowstone rpc subscription by token owner, accounts: {}", counter_int);
                        counter_int = 0;
                    }
                } else {
                    showlog = true;
                }

                if SPINLOCK_REFRESH_MESSAGE_TOKENOWNER.swap(0, Ordering::Relaxed) == 1 {
                    break;
                }

                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(tx)) => {
                                counter_int = counter_int + 1;

                                if let Some(acc) = tx.account {
                                    if acc.lamports == 0 {
                                        local_arc_3.clean_zero_account(Pubkey::from_str(String::from_utf8(acc.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                    } else {
                                        local_arc_3.handle_account_update(acc);
                                    }
                                }
                            }
                            Some(UpdateOneof::BlockMeta(block)) => {

                                local_arc_3.set_slot(block.slot);
                                local_arc_3.set_blockhash(block.blockhash);

                                if block.block_height.is_some() {
                                    local_arc_3.set_blockheight(block.block_height.unwrap().block_height);
                                }

                            }
                            _ => {}
                        }
                    }
                    Err(error) => {
                        error!("stream error: {error:?}");
                        break;
                    }
                }
            
            }

            let _1 = subscribe_tx.close().await.unwrap();
            info!("closing yellowstone subscription for token owner ...");

        } // end of loop

    });

    let _set_j = vec![_1, _2];

    //futures::future::join_all(set_j).await;

}


pub fn intersection(nums: Vec<Vec<String>>) -> Vec<String> {
    let mut intersect_result: Vec<String> = nums[0].clone();

    for temp_vec in nums {
        let unique_a: HashSet<String> = temp_vec.into_iter().collect();
        intersect_result = unique_a
            .intersection(&intersect_result.into_iter().collect())
            .map(|i| i.clone())
            .collect::<Vec<_>>();
    }
    intersect_result
}