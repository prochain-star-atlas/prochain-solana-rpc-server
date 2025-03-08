
use itertools::Itertools;
use parking_lot::Mutex;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_request::TokenAccountsFilter;
use solana_sdk::pubkey::Pubkey;
use crate::solana_state::{ProchainAccountInfo, SolanaStateManager};
use std::sync::Arc;

pub async fn add_user_address_to_index_with_all_child_with_sub(addr: Pubkey, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {

    log::info!("starting add_user_address_to_index: {}", addr);

    let res = sol_client.get_token_accounts_by_owner(&addr, 
        TokenAccountsFilter::ProgramId(Pubkey::try_from("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap())).await;   

    if res.is_ok() {

        let response = res.unwrap();
        let response_len = response.len();
        log::info!("adding account {}, user: {}", response_len, addr);

        let count: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

        let mut vec_acc_new: Vec<String> = vec![];

        for acc in response.iter() {

            let pk_c = Pubkey::try_from(acc.pubkey.as_str()).unwrap();

            let acc = sol_client.get_account(&pk_c).await;

            if acc.is_ok() {

                let acc_res = acc.unwrap();

                let tt = ProchainAccountInfo {
                    pubkey: pk_c.clone(),
                    lamports: acc_res.lamports,
                    executable: acc_res.executable,
                    owner: acc_res.owner.clone(),
                    rent_epoch: acc_res.rent_epoch,
                    slot: 0,
                    write_version: 0,
                    txn_signature: None,
                    data: acc_res.data.clone(),
                    last_update: chrono::offset::Utc::now()
                };

                vec_acc_new.push(pk_c.to_string());

                state.add_account_info(pk_c.clone(), tt);

                let mut data = count.lock();
                *data += 1;
    
                if *data % 100 == 0 {
                    log::info!("[{}] adding account {} / {}", addr, *data, response_len);
                }

            }
            
        }

        let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
        vec_acc.append(&mut vec_acc_new);
        let v: Vec<_> = vec_acc.into_iter().unique().collect();
        crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), v);
        crate::oracles::create_subscription_oracle::refresh();

    }

}

pub async fn add_user_address_to_index_with_sub(addr: Pubkey, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {

    log::info!("starting add_user_address_to_index: {}", addr);

    let res = sol_client.get_account(&addr).await;   

    if res.is_ok() {

        let response = res.unwrap();
        log::info!("adding account {}", addr);

        let owner_key = response.owner.clone();
        let tt = ProchainAccountInfo {
            pubkey: addr.clone(),
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

        state.add_account_info(addr.clone(), tt);

        let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
        vec_acc.push(addr.to_string());
        let v: Vec<_> = vec_acc.into_iter().unique().collect();
        crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), v);
        crate::oracles::create_subscription_oracle::refresh();

    }

}

pub async fn add_user_address_to_index(addr: Pubkey, state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) {

    log::info!("starting add_user_address_to_index: {}", addr);

    let res = sol_client.get_account(&addr).await;   

    if res.is_ok() {

        let response = res.unwrap();
        log::info!("adding account {}", addr);

        let owner_key = response.owner.clone();
        let tt = ProchainAccountInfo {
            pubkey: addr.clone(),
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

        state.add_account_info(addr.clone(), tt);

    }

}