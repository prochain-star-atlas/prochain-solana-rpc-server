use std::{clone, sync::Arc};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use tokio_cron_scheduler::{Job, JobScheduler, JobSchedulerError};

use crate::solana_state::{ProchainAccountInfo, SolanaStateManager};



pub async fn create_cron_scheduler(state: Arc<SolanaStateManager>, sol_client: Arc<RpcClient>) -> Result<(), JobSchedulerError> {

    let mut sched = JobScheduler::new().await?;

    let cloned_state_0 = state.clone();
    let cloned_sol_client_0 = sol_client.clone();

    // Add basic cron job
    // sched.add(
    //     Job::new("0 */10 * * * *", move |_uuid, _l| {
            
    //         let cloned_state = cloned_state_0.clone();
    //         let cloned_sol_client = cloned_sol_client_0.clone();

    //         log::info!("starting scheduled job processing accounts");

    //         let all_keys = cloned_state.get_all_account_info_pubkey();
    //         all_keys.iter().for_each(|pk| {
                        
    //             let res = cloned_sol_client.get_account(&pk.clone());   
        
    //             if res.is_ok() {
        
    //                 let response = res.unwrap();
        
    //                 let p_key = pk;
    //                 let owner_key = Pubkey::try_from(response.owner).unwrap();
    //                 let tt = ProchainAccountInfo {
    //                     pubkey: p_key.clone(),
    //                     lamports: response.lamports,
    //                     executable: response.executable,
    //                     owner: owner_key.clone(),
    //                     rent_epoch: response.rent_epoch,
    //                     slot: 0,
    //                     write_version: 0,
    //                     txn_signature: None,
    //                     data: response.data.clone(),
    //                     last_update: chrono::offset::Utc::now()
    //                 };
        
    //                 cloned_state.add_account_info_without_owner(pk.clone(), tt);
        
    //             } else {
    //                 let err = res.err().unwrap();
    //                 log::error!("error calling get_account: {}", err);

    //                 if err.to_string().contains(&String::from("AccountNotFound")) {
    //                     cloned_state.clean_zero_account(pk.clone());
    //                 }
    //             }       
        
    //         }); 

    //         log::info!("finished scheduled job");

    //     })?
    // ).await?;

    // Start the scheduler
    sched.start().await?;
    
    Ok(())

}

