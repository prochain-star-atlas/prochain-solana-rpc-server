use std::{collections::HashMap, str::FromStr, sync::atomic::AtomicUsize};

use parking_lot::Mutex;
use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};
use socketioxide::{
    extract::{AckSender, Data, SocketRef, State},
    SocketIo,
};
use solana_account_decoder::{parse_token::UiTokenAccount, UiAccountData, UiAccountEncoding};
use solana_client::{rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTokenAccountsFilter}, rpc_filter::{Memcmp, MemcmpEncodedBytes, RpcFilterType}};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use static_init::dynamic;
use utoipa::ToSchema;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, services::ServeDir};
use solana_account_decoder::UiAccountData::{Json, Binary, LegacyBinary};
use log::error;
use std::sync::atomic::Ordering;
use solana_client::rpc_response::OptionalContext::Context;
use solana_client::rpc_response::OptionalContext::NoContext;
use crate::{rpc::rpc_client_service::RpcClientService, services::{subscription_fleet_owner_service::SubscriptionFleetOwnerService, subscription_fleet_service::SubscriptionFleetService}, solana_state::SolanaStateManager, utils::types::structs::prochain::{UserFleetCargoItem, UserFleetInstanceRequest, UserFleetInstanceResponse}};

use {
    futures::{sink::SinkExt, stream::StreamExt},
    maplit::hashmap,
    yellowstone_grpc_client::GeyserGrpcClient,
    yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, SubscribeRequest,
        SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterAccounts
    },
};

use dashmap::DashMap;
use tracing::info;

#[dynamic] 
static LIST_FLEET_SUBSCRIPTION: Mutex<DashMap<String, FleetSubscription>> = Mutex::new(DashMap::new());

pub fn get_subs_from_accounts(addr: String) -> Vec<FleetSubscription> {
    let all_subs: Vec<FleetSubscription> = LIST_FLEET_SUBSCRIPTION.lock().iter().map(|f| { f.value().clone() }).collect();

    let mut vec_ret: Vec<FleetSubscription> = vec![];
    for t in all_subs {
        if t.owner_address.contains(&addr) || t.account_address.contains(&addr) {
            vec_ret.push(t);
        }
    }

    return vec_ret;
}

pub fn set_mutex_fleet_sub(sub_name: String, lst_vec: FleetSubscription) {
    LIST_FLEET_SUBSCRIPTION.lock().insert(sub_name.clone(), lst_vec);
    tokio::spawn(async {
        let _0 = SubscriptionFleetService::restart().await;
        let _1 = SubscriptionFleetOwnerService::restart().await;
    });
}

pub fn remove_mutex_fleet_sub_sid(sid: String) {
    let all_keys_subs: Vec<String> = LIST_FLEET_SUBSCRIPTION.lock().iter().map(|f| { f.key().clone() }).collect();
    for a in all_keys_subs {

        if !a.starts_with(&sid) {
            continue;
        }

        LIST_FLEET_SUBSCRIPTION.lock().remove(&a.clone());
    }
}

pub fn remove_mutex_fleet_sub(sub_name: String) {

    LIST_FLEET_SUBSCRIPTION.lock().remove(&sub_name.clone());
    
    tokio::spawn(async {
        let _0 = SubscriptionFleetService::restart().await;
        let _1 = SubscriptionFleetOwnerService::restart().await;
    });

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

pub fn get_mutex_fleet_sub(sub_name: String) -> Option<FleetSubscription> {
    let map = LIST_FLEET_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return None; },
        Some(val) => { Some(val.value().clone()) }
    }
}

pub fn start_socketio_httpd(state: Arc<SolanaStateManager>) {

    log::info!("Starting SocketIO httpd !");
    let _1 = std::thread::spawn(|| {
    
        let _t = run(state).is_ok();
        log::info!("SocketIO httpd started !");

    });

}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct FleetSubscriptionSummary {
    pub id_sub: String,
    pub account_address: Vec<String>,
    pub owner_address: Vec<String>
}

