
use std::time::Duration;

use actix_web::{
    get, put, web::{Path, ServiceConfig}, HttpResponse, Responder
};
use serde_json::json;
use solana_client::{rpc_client::RpcClient, rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTokenAccountsFilter}, rpc_request::RpcRequest, rpc_response::{RpcKeyedAccount, RpcResult}};
use solana_sdk::{commitment_config::{CommitmentConfig, CommitmentLevel}, pubkey::Pubkey};
use utoipa::OpenApi;
use crate::{model::model::GrpcYellowstoneSubscription, oracles::create_socketio_server_oracle::FleetSubscription, solana_state::{ProchainAccountInfo, ProchainAccountInfoSchema}};

#[derive(OpenApi)]
#[openapi(
    paths(
        get_solana_cached_acount_info,
        get_solana_cached_refresh_acount,
        get_solana_cached_close_acount,
        get_solana_cached_subscription_owner,
        get_solana_cached_subscription_tokenowner,
        get_solana_cached_subscription_account,
        get_solana_cached_subscription_token_account,
        get_solana_subscription_settings
    ),
    components(schemas(ProchainAccountInfoSchema, GrpcYellowstoneSubscription, FleetSubscription))
)]
pub(super) struct SolanaApi;

pub(super) fn configure() -> impl FnOnce(&mut ServiceConfig) {
    |config: &mut ServiceConfig| {
        config
            .service(get_solana_cached_acount_info)
            .service(get_solana_cached_refresh_acount)
            .service(get_solana_cached_close_acount)
            .service(get_solana_cached_subscription_owner)
            .service(get_solana_cached_subscription_tokenowner)
            .service(get_solana_cached_subscription_account)
            .service(get_solana_cached_subscription_token_account)
            .service(get_solana_subscription_settings);
    }
}

/// Get list of twich channel.
#[utoipa::path(
    responses(
        (status = 200, description = "get cached solana acco", body = [ProchainAccountInfoSchema])
    )
)]
#[get("/solana/cached/account/{addr}")]
async fn get_solana_cached_acount_info(addr: Path<String>) -> impl Responder {

    let pk = Pubkey::try_from(addr.to_string().as_str()).unwrap();
    let state = crate::solana_state::get_solana_state();
    let acc = state.get_account_info(pk).unwrap().unwrap();

    let acc_info = ProchainAccountInfoSchema { 

        pubkey: acc.pubkey.to_string(), 
        lamports: acc.lamports, 
        owner: acc.owner.to_string(), 
        executable: acc.executable, 
        rent_epoch: acc.rent_epoch, 
        data: acc.data, 
        slot: acc.slot, 
        write_version: acc.write_version, 
        txn_signature: acc.txn_signature, 
        last_update: acc.last_update
        
    };

    HttpResponse::Ok().json(acc_info)

}

