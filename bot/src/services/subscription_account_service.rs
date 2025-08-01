use carbon_core::account::AccountDecoder;
use carbon_core::datasource::DatasourceId;
use solana_pubkey::Pubkey;
use static_init::dynamic;
use parking_lot::{Mutex};
use tokio::{signal, task};
use yellowstone_grpc_proto::geyser::SubscribeUpdateAccountInfo;
use std::ops::Add;
use std::str::FromStr;
use std::{env, time::Duration};
use std::sync::Arc;
use dashmap::DashMap;
use crate::services::subscription_deletion_service::SubscriptionDeletionService;
use crate::utils::helpers::load_env_vars;
use crate::utils::prochain_datasource::ProchainYellowstoneGrpcGeyserClient;

use {
    async_trait::async_trait,
    carbon_core::{
        deserialize::ArrangeAccounts,
        error::CarbonResult,
        account::{DecodedAccount, AccountMetadata},
        instruction::{DecodedInstruction, InstructionMetadata, NestedInstructions},
        metrics::MetricsCollection,
        processor::Processor,
    },
    std::{
        collections::{HashMap, HashSet}
    },
    tokio::sync::RwLock,
    yellowstone_grpc_proto::geyser::{
        SubscribeRequestFilterAccounts, SubscribeRequestFilterTransactions,
    },
    tokio_util::sync::CancellationToken
};

use crate::solana_state::{self, get_solana_state, ProchainAccountInfo};

#[dynamic] 
static JOINHANDLE_REF: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::from(None));

#[dynamic] 
static COUNT_REF: Arc<Mutex<u32>> = Arc::new(Mutex::from(0));

#[dynamic] 
static LIST_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());

pub fn set_mutex_account_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_account_sub(sub_name: String) -> Vec<String> {
    let map = LIST_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn reset_all_list_sub() {
    LIST_ACCOUNT_SUBSCRIPTION.lock().clear();
}

#[derive(Debug, Clone, Default)]
pub struct SubscriptionAccountService {
}

impl SubscriptionAccountService {

    pub async fn init_sub(sub_name: String) {

        let arc_state = solana_state::get_solana_state();
        let local_arc = arc_state.clone();

        let accounts = get_mutex_account_sub(sub_name.clone());   
        for acc in accounts.iter() {

            log::info!("starting processing account: {}", acc);

            let res = local_arc.get_sol_client().get_account(&Pubkey::try_from(acc.as_str()).unwrap()).await;   

            if res.is_ok() {

                let response = res.unwrap();
                log::info!("adding account {}", acc);

                let p_key = Pubkey::try_from(acc.as_str()).unwrap();
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

                local_arc.add_account_info(p_key, tt);

            } else {
                log::error!("error calling get_account: {}", res.err().unwrap());
            }       

            log::info!("finished processing accounts: {}", acc);

        }

    }

    pub async fn restart() {

        let jh = SubscriptionAccountService::start_monitor().await;

        tokio::time::sleep(Duration::from_millis(5000)).await;

        if JOINHANDLE_REF.lock().is_some() {
            let mut data = JOINHANDLE_REF.lock();
            let ojh = data.as_mut().unwrap();
            ojh.cancel();
        }

        if jh.is_ok() {

            let mut data = JOINHANDLE_REF.lock();
            *data = Some(jh.unwrap());

        }

        SubscriptionDeletionService::check_and_restart_deletion_sub();

    }

    pub async fn start_monitor() -> Result<CancellationToken, anyhow::Error> {
        
        let list_add: Vec<String> = get_mutex_account_sub("sage".to_string()).into_iter().map(|f| { f }).collect();
        let mut hs_pk: HashSet<Pubkey> = HashSet::new();

        if list_add.len() < 1 {
            anyhow::bail!("error in account service no accounts")
        }

        for pk in list_add.clone() {
            hs_pk.insert(Pubkey::from_str(pk.as_str()).unwrap());
        }

        let path = env::current_dir().unwrap();
        let _res = load_env_vars(&path);

        let url_solana_geyser = std::env::var("SOL_GEYSER_YELLOWSTONE").unwrap();

        // 3 - Initialize account filters
        let mut account_filters: HashMap<String, SubscribeRequestFilterAccounts> = HashMap::new();

        let mut data_count = COUNT_REF.lock();
        *data_count = data_count.add(1);

        account_filters.insert(
            "sas_".to_string() + data_count.to_string().as_str(),
            SubscribeRequestFilterAccounts {
                nonempty_txn_signature: None,
                account: list_add.clone(),
                owner: vec![],
                filters: vec![],
            },
        );

        // 4 - Initialize transaction filter

        let transaction_filters: HashMap<String, SubscribeRequestFilterTransactions> = HashMap::new();

        // 5 - Initialize Yellowstone Geyser gRPC Client
        let yellowstone_grpc = ProchainYellowstoneGrpcGeyserClient::new(
            url_solana_geyser,
            None,
            Some(yellowstone_grpc_proto::geyser::CommitmentLevel::Confirmed),
            account_filters,
            transaction_filters,
            crate::utils::prochain_datasource::BlockFilters {
                filters: HashMap::new(),
                failed_transactions: None
            },
            Arc::new(RwLock::new(HashSet::new()))
        );

        let datasource_cancellation_token = CancellationToken::new(); 

        let datasource_id = DatasourceId::new_unique();

        // 6 - Build and run the Carbon pipeline
        let mut pipeline = carbon_core::pipeline::Pipeline::builder()
            .datasource_with_id(yellowstone_grpc, datasource_id.clone())
            .datasource_cancellation_token(datasource_cancellation_token.clone())
            .account(GenericAccountDecoder, GenericAccountProcessor)
            .shutdown_strategy(carbon_core::pipeline::ShutdownStrategy::Immediate)
            .build()?;

        let _thread = tokio::spawn(async move {
            if let Err(e) = pipeline.run().await {
                log::error!("Pipeline run error: {:?}", e);
            }
        });

        log::info!("Start subscription for subscription_account_service ...");

        return Ok(datasource_cancellation_token.clone());

    }

}

pub struct GenericAccount {
    state: bool
}

pub struct GenericAccountDecoder;

impl AccountDecoder<'_> for GenericAccountDecoder {
    type AccountType = GenericAccount;
    fn decode_account(
        &self,
        account: &solana_account::Account,
    ) -> Option<carbon_core::account::DecodedAccount<Self::AccountType>> {
        return Some(carbon_core::account::DecodedAccount {
                lamports: account.lamports,
                data: GenericAccount { state: true },
                owner: account.owner,
                executable: account.executable,
                rent_epoch: account.rent_epoch,
            });
    }
}

pub struct GenericAccountProcessor;
#[async_trait]
impl Processor for GenericAccountProcessor {
    type InputType = (
        AccountMetadata,
        DecodedAccount<GenericAccount>,
        solana_account::Account
    );

    async fn process(
        &mut self,
        (metadata, decoded_account, account): Self::InputType,
        _metrics: Arc<MetricsCollection>,
    ) -> CarbonResult<()> {

        let arc_state = solana_state::get_solana_state();

        arc_state.set_slot(metadata.slot);

        arc_state.handle_account_update(metadata.pubkey.clone(), account);

        Ok(())

    }
}