#[derive(Clone, Debug)]
pub struct FleetSubscription {
    pub id_sub: String,
    pub ufi: UserFleetInstanceRequest,
    pub account_address: Vec<String>,
    pub owner_address: Vec<String>,
    pub socket: Option<Arc<SocketRef>>
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

pub async fn create_subscription_for_fleet(key: String, ufi: UserFleetInstanceRequest, socket: Arc<SocketRef>) -> Result<FleetSubscription, anyhow::Error> {

    let mut fleet_sub = FleetSubscription { id_sub: key, account_address: vec![], owner_address: vec![], socket: Some(socket), ufi: ufi.clone() };

    fleet_sub.account_address.push(ufi.clone().publicKey.to_string());
    fleet_sub.owner_address.push(ufi.clone().publicKey.to_string());

    let rpc_token_account_filter = RpcTokenAccountsFilter::ProgramId("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA".to_string());

    fleet_sub.owner_address.push(ufi.clone().cargoHold.clone());
    fleet_sub.account_address.push(ufi.clone().foodToken);
    fleet_sub.account_address.push(ufi.clone().sduToken);
    
    fleet_sub.owner_address.push(ufi.clone().fuelTank.clone());
    fleet_sub.account_address.push(ufi.clone().fuelToken);

    fleet_sub.owner_address.push(ufi.clone().ammoBank.clone());
    fleet_sub.account_address.push(ufi.clone().ammoToken);

    let rpc_acc_info_req_2 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let rpc_prog_info = RpcProgramAccountsConfig {
        filters: Some(vec![
            RpcFilterType::Memcmp(Memcmp::new(0, MemcmpEncodedBytes::Base58(String::from_str("UcyYNefQ2BW")?))),
            RpcFilterType::Memcmp(Memcmp::new(41, MemcmpEncodedBytes::Base58(ufi.clone().publicKey.to_string())))]),
        account_config: rpc_acc_info_req_2,
        with_context: None,
        sort_results: None,
    };

    let starbase_player_cargo_holds = RpcClientService::new().get_program_accounts(&Pubkey::try_from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk")?, Some(rpc_prog_info), Some(ufi.clone().forceRefresh)).await?;

    let rp1 = match starbase_player_cargo_holds {

        Context(ctx) => {
            
            for rka in ctx.value {

                let rpc_acc_info_req_cargo = RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::JsonParsed),
                    data_slice: None,
                    commitment: Some(CommitmentConfig::confirmed()),
                    min_context_slot: None,
                };
            
                let fleet_current_cargo = 
                    RpcClientService::new().get_token_account_by_owner(
                        rka.pubkey, 
                        rpc_token_account_filter.clone(), 
                    Some(rpc_acc_info_req_cargo), 
                    Some(ufi.clone().forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    fleet_sub.account_address.push(fcc.pubkey);

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
            
                let fleet_current_cargo = 
                    RpcClientService::new().get_token_account_by_owner(
                        rka.pubkey, 
                        rpc_token_account_filter.clone(), 
                Some(rpc_acc_info_req_cargo), 
         Some(ufi.clone().forceRefresh)).await?;
                
                for fcc in fleet_current_cargo.value {

                    fleet_sub.account_address.push(fcc.pubkey);

                }

            }

        }
    };

    log::info!("fleet_sub: {:?}", fleet_sub);

    Ok(fleet_sub) 

}

pub async fn refresh_fleet(ufi: UserFleetInstanceRequest) -> Result<UserFleetInstanceResponse, anyhow::Error> {

    let pub_key = Pubkey::try_from(ufi.publicKey.as_str())?;

    let rpc_acc_info_req_1 = RpcAccountInfoConfig {
        encoding: Some(UiAccountEncoding::Base64),
        data_slice: None,
        commitment: Some(CommitmentConfig::confirmed()),
        min_context_slot: None,
    };

    let fleet_acc_info = RpcClientService::new().get_account_info(&pub_key, Some(rpc_acc_info_req_1), Some(ufi.forceRefresh)).await?;
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

    let fleet_current_cargo = RpcClientService::new().get_token_account_by_owner(ufi.cargoHold.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_3), Some(ufi.forceRefresh)).await?;
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

    let fleet_current_fuel = RpcClientService::new().get_token_account_by_owner(ufi.fuelTank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_4), Some(ufi.forceRefresh)).await?;
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

    let fleet_current_ammo = RpcClientService::new().get_token_account_by_owner(ufi.ammoBank.clone(), rpc_token_account_filter.clone(), Some(rpc_acc_info_req_5), Some(ufi.forceRefresh)).await?;
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

    let starbase_player_cargo_holds = RpcClientService::new().get_program_accounts(&Pubkey::try_from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk").unwrap(), Some(rpc_prog_info), Some(ufi.forceRefresh)).await?;

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
            
                let fleet_current_cargo = RpcClientService::new().get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
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
            
                let fleet_current_cargo = RpcClientService::new().get_token_account_by_owner(rka.pubkey, rpc_token_account_filter.clone(), Some(rpc_acc_info_req_cargo), Some(ufi.forceRefresh)).await?;
                
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
pub async fn run(state: Arc<SolanaStateManager>) -> Result<(), Box<dyn std::error::Error>> {

    let state_local = state.clone();

    loop {

        log::info!("Starting sockerio server on 0.0.0.0:14655");

        let (layer, io) = SocketIo::builder().with_state(UserCnt::new()).build_layer();
    
        io.ns("/", move |s: SocketRef| {
        
            s.on(
                "subscribeToFleetChange",
                |s: SocketRef, Data::<UserFleetInstanceRequest>(ufi), user_cnt: State<UserCnt>| async move {
    
                    user_cnt.add_user(s.id.to_string());
    
                    let s_id_key: String = s.id.to_string().replace("-", "").chars().skip(0).take(10).collect();
                    let user_id_key: String = ufi.userId.replace("-", "").chars().skip(0).take(10).collect();
                    let key = s_id_key + "-" + user_id_key.as_str() + "-" + &ufi.publicKey;
    
                    let vec_current_sub = get_all_values_sub();
    
                    if !vec_current_sub.iter().any(|f| { f.id_sub == key }) {
    
                        log::info!("[SOCKETIO] subscribeToFleetChange for id: {:?}", key);
    
                        tokio::spawn(async move {
        
                            let fleet_subscription = create_subscription_for_fleet(key.clone(), ufi.clone(), Arc::new(s)).await;
                            if fleet_subscription.is_ok() {
                                set_mutex_fleet_sub(key.clone(), fleet_subscription.unwrap());
                                tokio::spawn(async {
                                    let _0 = SubscriptionFleetService::restart().await;
                                    let _1 = SubscriptionFleetOwnerService::restart().await;
                                });
                            } else {
                                log::error!("Error creating the fleet subscription {:?}", ufi.clone().publicKey);
                            }
        
                        });  
    
                    }
                    
                },
            );
        
            s.on(
                "forceRefreshFleet",
                |s: SocketRef, Data::<UserFleetInstanceRequest>(ufi), ack: AckSender| async move {
    
                    log::info!("[SOCKETIO] force_fleet_refreshed for pubkey: {:?}", ufi.publicKey);
    
                    tokio::spawn(async move {
    
                        let fleet_refreshed = refresh_fleet(ufi.clone()).await;
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
