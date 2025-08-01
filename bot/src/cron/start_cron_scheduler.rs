use std::{clone, sync::Arc};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};

use crate::solana_state::{ProchainAccountInfo, SolanaStateManager};



pub async fn create_cron_scheduler() -> Result<(), JobSchedulerError> {

    let mut sched = JobScheduler::new().await?;

    // Add basic cron job
    let _1 = sched.add(
        Job::new_async("0 */10 * * * *", |_uuid, _lock| {
            
            return Box::pin(async move { 

                let arc_state = crate::solana_state::get_solana_state();
                let cloned_state = arc_state.clone();

                log::info!("starting scheduled job processing accounts");

                let all_keys = cloned_state.get_all_account_info_pubkey();
                for pk in all_keys.iter() {
                            
                    let res = cloned_state.get_sol_client().clone().get_account(&pk.clone()).await;   
            
                    if res.is_ok() {
            
                        let response = res.unwrap();
            
                        let p_key = pk;
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
            
                        cloned_state.add_account_info_without_owner(pk.clone(), tt);
            
                    } else {
                        let err = res.err().unwrap();
                        //log::error!("error calling get_account: {}", err);

                        if err.to_string().contains(&String::from("AccountNotFound")) {
                            cloned_state.clean_zero_account(pk.clone());
                        }
                    }       
            
                }

                log::info!("finished scheduled job");

            });

        })?
    ).await?;       

    // Start the scheduler
    sched.start().await?;
    
    Ok(())

}

