use std::{collections::HashMap, str::FromStr, sync::atomic::AtomicUsize};

use bigdecimal::ToPrimitive;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{AckSender, Data, SocketRef, State},
    SocketIo,
};
use solana_account_decoder::{parse_token::UiTokenAccount, UiAccountData, UiAccountEncoding};
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTokenAccountsFilter}, rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType}};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use utoipa::ToSchema;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use solana_account_decoder::UiAccountData::{Json, Binary, LegacyBinary};
use crate::{rpc::rpc::OptionalContext::{Context, NoContext}, utils::types::structs::prochain::UserFleetCargoItem};
use crate::{rpc::{request_processor::JsonRpcRequestProcessor, rpc_service::JsonRpcConfig}, solana_state::SolanaStateManager, utils::types::structs::prochain::{UserFleetInstanceRequest, UserFleetInstanceResponse}};
use log::error;
use std::sync::atomic::Ordering;

use {
    futures::{sink::SinkExt, stream::StreamExt},
    maplit::hashmap,
    yellowstone_grpc_client::GeyserGrpcClient,
    yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, SubscribeRequest,
        SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterAccounts
    },
};

use lazy_static::lazy_static;
use dashmap::DashMap;
use tracing::info;

lazy_static! {
    static ref LIST_FLEET_SUBSCRIPTION: Mutex<DashMap<String, FleetSubscription>> = Mutex::new(DashMap::new());
    static ref SPINLOCK_REFRESH_FLEET: Mutex<DashMap<String, Arc<AtomicUsize>>> = Mutex::new(DashMap::new());
    static ref SPINLOCK_MESSAGE_REFRESH_FLEET: Mutex<DashMap<String, Arc<AtomicUsize>>> = Mutex::new(DashMap::new());
    static ref SPINLOCK_REFRESH_FLEET_OWNER: Mutex<DashMap<String, Arc<AtomicUsize>>> = Mutex::new(DashMap::new());
    static ref SPINLOCK_MESSAGE_REFRESH_FLEET_OWNER: Mutex<DashMap<String, Arc<AtomicUsize>>> = Mutex::new(DashMap::new());
}

pub fn set_mutex_fleet_sub(sub_name: String, lst_vec: FleetSubscription) {
    LIST_FLEET_SUBSCRIPTION.lock().insert(sub_name.clone(), lst_vec);
    SPINLOCK_REFRESH_FLEET.lock().insert(sub_name.clone(), Arc::new(AtomicUsize::new(0)));
    SPINLOCK_MESSAGE_REFRESH_FLEET.lock().insert(sub_name.clone(), Arc::new(AtomicUsize::new(0)));
    SPINLOCK_REFRESH_FLEET_OWNER.lock().insert(sub_name.clone(), Arc::new(AtomicUsize::new(0)));
    SPINLOCK_MESSAGE_REFRESH_FLEET_OWNER.lock().insert(sub_name.clone(), Arc::new(AtomicUsize::new(0)));
}

