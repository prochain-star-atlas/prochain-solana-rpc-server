use std::{env, sync::Arc};

use crate::{cron::start_cron_scheduler::create_cron_scheduler, http::start_web_server, oracles::{
    create_socketio_server_oracle, 
    handle_user_address_oracle::add_user_address_to_index
}, rpc::rpc_service::Metrics, services::{subscription_account_service::SubscriptionAccountService, subscription_program_account_service::SubscriptionProgramAccountService, subscription_token_account_service::SubscriptionTokenAccountService, subscription_token_owner_account_service::SubscriptionTokenOwnerAccountService}, solana_state::{self}, utils::{helpers::load_env_vars, types::{ events::*, structs::bot::Bot }}};

use parking_lot::RwLock;
use tokio::{ signal, task };
use solana_sdk::pubkey;

pub async fn init_start() {

    let arc_state = solana_state::get_solana_state();

    crate::services::subscription_token_owner_account_service::set_mutex_token_owner_sub(String::from("sage"), 
        vec![
            String::from("Hc9iztjxoMiE9uv38WUvwzLqWCN153eF5mFSLZUecB7J")]);

    crate::services::subscription_account_service::set_mutex_account_sub(String::from("sage"), 
        vec![
            String::from("Hc9iztjxoMiE9uv38WUvwzLqWCN153eF5mFSLZUecB7J")]);

    crate::services::subscription_program_account_service::set_mutex_program_sub(String::from("sage"), 
        vec![
            String::from("GAMEzqJehF8yAnKiTARUuhZMvLvkZVAsCVri5vSfemLr"), 
            String::from("SAGE2HAwep459SNq61LHvjxPk4pLPEJLoMETef7f7EE"), 
            String::from("traderDnaR5w6Tcoi3NFm53i48FTDNbGjBSZwWXDRrg"), 
            String::from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk"), 
            String::from("CRAFT2RPXPJWCEix4WpJST3E7NLf79GTqZUL75wngXo5"),
            String::from("pprofELXjL5Kck7Jn5hCpwAL82DpTkSYBENzahVtbc9")]);

    add_user_address_to_index(pubkey!("GAMEzqJehF8yAnKiTARUuhZMvLvkZVAsCVri5vSfemLr"), arc_state.clone(), arc_state.get_sol_client().clone()).await;
    add_user_address_to_index(pubkey!("SAGE2HAwep459SNq61LHvjxPk4pLPEJLoMETef7f7EE"), arc_state.clone(), arc_state.get_sol_client().clone()).await;
    add_user_address_to_index(pubkey!("traderDnaR5w6Tcoi3NFm53i48FTDNbGjBSZwWXDRrg"), arc_state.clone(), arc_state.get_sol_client().clone()).await;
    add_user_address_to_index(pubkey!("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk"), arc_state.clone(), arc_state.get_sol_client().clone()).await;
    add_user_address_to_index(pubkey!("CRAFT2RPXPJWCEix4WpJST3E7NLf79GTqZUL75wngXo5"), arc_state.clone(), arc_state.get_sol_client().clone()).await;
    add_user_address_to_index(pubkey!("pprofELXjL5Kck7Jn5hCpwAL82DpTkSYBENzahVtbc9"), arc_state.clone(), arc_state.get_sol_client().clone()).await;

}

pub async fn start() {
    log::info!("Starting Bot");   

    let path = env::current_dir().unwrap();
    let _res = load_env_vars(&path);

    // ** prepare block oracle

    // hold all oracles inside bot struct
    let bot = Arc::new(
        RwLock::new(Bot::new())
    );

    let arc_state = solana_state::get_solana_state();

    init_start().await;

	let metrics = Metrics::default();
	let handle = crate::rpc::rpc_service::run_server(metrics.clone()).await;

    if handle.is_ok() {
        tokio::spawn(handle.unwrap().stopped());
    }

    create_socketio_server_oracle::start_socketio_httpd(arc_state.clone());

    start_web_server::start_httpd();

    let _cron = create_cron_scheduler().await;

    let _00 = SubscriptionAccountService::init_sub("sage".to_string()).await;
    let _11 = SubscriptionProgramAccountService::init_sub("sage".to_string()).await;

    let _0 = SubscriptionAccountService::restart().await;
    let _1 = SubscriptionProgramAccountService::restart().await;
    let _2 = SubscriptionTokenAccountService::restart().await;
    let _3 = SubscriptionTokenOwnerAccountService::restart().await;

    log::info!("All Oracles Started");

    let sleep = tokio::time::Duration::from_secs_f32(20.0);
    // keep the bot running
    tokio::select! {
        _ = signal::ctrl_c() => {
            println!("Main Loop CTRL+C received... exiting");
            let bot_guard = bot.read();
            let _1 = bot_guard.shutdown_event.0.send(ShutdownEvent::Shutdown {
                triggered: true
            });
        }
        _ = async {
            loop {
                tokio::time::sleep(sleep).await;
                task::yield_now().await;
            }
        } => {}
    }
}
