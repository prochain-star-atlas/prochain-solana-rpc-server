
use actix_web::{
    get,
    web::{Path, ServiceConfig},
    HttpResponse, Responder,
};
use solana_sdk::pubkey::Pubkey;
use utoipa::{OpenApi, ToSchema};
use crate::{model::model::GrpcYellowstoneSubscription, solana_state::ProchainAccountInfo};

#[derive(OpenApi)]
#[openapi(
    paths(
        get_solana_cached_acount_info,
    ),
    components(schemas(ProchainAccountInfo, GrpcYellowstoneSubscription))
)]
pub(super) struct SolanaApi;

pub(super) fn configure() -> impl FnOnce(&mut ServiceConfig) {
    |config: &mut ServiceConfig| {
        config
            .service(get_solana_cached_acount_info)
            .service(get_solana_subscription_settings);
    }
}

/// Get list of twich channel.
#[utoipa::path(
    responses(
        (status = 200, description = "get cached solana acco", body = [ProchainAccountInfo])
    )
)]
#[get("/solana/cached/account/{addr}")]
async fn get_solana_cached_acount_info(addr: Path<String>) -> impl Responder {
    let pk = Pubkey::try_from(addr.to_string().as_str()).unwrap();
    let state = crate::solana_state::get_solana_state();
    let acc = state.get_account_info(pk).unwrap().unwrap();
    HttpResponse::Ok().json(acc)
}

/// Get list of twich channel.
#[utoipa::path(
    responses(
        (status = 200, description = "get subscription settings", body = [GrpcYellowstoneSubscription])
    )
)]
#[get("/solana/subscription/settings")]
async fn get_solana_subscription_settings() -> impl Responder {
    let sub_name = String::from("Sage");
    let sage_program = crate::oracles::create_subscription_oracle::get_mutex_program_sub(sub_name.clone());
    let sage_account = crate::oracles::create_subscription_oracle::get_mutex_account_sub(sub_name.clone());

    let res1 = GrpcYellowstoneSubscription {
        name: sub_name.clone(),
        accounts: sage_account,
        owners: sage_program
    };

    HttpResponse::Ok().json(res1)
}