pub fn remove_mutex_fleet_sub_sid(sid: String) {

    let all_keys_subs: Vec<String> = LIST_FLEET_SUBSCRIPTION.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_subs {

        if !a.starts_with(&sid) {
            continue;
        }

        LIST_FLEET_SUBSCRIPTION.lock().remove(&a.clone());
    }
    
    let all_keys_locks: Vec<String> = SPINLOCK_REFRESH_FLEET.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_locks {

        if !a.starts_with(&sid) {
            continue;
        }

        SPINLOCK_REFRESH_FLEET.lock().alter(&a.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    }

    let all_keys_locks_message: Vec<String> = SPINLOCK_MESSAGE_REFRESH_FLEET.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_locks_message {

        if !a.starts_with(&sid) {
            continue;
        }

        SPINLOCK_MESSAGE_REFRESH_FLEET.lock().alter(&a.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    }

    let all_keys_locks_owner: Vec<String> = SPINLOCK_REFRESH_FLEET_OWNER.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_locks_owner {

        if !a.starts_with(&sid) {
            continue;
        }

        SPINLOCK_REFRESH_FLEET_OWNER.lock().alter(&a.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    }

    let all_keys_locks_message_owner: Vec<String> = SPINLOCK_MESSAGE_REFRESH_FLEET_OWNER.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_locks_message_owner {

        if !a.starts_with(&sid) {
            continue;
        }

        SPINLOCK_MESSAGE_REFRESH_FLEET_OWNER.lock().alter(&a.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    }

}


pub fn remove_mutex_fleet_sub(sub_name: String) {

    LIST_FLEET_SUBSCRIPTION.lock().remove(&sub_name.clone());
    SPINLOCK_REFRESH_FLEET.lock().alter(&sub_name.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    SPINLOCK_MESSAGE_REFRESH_FLEET.lock().alter(&sub_name.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    SPINLOCK_REFRESH_FLEET_OWNER.lock().alter(&sub_name.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });
    SPINLOCK_MESSAGE_REFRESH_FLEET_OWNER.lock().alter(&sub_name.clone(), |k, v| { return Arc::new(AtomicUsize::new(1)) });

}

pub fn remove_all_fleet_sub() {
    let av = get_all_values_sub();
    for it in av {
        remove_mutex_fleet_sub(it.id_sub);
    }
}

pub fn get_all_values_sub() -> Vec<FleetSubscription> {
    let map: Vec<FleetSubscription> = LIST_FLEET_SUBSCRIPTION.lock().iter().map(|ref_multi| ref_multi.value().clone()).collect();
    return map;
}

pub fn get_all_values_sub_by_user_id(user_id: String) -> Vec<FleetSubscription> {

    let user_id_key: String = user_id.replace("-", "").chars().skip(0).take(10).collect();

    let map: Vec<FleetSubscription> = LIST_FLEET_SUBSCRIPTION.lock().iter().map(|ref_multi| ref_multi.value().clone()).collect();
    return map.iter().filter(|t| {
        let parts= t.id_sub.split("-");
        let collection: Vec<&str> = parts.collect();
        return collection[1] == user_id_key.as_str();
    }).map(|t| t.clone()).collect();
}

pub fn get_mutex_fleet_sub(sub_name: String) -> FleetSubscription {
    let map = LIST_FLEET_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return FleetSubscription { id_sub: sub_name, account_address: vec![], owner_address: vec![] }; },
        Some(val) => { val.value().clone() }
    }
}

pub fn get_mutex_spinlock_fleet_sub(sub_name: String) -> Arc<AtomicUsize> {
    let map = SPINLOCK_REFRESH_FLEET.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return Arc::new(AtomicUsize::new(0)); },
        Some(val) => { val.value().clone() }
    }
}

pub fn get_mutex_spinlock_message_fleet_sub(sub_name: String) -> Arc<AtomicUsize> {
    let map = SPINLOCK_MESSAGE_REFRESH_FLEET.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return Arc::new(AtomicUsize::new(0)); },
        Some(val) => { val.value().clone() }
    }
}

pub fn get_mutex_spinlock_fleet_owner_sub(sub_name: String) -> Arc<AtomicUsize> {
    let map = SPINLOCK_REFRESH_FLEET.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return Arc::new(AtomicUsize::new(0)); },
        Some(val) => { val.value().clone() }
    }
}

pub fn get_mutex_spinlock_message_fleet_owner_sub(sub_name: String) -> Arc<AtomicUsize> {
    let map = SPINLOCK_MESSAGE_REFRESH_FLEET.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return Arc::new(AtomicUsize::new(0)); },
        Some(val) => { val.value().clone() }
    }
}

pub fn start_socketio_httpd(config: JsonRpcConfig, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {

    log::info!("Starting SocketIO httpd !");
    let _1 = std::thread::spawn(|| {
    
        let _t = run(config, state, sol_client).is_ok();
        log::info!("SocketIO httpd started !");

    });

}

#[derive(Serialize, Deserialize, Clone, Debug, ToSchema)]
pub struct FleetSubscription {
    id_sub: String,
    account_address: Vec<String>,
    owner_address: Vec<String>
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase", untagged)]
enum Res {
    Login {
        #[serde(rename = "numUsers")]
        num_users: usize,
    }
}

#[derive(Clone)]
struct UserCnt(Arc<AtomicUsize>, DashMap<String, String>);
impl UserCnt {
    fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(0)), DashMap::new())
    }
    fn add_user(&self, socket_id: String) {
        let _ = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        self.1.alter(&socket_id, |k, v| {
            return socket_id.clone();
        });
    }
    fn remove_user(&self, socket_id: String) {
        let _ = self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst) - 1;
        self.1.remove(&socket_id);
    }
}

