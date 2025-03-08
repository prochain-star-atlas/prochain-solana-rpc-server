use std::sync::Arc;

use crate::{cron::start_cron_scheduler::create_cron_scheduler, http::start_web_server, oracles::{
    create_rpc_server_oracle, 
    create_socketio_server_oracle, 
    create_token_list_oracle, 
    handle_user_address_oracle::{self, add_user_address_to_index}
}, rpc::rpc_service::JsonRpcConfig, solana_state::{self}, utils::types::{ events::*, structs::bot::Bot }};

use parking_lot::RwLock;
use solana_client::rpc_request::MAX_MULTIPLE_ACCOUNTS;
use crate::oracles::create_subscription_oracle;
use tokio::{ signal, task };
use solana_sdk::pubkey;

pub async fn init_start() {

    let arc_state = solana_state::get_solana_state();

    crate::oracles::create_subscription_oracle::set_mutex_token_owner_sub(String::from("sage"), 
        vec![
            String::from("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")]);

    crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), 
        vec![]);

    crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), 
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

    // ** prepare block oracle

    // hold all oracles inside bot struct
    let bot = Arc::new(
        RwLock::new(Bot::new())
    );

    let arc_state = solana_state::get_solana_state();

    init_start().await;

    create_token_list_oracle::create_token_list(arc_state.clone(), arc_state.get_sol_client().clone()).await;

    create_subscription_oracle::init_sub(arc_state.clone(), arc_state.get_sol_client().clone(), String::from("sage")).await;

    create_subscription_oracle::run(arc_state.clone(), String::from("sage")).await;

    let default_rpc_max_multiple_accounts = MAX_MULTIPLE_ACCOUNTS;
    let config: JsonRpcConfig = JsonRpcConfig {
        max_multiple_accounts: Some(default_rpc_max_multiple_accounts),
        rpc_threads: 8,
        rpc_niceness_adj: 0,
    };

    create_rpc_server_oracle::run(config.clone(), arc_state.clone(), arc_state.get_sol_client().clone()).await;

    create_socketio_server_oracle::start_socketio_httpd(config.clone(), arc_state.clone(), arc_state.get_sol_client().clone());

    start_web_server::start_httpd();

    let _cron = create_cron_scheduler().await;

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
            create_subscription_oracle::exit_subscription();
        }
        _ = async {
            loop {
                tokio::time::sleep(sleep).await;
                task::yield_now().await;
            }
        } => {}
    }
}
