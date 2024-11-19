use std::{sync::Arc, time::Duration};
use crate::{http::start_web_server::start_httpd, oracles::{create_rpc_server_oracle, create_token_list_oracle, handle_user_address_oracle}, solana_state::{self}, utils::types::{ events::*, structs::bot::Bot }};
use parking_lot::RwLock;
use solana_client::rpc_client::RpcClient;
use crate::oracles::create_subscription_oracle;
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey};
use tokio::{ signal, task };

pub async fn start() {
    log::info!("Starting Bot");

    

    // ** prepare block oracle

    // hold all oracles inside bot struct
    let bot = Arc::new(
        RwLock::new(Bot::new())
    );

    let sol_client = Arc::new(RpcClient::new_with_timeout_and_commitment("http://192.168.100.98:18899", Duration::from_secs(240), CommitmentConfig::confirmed()));
    let arc_state = solana_state::get_solana_state();
    
    crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), 
        vec![
            String::from("Hc9iztjxoMiE9uv38WUvwzLqWCN153eF5mFSLZUecB7J")]);

    crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), 
        vec![
            String::from("SAGE2HAwep459SNq61LHvjxPk4pLPEJLoMETef7f7EE"), 
            String::from("traderDnaR5w6Tcoi3NFm53i48FTDNbGjBSZwWXDRrg"), 
            String::from("Cargo2VNTPPTi9c1vq1Jw5d3BWUNr18MjRtSupAghKEk"), 
            String::from("CRAFT2RPXPJWCEix4WpJST3E7NLf79GTqZUL75wngXo5"),
            String::from("pprofELXjL5Kck7Jn5hCpwAL82DpTkSYBENzahVtbc9")]);

    create_token_list_oracle::create_token_list(arc_state.clone(), sol_client.clone()).await;

    handle_user_address_oracle::add_user_address_to_index_with_all_child_with_sub(Pubkey::try_from("Hc9iztjxoMiE9uv38WUvwzLqWCN153eF5mFSLZUecB7J").unwrap(), arc_state.clone(), sol_client.clone());

    create_subscription_oracle::run(arc_state.clone(), sol_client.clone(), String::from("sage")).await;

    create_rpc_server_oracle::run(arc_state.clone(), sol_client.clone()).await;

    start_httpd();

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