pub async fn run_subscription_fleet(state: Arc<SolanaStateManager>, sub_name: String, json_rpc_processor: JsonRpcRequestProcessor, ufi: UserFleetInstanceRequest, s: SocketRef) {

    let sub_name_local = sub_name.clone();
    let local_arc = state.clone();
    let local_socket = s.clone();
    let local_ufi = ufi.clone();
    let local_json_rpc_processor = json_rpc_processor.clone();

    let _1 = tokio::spawn(async move {

        info!("starting yellowstone subscription for fleet account {:?}", sub_name_local);

        loop {

            if get_mutex_spinlock_fleet_sub(sub_name_local.clone()).swap(0, Ordering::Relaxed) == 1 {
                info!("stopping yellowstone subscription for fleet get_mutex_spinlock_fleet_sub account {:?}", sub_name_local);
                break;
            }
            
            let mut client = GeyserGrpcClient::build_from_shared(String::from("http://192.168.100.98:10000")).unwrap().connect().await.unwrap();

            let (mut subscribe_tx, mut stream) = client.subscribe().await.unwrap();           

            let sub_fleet = get_mutex_fleet_sub(sub_name_local.clone());

            let mut hp = HashMap::new();
            hp.insert(sub_name_local.clone()[..30].to_string(), SubscribeRequestFilterAccounts {
                account: sub_fleet.account_address,
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

            while let Some(message) = stream.next().await {

                if get_mutex_spinlock_message_fleet_sub(sub_name_local.clone()).swap(0, Ordering::Relaxed) == 1 {
                    info!("stopping yellowstone subscription for fleet get_mutex_spinlock_message_fleet_sub account {:?}", sub_name_local.clone());
                    break;
                }

                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(tx)) => {

                                if let Some(acc) = tx.account {

                                    log::info!("fleet account {:?}", local_ufi.clone().publicKey);

                                    if acc.lamports == 0 {
                                        local_arc.clean_zero_account(Pubkey::from_str(String::from_utf8(acc.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                    } else {
                                        local_arc.handle_account_update(acc);
                                    }

                                    let fleet_refreshed = refresh_fleet(local_json_rpc_processor.clone(), local_ufi.clone()).await;
                                    if fleet_refreshed.is_ok() {
                                        let fleet_refreshed_json = serde_json::to_string(&fleet_refreshed.unwrap());
                                        if fleet_refreshed_json.is_ok() {
                                            let _ = local_socket.emit("userfleet_refreshed", &fleet_refreshed_json.unwrap());
                                            log::info!("fleet updated {:?}", local_ufi.clone().publicKey);
                                        } else {
                                            log::error!("userfleet_refreshed error {:?}", fleet_refreshed_json.err());
                                        }
                                    } else {
                                        log::error!("fleet update error {:?}", fleet_refreshed.err());
                                    }

                                                                        
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
            info!("closing yellowstone subscription for fleets account ...");

        } // end of loop

        info!("stopping yellowstone subscription for fleet account {:?}", sub_name_local);

    });   

    let sub_name_local_2 = sub_name.clone();
    let local_arc_2 = state.clone();
    let local_socket_2 = s.clone();
    let local_ufi_2 = ufi.clone();
    let local_json_rpc_processor_2 = json_rpc_processor.clone();

    let _2 = tokio::spawn(async move {

        info!("starting yellowstone subscription for fleet owner {:?}", sub_name);

        loop {

            if get_mutex_spinlock_fleet_owner_sub(sub_name_local_2.clone()).swap(0, Ordering::Relaxed) == 1 {
                info!("stopping yellowstone subscription for fleet get_mutex_spinlock_fleet_owner_sub owner {:?}", sub_name_local_2);
                break;
            }
            
            let mut client = GeyserGrpcClient::build_from_shared(String::from("http://192.168.100.98:10000")).unwrap().connect().await.unwrap();

            let (mut subscribe_tx, mut stream) = client.subscribe().await.unwrap();           

            let sub_fleet = get_mutex_fleet_sub(sub_name_local_2.clone());

            let mut hp = HashMap::new();
            hp.insert(sub_name_local_2.clone()[..20].to_string() + "_fleet_own", SubscribeRequestFilterAccounts {
                account: vec![],
                owner: sub_fleet.owner_address,
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

            while let Some(message) = stream.next().await {

                if get_mutex_spinlock_message_fleet_owner_sub(sub_name_local_2.clone()).swap(0, Ordering::Relaxed) == 1 {
                    info!("stopping yellowstone subscription for fleet get_mutex_spinlock_message_fleet_owner_sub {:?}", sub_name_local_2);
                    break;
                }

                match message {
                    Ok(msg) => {
                        match msg.update_oneof {
                            Some(UpdateOneof::Account(tx)) => {

                                if let Some(acc) = tx.account {

                                    log::info!("fleet account {:?}", local_ufi_2.clone().publicKey);

                                    if acc.lamports == 0 {
                                        local_arc_2.clean_zero_account(Pubkey::from_str(String::from_utf8(acc.pubkey.clone()).unwrap_or_default().as_str()).unwrap_or_default());
                                    } else {
                                        local_arc_2.handle_account_update(acc);
                                    }

                                    let fleet_refreshed = refresh_fleet(local_json_rpc_processor_2.clone(), local_ufi_2.clone()).await;
                                    if fleet_refreshed.is_ok() {
                                        let fleet_refreshed_json = serde_json::to_string(&fleet_refreshed.unwrap());
                                        if fleet_refreshed_json.is_ok() {
                                            let _ = local_socket_2.emit("userfleet_refreshed", &fleet_refreshed_json.unwrap());
                                            log::info!("fleet updated {:?}", ufi.clone().publicKey);
                                        } else {
                                            log::error!("userfleet_refreshed error {:?}", fleet_refreshed_json.err());
                                        }
                                    } else {
                                        log::error!("fleet update error {:?}", fleet_refreshed.err());
                                    }

                                                                        
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
            info!("closing yellowstone subscription for fleets owner ...");

        } // end of loop

        info!("stopping yellowstone subscription for fleet owner {:?}", sub_name_local_2);
        
    }); 

    let _set_j = vec![_1, _2];

}

pub async fn create_subscription_for_fleet(key: String, json_rpc_processor: JsonRpcRequestProcessor, ufi: UserFleetInstanceRequest) -> Result<FleetSubscription, anyhow::Error> {

    let mut fleet_sub = FleetSubscription { id_sub: key, account_address: vec![], owner_address: vec![] };

    fleet_sub.account_address.push(ufi.publicKey.to_string());
    fleet_sub.owner_address.push(ufi.publicKey.to_string());

    let rpc_token_account_filter = RpcTokenAccountsFilter::ProgramId("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string());

    // let _rpc_acc_info_req_3 = RpcAccountInfoConfig {
    //     encoding: Some(UiAccountEncoding::JsonParsed),
    //     data_slice: None,
    //     commitment: Some(CommitmentConfig::confirmed()),
    //     min_context_slot: None,
    // };

    fleet_sub.owner_address.push(ufi.cargoHold.clone());
    fleet_sub.account_address.push(ufi.foodToken);
    fleet_sub.account_address.push(ufi.sduToken);

    // let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(ufi.cargoHold.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_3)).await?;
    // let current_food_iter = fleet_current_cargo.value.iter().filter(|f| { f.pubkey == ufi.foodToken }).nth(0);
    // if current_food_iter.is_some() {

    //     let addr1 = match current_food_iter.unwrap().clone().account.data {
    //         Json(t) => {
    //             Some(t.parsed)
    //         }
    //         Binary(_1, _2) => None,
    //         LegacyBinary(_1) => None
    //     };

    //     if addr1.is_some() {
    //         fleet_sub.account_address.push(ufi.foodToken);
    //     }
        
    // }

    // let current_sdu_iter = fleet_current_cargo.value.iter().filter(|f| { f.pubkey == ufi.sduToken }).nth(0);
    // if current_sdu_iter.is_some() {

    //     let addr1 = match current_sdu_iter.unwrap().clone().account.data {
    //         Json(t) => {
    //             Some(t.program)
    //         }
    //         Binary(_1, _2) => {
    //             None
    //         },
    //         LegacyBinary(_1) => {
    //             None
    //         }
    //     };

    //     if addr1.is_some() {
    //         fleet_sub.account_address.push(addr1.unwrap());
    //     }

    // }

    // let rpc_acc_info_req_4 = RpcAccountInfoConfig {
    //     encoding: Some(UiAccountEncoding::JsonParsed),
    //     data_slice: None,
    //     commitment: Some(CommitmentConfig::confirmed()),
    //     min_context_slot: None,
    // };

    fleet_sub.owner_address.push(ufi.fuelTank.clone());
    fleet_sub.account_address.push(ufi.fuelToken);

    // let fleet_current_fuel = json_rpc_processor.get_token_account_by_owner(ufi.fuelTank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_4)).await?;
    // let current_fuel_iter = fleet_current_fuel.value.iter().filter(|f| { f.pubkey == ufi.fuelToken }).nth(0);
    // if current_fuel_iter.is_some() {

    //     let addr1 = match current_fuel_iter.unwrap().clone().account.data {
    //         Json(t) => {
    //             Some(t.program)
    //         }
    //         Binary(_1, _2) => {
    //             None
    //         },
    //         LegacyBinary(_1) => {
    //             None
    //         }
    //     };

    //     if addr1.is_some() {
    //         fleet_sub.account_address.push(addr1.unwrap());
    //     }

    // }

    // let rpc_acc_info_req_5 = RpcAccountInfoConfig {
    //     encoding: Some(UiAccountEncoding::JsonParsed),
    //     data_slice: None,
    //     commitment: Some(CommitmentConfig::confirmed()),
    //     min_context_slot: None,
    // };

    fleet_sub.owner_address.push(ufi.ammoBank.clone());
    fleet_sub.account_address.push(ufi.ammoToken);

    // let fleet_current_ammo = json_rpc_processor.get_token_account_by_owner(ufi.ammoBank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_5)).await?;
    // let current_ammo_iter = fleet_current_ammo.value.iter().filter(|f| { f.pubkey == ufi.ammoToken }).nth(0);
    // if current_ammo_iter.is_some() {

    //     let addr1 = match current_ammo_iter.unwrap().clone().account.data {
    //         Json(t) => {
    //             Some(t.program)
    //         }
    //         Binary(_1, _2) => {
    //             None
    //         },
    //         LegacyBinary(_1) => {
    //             None
    //         }
    //     };

    //     if addr1.is_some() {
    //         fleet_sub.account_address.push(addr1.unwrap());
    //     }

    // }

    let rpc_acc_info_req_2 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let rpc_prog_info = RpcProgramAccountsConfig {
        filters: Some(vec![
            RpcFilterType::Memcmp(Memcmp::new(0, MemcmpEncodedBytes::Base58(String::from_str("UcyYNefQ2BW")?))),
            RpcFilterType::Memcmp(Memcmp::new(41, MemcmpEncodedBytes::Base58(ufi.publicKey.to_string())))]),
        account_config: rpc_acc_info_req_2,
        with_context: None,
        sort_results: None,
    };

    let starbase_player_cargo_holds = json_rpc_processor.get_program_accounts(&Pubkey::try_from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk")?, Some(rpc_prog_info), Some(ufi.forceRefresh)).await?;

    let rp1 = match starbase_player_cargo_holds {

        Context(ctx) => {
            
            for rka in ctx.value {

                let rpc_acc_info_req_cargo = RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::JsonParsed),
                    data_slice: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    min_context_slot: None,
                };
            
                let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    fleet_sub.account_address.push(fcc.pubkey);

                    // let addr1 = match fcc.clone().account.data {
                    //     Json(t) => {
                    //         Some(t.program)
                    //     }
                    //     Binary(_1, _2) => {
                    //         None
                    //     },
                    //     LegacyBinary(_1) => {
                    //         None
                    //     }
                    // };

                    // if addr1.is_some() {
                    //     fleet_sub.account_address.push(addr1.unwrap());
                    // }

                }

            }

        },
        NoContext(nc) => {

            for rka in nc {

                let rpc_acc_info_req_cargo = RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::JsonParsed),
                    data_slice: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    min_context_slot: None,
                };
            
                let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    fleet_sub.account_address.push(fcc.pubkey);

                    // let addr1 = match fcc.clone().account.data {
                    //     Json(t) => {
                    //         Some(t.program)
                    //     }
                    //     Binary(_1, _2) => {
                    //         None
                    //     },
                    //     LegacyBinary(_1) => {
                    //         None
                    //     }
                    // };

                    // if addr1.is_some() {
                    //     fleet_sub.account_address.push(addr1.unwrap());
                    // }

                }

            }

        }
    };

    log::info!("fleet_sub: {:?}", fleet_sub);

    Ok(fleet_sub) 

}

pub async fn refresh_fleet(json_rpc_processor: JsonRpcRequestProcessor, ufi: UserFleetInstanceRequest) -> Result<UserFleetInstanceResponse, anyhow::Error> {

    let pub_key = Pubkey::try_from(ufi.publicKey.as_str())?;

    let rpc_acc_info_req_1 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let fleet_acc_info = json_rpc_processor.get_account_info(&pub_key, Some(rpc_acc_info_req_1), Some(ufi.forceRefresh)).await?;
    let res_fleet_acc_info = match fleet_acc_info.value.unwrap().data {
        UiAccountData::Json(_) => None,
        UiAccountData::LegacyBinary(blob) => None,
        UiAccountData::Binary(blob, encoding) => match encoding {
            UiAccountEncoding::Base58 => Some(blob),
            UiAccountEncoding::Base64 => Some(blob),
            UiAccountEncoding::Base64Zstd => Some(blob),
            UiAccountEncoding::Binary | UiAccountEncoding::JsonParsed => None,
        }
    };

    let rpc_token_account_filter = RpcTokenAccountsFilter::ProgramId("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string());

    let rpc_acc_info_req_3 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::JsonParsed),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(ufi.cargoHold.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_3), Some(ufi.forceRefresh)).await?;
    let current_food_iter = fleet_current_cargo.value.iter().filter(|f| { f.pubkey == ufi.foodToken }).nth(0);
    let mut amount_food: u64 = 0;
    if current_food_iter.is_some() {
        amount_food = match current_food_iter.unwrap().clone().account.data {
            Json(t) => {
                let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap()
            }
            Binary(_1, _2) => {
                0
            },
            LegacyBinary(_1) => {
                0
            }
        };
    }

    let current_sdu_iter = fleet_current_cargo.value.iter().filter(|f| { f.pubkey == ufi.sduToken }).nth(0);
    let mut amount_sdu: u64 = 0;
    if current_sdu_iter.is_some() {
        amount_sdu = match current_sdu_iter.unwrap().clone().account.data {
            Json(t) => {
                let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap()
            }
            Binary(_1, _2) => {
                0
            },
            LegacyBinary(_1) => {
                0
            }
        };
    }

    let rpc_acc_info_req_4 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::JsonParsed),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let fleet_current_fuel = json_rpc_processor.get_token_account_by_owner(ufi.fuelTank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_4), Some(ufi.forceRefresh)).await?;
    let current_fuel_iter = fleet_current_fuel.value.iter().filter(|f| { f.pubkey == ufi.fuelToken }).nth(0);
    let mut amount_fuel: u64 = 0;
    if current_fuel_iter.is_some() {
        amount_fuel = match current_fuel_iter.unwrap().clone().account.data {
            Json(t) => {
                let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap()
            }
            Binary(_1, _2) => {
                0
            },
            LegacyBinary(_1) => {
                0
            }
        };
    }

    let rpc_acc_info_req_5 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::JsonParsed),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let fleet_current_ammo = json_rpc_processor.get_token_account_by_owner(ufi.ammoBank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_5), Some(ufi.forceRefresh)).await?;
    let current_ammo_iter = fleet_current_ammo.value.iter().filter(|f| { f.pubkey == ufi.ammoToken }).nth(0);
    let mut amount_ammo: u64 = 0;
    if current_ammo_iter.is_some() {
        amount_ammo = match current_ammo_iter.unwrap().clone().account.data {
            Json(t) => {
                let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap()
            }
            Binary(_1, _2) => {
                0
            },
            LegacyBinary(_1) => {
                0
            }
        };
    }

    let rpc_acc_info_req_2 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let rpc_prog_info = RpcProgramAccountsConfig {
        filters: Some(vec![
            RpcFilterType::Memcmp(Memcmp::new(0, MemcmpEncodedBytes::Base58(String::from_str("UcyYNefQ2BW").unwrap()))),
            RpcFilterType::Memcmp(Memcmp::new(41, MemcmpEncodedBytes::Base58(ufi.publicKey.to_string())))]),
        account_config: rpc_acc_info_req_2,
        with_context: None,
        sort_results: None,
    };

    let starbase_player_cargo_holds = json_rpc_processor.get_program_accounts(&Pubkey::try_from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk").unwrap(), Some(rpc_prog_info), Some(ufi.forceRefresh)).await?;

    let mut vec_tokens: Vec<UserFleetCargoItem> = vec![];

    let rp1 = match starbase_player_cargo_holds {

        Context(ctx) => {
            
            for rka in ctx.value {

                let rpc_acc_info_req_cargo = RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::JsonParsed),
                    data_slice: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    min_context_slot: None,
                };
            
                let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    let amount_cargo = match fcc.clone().account.data {
                        Json(t) => {
                            let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                            let res1 = deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap();
                            (res1, deserialized_token_account.mint)
                        }
                        Binary(_1, _2) => {
                            (0, "".to_string())
                        },
                        LegacyBinary(_1) => {
                            (0, "".to_string())
                        }
                    };

                    let ufci = UserFleetCargoItem {
                        publicKey: fcc.pubkey,
                        tokenAmount: amount_cargo.0,
                        tokenMint: amount_cargo.1
                    };

                    vec_tokens.push(ufci);

                }

            }

        },
        NoContext(nc) => {

            for rka in nc {

                let rpc_acc_info_req_cargo = RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::JsonParsed),
                    data_slice: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    min_context_slot: None,
                };
            
                let fleet_current_cargo = json_rpc_processor.get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    let amount_cargo = match fcc.clone().account.data {
                        Json(t) => {
                            let deserialized_token_account: UiTokenAccount = serde_json::from_value(t.parsed.get("info").unwrap().clone()).unwrap();
                            let res1 = deserialized_token_account.token_amount.ui_amount.unwrap().to_u64().unwrap();
                            (res1, deserialized_token_account.mint)
                        }
                        Binary(_1, _2) => {
                            (0, "".to_string())
                        },
                        LegacyBinary(_1) => {
                            (0, "".to_string())
                        }
                    };

                    let ufci = UserFleetCargoItem {
                        publicKey: fcc.pubkey,
                        tokenAmount: amount_cargo.0,
                        tokenMint: amount_cargo.1
                    };

                    vec_tokens.push(ufci);

                }

            }

        }
    };

    let user_instance: UserFleetInstanceResponse = UserFleetInstanceResponse {
        userId: ufi.userId,
        publicKey: pub_key.to_string(),
        fleetAcctInfo: res_fleet_acc_info.unwrap(),
        foodCnt: amount_food,
        fuelCnt: amount_fuel,
        ammoCnt: amount_ammo,
        sduCnt: amount_sdu,
        cargoHold: ufi.cargoHold.clone(),
        fuelTank: ufi.fuelTank.clone(),
        ammoBank: ufi.ammoBank.clone(),
        foodToken: ufi.foodToken.clone(),
        fuelToken: ufi.fuelToken.clone(),
        ammoToken: ufi.ammoToken.clone(),
        sduToken: ufi.sduToken.clone(),
        fleetCargo: vec_tokens,
    };

    return Ok(user_instance);

}

#[tokio::main]
pub async fn run(config: JsonRpcConfig, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) -> Result<(), Box<dyn std::error::Error>> {

    let state_local = state.clone();

    loop {

        let state_loop = state_local.clone();

        log::info!("Starting sockerio server on 0.0.0.0:14655");

        let request_processor = JsonRpcRequestProcessor::new(config.clone(), sol_client.clone(), state_loop.clone());
        let (layer, io) = SocketIo::builder().with_state(UserCnt::new()).build_layer();
    
        io.ns("/", move |s: SocketRef| {
    
            let request_processor_local = request_processor.clone();
    
            s.on(
                "subscribeToFleetChange",
                |s: SocketRef, Data::<String>(msg), user_cnt: State<UserCnt>| async move {
    
                    user_cnt.add_user(s.id.to_string());
                    let ufi: UserFleetInstanceRequest = serde_json::from_str(&msg).unwrap();
    
                    let s_id_key: String = s.id.to_string().replace("-", "").chars().skip(0).take(10).collect();
                    let user_id_key: String = ufi.userId.replace("-", "").chars().skip(0).take(10).collect();
                    let key = s_id_key + "-" + user_id_key.as_str() + "-" + &ufi.publicKey;
    
                    let vec_current_sub = get_all_values_sub();
    
                    if !vec_current_sub.iter().any(|f| { f.id_sub == key }) {
    
                        log::info!("[SOCKETIO] subscribeToFleetChange for id: {:?}", key);
    
                        tokio::spawn(async move {
        
                            let fleet_subscription = create_subscription_for_fleet(key.clone(), request_processor_local.clone(), ufi.clone()).await;
                            if fleet_subscription.is_ok() {
                                set_mutex_fleet_sub(key.clone(), fleet_subscription.unwrap());
                                let _res = run_subscription_fleet(state_loop.clone(), key.clone(), request_processor_local.clone(), ufi.clone(), s).await;
                            } else {
                                log::error!("Error creating the fleet subscription {:?}", ufi.clone().publicKey);
                            }
        
                        });  
    
                    }
                    
                },
            );
    
            let request_processor_local_1 = request_processor.clone();
    
            s.on(
                "forceRefreshFleet",
                |s: SocketRef, Data::<String>(msg), ack: AckSender| async move {
    
                    let ufi: UserFleetInstanceRequest = serde_json::from_str(&msg).unwrap();
                    log::info!("[SOCKETIO] force_fleet_refreshed for pubkey: {:?}", ufi.publicKey);
    
                    tokio::spawn(async move {
    
                        let fleet_refreshed = refresh_fleet(request_processor_local_1.clone(), ufi.clone()).await;
                        if fleet_refreshed.is_ok() {
    
                            let fleet_refreshed_json = serde_json::to_string(&fleet_refreshed.unwrap());
                            if fleet_refreshed_json.is_ok() {
                                log::info!("force_fleet_refreshed updated {:?}", ufi.clone().publicKey);
                                ack.send(&fleet_refreshed_json.unwrap()).ok()
                            } else {
                                log::error!("force_fleet_refreshed error {:?}", fleet_refreshed_json.err());
                                None
                            }
    
                        } else {
                            log::error!("force_fleet_refreshed {:?}", ufi.clone().publicKey);
                            None
                        }
    
                    });  
                    
                },
            );
    
            s.on_disconnect(
                |s: SocketRef, user_cnt: State<UserCnt>| async move {
    
                    user_cnt.remove_user(s.id.to_string());
    
                    let s_id_key: String = s.id.to_string().replace("-", "").chars().skip(0).take(10).collect();
                    remove_mutex_fleet_sub_sid(s_id_key);
    
                    log::info!("[SOCKETIO] on_disconnect for id: {:?}", s.id.to_string());
    
                },
            );
    
        });
    
        let app = axum::Router::new()
            .fallback_service(ServeDir::new("dist"))
            .layer(
                ServiceBuilder::new()
                    .layer(CorsLayer::permissive()) // Enable CORS policy
                    .layer(layer),
            );
    
        let listener = tokio::net::TcpListener::bind("0.0.0.0:14655").await.unwrap();
            
        axum::serve(listener, app.into_make_service()).await.unwrap();
    }

}