#[utoipa::path(
    responses(
        (status = 200, description = "closing solana account", body = [bool])
    )
)]
#[get("/solana/cached/close/account/{addr}")]
async fn get_solana_cached_close_acount(addr: Path<String>) -> impl Responder {

    let pk = Pubkey::try_from(addr.to_string().as_str()).unwrap();
    let state = crate::solana_state::get_solana_state();
    let acc = state.get_account_info(pk);
    
    if acc.is_some() {
        let acc_opt = acc.unwrap();

        if acc_opt.is_ok() {

            let res_acc = acc_opt.unwrap();

            if res_acc.lamports > 0 {
                state.clean_zero_account(pk);
            }

        }

    }

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "closing solana account", body = [bool])
    )
)]
#[get("/solana/cached/refresh/account/{addr}")]
async fn get_solana_cached_refresh_acount(addr: Path<String>) -> impl Responder {

    let pk = Pubkey::try_from(addr.to_string().as_str()).unwrap();

    let sol_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(String::from("http://192.168.100.98:18899"), Duration::from_secs(240), CommitmentConfig::confirmed());
    let state = crate::solana_state::get_solana_state();

    let res_simple_account = sol_client.get_account(&pk.clone()).await;

    if res_simple_account.is_ok() {

        let res1 = res_simple_account.unwrap();

        state.add_account_info(pk.clone(), ProchainAccountInfo {
            data: res1.data.clone(),
            executable: res1.executable,
            lamports: res1.lamports,
            owner: Pubkey::try_from(res1.owner.to_bytes().to_vec()).unwrap(),
            pubkey: pk.clone(),
            rent_epoch: res1.rent_epoch,
            slot: 0,
            txn_signature: None,
            write_version: 0,
            last_update: chrono::offset::Utc::now()
        });
    }

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "add subscription by owner", body = [bool])
    )
)]
#[get("/solana/cached/subscription/owner/{owner}")]
async fn get_solana_cached_subscription_owner(owner: Path<String>) -> impl Responder {

    let state = crate::solana_state::get_solana_state();
    let pubkey_owner = Pubkey::try_from(owner.to_string().as_str()).unwrap();
    let sol_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(String::from("http://192.168.100.98:18899"), Duration::from_secs(240), CommitmentConfig::confirmed());
    let config = RpcProgramAccountsConfig { filters: None, account_config: RpcAccountInfoConfig { encoding: None, data_slice: None, commitment: Some(CommitmentConfig { commitment: CommitmentLevel::Confirmed }), min_context_slot: None }, with_context: None, sort_results: None };

    let res = sol_client.get_program_accounts_with_config(&pubkey_owner, config.clone()).await;       
    let ar_results = res.unwrap();

    for f in ar_results {
        let res_simple_account = sol_client.get_account(&f.0.clone()).await.unwrap();

        state.add_account_info(f.0.clone(), ProchainAccountInfo {
            data: res_simple_account.data.clone(),
            executable: res_simple_account.executable,
            lamports: res_simple_account.lamports,
            owner: Pubkey::try_from(res_simple_account.owner.to_bytes().to_vec()).unwrap(),
            pubkey: f.0.clone(),
            rent_epoch: res_simple_account.rent_epoch,
            slot: 0,
            txn_signature: None,
            write_version: 0,
            last_update: chrono::offset::Utc::now()
        });
    }

    //self.sol_state. (program_id.clone(), ar_pkey);
    let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));

    if !vec_acc.contains(&owner.to_string()) {

        vec_acc.push(owner.to_string());
        crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), vec_acc);
        crate::oracles::create_subscription_oracle::refresh_owner();

    }

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "add subscription by token owner", body = [bool])
    )
)]
#[get("/solana/cached/subscription/tokenowner/{tokenowner}")]
async fn get_solana_cached_subscription_tokenowner(tokenowner: Path<String>) -> impl Responder {

    let program_id_str = tokenowner.to_string();
    let token_account_filter = RpcTokenAccountsFilter::ProgramId(tokenowner.to_string());
    let config = Some( RpcAccountInfoConfig { encoding: None, data_slice: None, commitment: Some(CommitmentConfig { commitment: CommitmentLevel::Confirmed }), min_context_slot: None } );

    let pb_owner = Pubkey::try_from(tokenowner.as_str()).unwrap();  
    let state = crate::solana_state::get_solana_state();
    let sol_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(String::from("http://192.168.100.98:18899"), Duration::from_secs(240), CommitmentConfig::confirmed());

    let acc_owner = sol_client.get_account(&pb_owner).await.unwrap();    

    state.add_account_info(pb_owner, ProchainAccountInfo {
        data: acc_owner.data.clone(),
        executable: acc_owner.executable,
        lamports: acc_owner.lamports,
        owner: acc_owner.owner.clone(),
        pubkey: pb_owner.clone(),
        rent_epoch: acc_owner.rent_epoch,
        slot: 0,
        txn_signature: None,
        write_version: 0,
        last_update: chrono::offset::Utc::now()
    });

    let res_rpc: RpcResult<Vec<RpcKeyedAccount>> = sol_client.send(
        RpcRequest::GetTokenAccountsByOwner,
        json!([program_id_str, token_account_filter, config]),
    ).await;

    let ar_results = res_rpc.unwrap().clone();

    let mut ar_pkey: Vec<Pubkey> = vec![];

    let mut vec_acc_o = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));

    for f in ar_results.value {

        let pb = Pubkey::try_from(f.pubkey.as_str()).unwrap();  
        let acc_raw = sol_client.get_account(&pb).await.unwrap();       
        ar_pkey.push(pb);

        let pca = ProchainAccountInfo {
            data: acc_raw.data.clone(),
            executable: acc_raw.executable,
            lamports: acc_raw.lamports,
            owner: acc_raw.owner.clone(),
            pubkey: pb.clone(),
            rent_epoch: acc_raw.rent_epoch,
            slot: 0,
            txn_signature: None,
            write_version: 0,
            last_update: chrono::offset::Utc::now()
        };

        state.add_account_info(pb.clone(), pca.clone());

        if !vec_acc_o.contains(&pb.clone().to_string()) {

            vec_acc_o.push(pb.clone().to_string());
    
        }

    }

    crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_acc_o);

    let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_token_owner_sub(String::from("sage"));
    
    if !vec_acc.contains(&program_id_str) {

        vec_acc.push(program_id_str);
        crate::oracles::create_subscription_oracle::set_mutex_token_owner_sub(String::from("sage"), vec_acc);

    }

    crate::oracles::create_subscription_oracle::refresh_token_account();
    crate::oracles::create_subscription_oracle::refresh_token_owner();
    
    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "add subscription by token account", body = [bool])
    )
)]
#[get("/solana/cached/subscription/token/{account}")]
async fn get_solana_cached_subscription_token_account(account: Path<String>) -> impl Responder {

    let pb_token = Pubkey::try_from(account.as_str()).unwrap();  
    let state = crate::solana_state::get_solana_state();
    let sol_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(String::from("http://192.168.100.98:18899"), Duration::from_secs(240), CommitmentConfig::confirmed());
    
    let acc_raw = sol_client.get_account(&pb_token).await.unwrap();       

    let pca = ProchainAccountInfo {
        data: acc_raw.data.clone(),
        executable: acc_raw.executable,
        lamports: acc_raw.lamports,
        owner: acc_raw.owner.clone(),
        pubkey: pb_token.clone(),
        rent_epoch: acc_raw.rent_epoch,
        slot: 0,
        txn_signature: None,
        write_version: 0,
        last_update: chrono::offset::Utc::now()
    };

    state.add_account_info(pb_token.clone(), pca.clone());

    let mut vec_token_sub = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
    
    if !vec_token_sub.contains(&pb_token.clone().to_string()) {

        vec_token_sub.push(pb_token.clone().to_string());
        crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_token_sub.clone());
        crate::oracles::create_subscription_oracle::refresh_token_owner();

    }

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "add subscription by account", body = [bool])
    )
)]
#[get("/solana/cached/subscription/account/{account}")]
async fn get_solana_cached_subscription_account(account: Path<String>) -> impl Responder {

    let config = RpcAccountInfoConfig { 

        encoding: None, 
        data_slice: None, 
        commitment: Some(CommitmentConfig { commitment: CommitmentLevel::Confirmed }), 
        min_context_slot: None

    };

    let state = crate::solana_state::get_solana_state();
    let pubkey = Pubkey::try_from(account.to_string().as_str()).unwrap();
    let sol_client = solana_client::nonblocking::rpc_client::RpcClient::new_with_timeout_and_commitment(String::from("http://192.168.100.98:18899"), Duration::from_secs(240), CommitmentConfig::confirmed());
    let res = sol_client.get_account_with_config(&pubkey, config).await.unwrap();

    let acc_pk = res.value.clone().unwrap_or_default();

    let pa = ProchainAccountInfo {
        data: acc_pk.data,
        executable: acc_pk.executable,
        lamports: acc_pk.lamports,
        owner: Pubkey::try_from(acc_pk.owner).unwrap(),
        pubkey: pubkey.clone(),
        rent_epoch: acc_pk.rent_epoch,
        slot: 0,
        txn_signature: None,
        write_version: 0,
        last_update: chrono::offset::Utc::now()
    };

    if res.value.clone().is_some() {

        state.add_account_info(pubkey.clone(), pa.clone());
        let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
        if !vec_acc.contains(&account.to_string()) {

            vec_acc.push(pubkey.to_string());
            state.add_account_info(pubkey.clone(), pa.clone());
            crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
            crate::oracles::create_subscription_oracle::refresh();
            
        }

    }

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "get subscription settings", body = [GrpcYellowstoneSubscription])
    )
)]
#[get("/solana/subscription/settings")]
async fn get_solana_subscription_settings() -> impl Responder {
    let sub_name = String::from("sage");
    let sage_program = crate::oracles::create_subscription_oracle::get_mutex_program_sub(sub_name.clone());
    let sage_account = crate::oracles::create_subscription_oracle::get_mutex_account_sub(sub_name.clone());
    let sage_token_account = crate::oracles::create_subscription_oracle::get_mutex_token_sub(sub_name.clone());
    let sage_token_owner_account = crate::oracles::create_subscription_oracle::get_mutex_token_owner_sub(sub_name.clone());

    let res1 = GrpcYellowstoneSubscription {
        name: sub_name.clone(),
        accounts: sage_account,
        token_accounts: sage_token_account,
        token_owner: sage_token_owner_account,
        owners: sage_program
    };

    HttpResponse::Ok().json(res1)
}
