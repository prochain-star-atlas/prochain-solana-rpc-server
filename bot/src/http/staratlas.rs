
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
        get_staratlas_fleet_subscription_all,
        get_staratlas_fleet_subscription_by_user_id,
        remove_staratlas_fleet_subscription_by_id,
        remove_staratlas_fleet_sub_all
    ),
    components(schemas(FleetSubscription))
)]
pub(super) struct StarAtlasApi;

pub(super) fn configure() -> impl FnOnce(&mut ServiceConfig) {
    |config: &mut ServiceConfig| {
        config
            .service(get_staratlas_fleet_subscription_all)
            .service(get_staratlas_fleet_subscription_by_user_id)
            .service(remove_staratlas_fleet_subscription_by_id)
            .service(remove_staratlas_fleet_sub_all);
    }
}

#[utoipa::path(
    responses(
        (status = 200, description = "get staratlas fleet subscriptions", body = [Vec<FleetSubscription>])
    )
)]
#[get("/staratlas/fleet/subscription/all")]
async fn get_staratlas_fleet_subscription_all() -> impl Responder {

    let lst_fleet_sub = crate::oracles::create_socketio_server_oracle::get_all_values_sub();

    HttpResponse::Ok().json(lst_fleet_sub)

}

#[utoipa::path(
    responses(
        (status = 200, description = "get staratlas fleet subscriptions by user id", body = [Vec<FleetSubscription>])
    )
)]
#[get("/staratlas/fleet/subscription/byuserid/{user_id}")]
async fn get_staratlas_fleet_subscription_by_user_id(user_id: Path<String>) -> impl Responder {

    let lst_fleet_sub = crate::oracles::create_socketio_server_oracle::get_all_values_sub_by_user_id(user_id.to_string());

    HttpResponse::Ok().json(lst_fleet_sub)

}

#[utoipa::path(
    responses(
        (status = 200, description = "remove staratlas fleet subscription", body = [bool])
    )
)]
#[put("/staratlas/fleet/subscription/remove/{sub_name}")]
async fn remove_staratlas_fleet_subscription_by_id(sub_name: Path<String>) -> impl Responder {

    let _1 = crate::oracles::create_socketio_server_oracle::remove_mutex_fleet_sub(sub_name.to_string());

    HttpResponse::Ok().json(true)

}

#[utoipa::path(
    responses(
        (status = 200, description = "remove staratlas fleet subscription", body = [bool])
    )
)]
#[put("/staratlas/fleet/subscription/removeall")]
async fn remove_staratlas_fleet_sub_all() -> impl Responder {

    let _1 = crate::oracles::create_socketio_server_oracle::remove_all_fleet_sub();

    HttpResponse::Ok().json(true)

}