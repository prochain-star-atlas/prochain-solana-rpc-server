use carbon_core::account::AccountDecoder;
use carbon_core::datasource::DatasourceId;
use solana_pubkey::Pubkey;
use static_init::dynamic;
use parking_lot::{Mutex};
use tokio::{signal, task};
use std::ops::Add;
use std::str::FromStr;
use std::{env, time::Duration};
use std::sync::Arc;

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
use dashmap::DashMap;
use crate::solana_state::{self, get_solana_state};

#[dynamic] 
static JOINHANDLE_REF: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::from(None));

#[dynamic] 
static COUNT_REF: Arc<Mutex<u32>> = Arc::new(Mutex::from(0));

#[dynamic] 
static LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION: Mutex<DashMap<String, Vec<String>>> = Mutex::new(DashMap::new());

pub fn set_mutex_token_owner_sub(sub_name: String, lst_vec: Vec<String>) {
    LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION.lock().insert(sub_name, lst_vec);
}

pub fn get_mutex_token_owner_sub(sub_name: String) -> Vec<String> {
    let map = LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION.lock();
    let val0 = map.get(&sub_name);
    match val0 {
        None => { return vec![]; },
        Some(val) => { val.value().clone() }
    }
}

pub fn reset_all_list_sub() {
    LIST_TOKEN_OWNER_ACCOUNT_SUBSCRIPTION.lock().clear();
}

#[derive(Debug, Clone, Default)]
pub struct SubscriptionTokenOwnerAccountService {
}

impl SubscriptionTokenOwnerAccountService {

    pub async fn restart() {

        let jh = SubscriptionTokenOwnerAccountService::start_monitor().await;

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

    }

    pub async fn start_monitor() -> Result<CancellationToken, anyhow::Error> {
        
        let list_add: Vec<String> = get_mutex_token_owner_sub("sage".to_string()).into_iter().map(|f| { f }).collect();
        let mut hs_pk: HashSet<Pubkey> = HashSet::new();

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
            "stoas_".to_string() + data_count.to_string().as_str(),
            SubscribeRequestFilterAccounts {
                nonempty_txn_signature: None,
                account: vec![],
                owner: list_add.clone(),
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
            Arc::new(RwLock::new(hs_pk))
        );

        let datasource_cancellation_token = CancellationToken::new(); 

        let datasource_id = DatasourceId::new_unique();

        // 6 - Build and run the Carbon pipeline
        let mut pipeline = carbon_core::pipeline::Pipeline::builder()
            .datasource_with_id(yellowstone_grpc, datasource_id.clone())
            .datasource_cancellation_token(datasource_cancellation_token.clone())
            .account(GenericAccountDecoder, GenericAccountProcessor)
            .account_deletions(GenericAccountDeletionProcessor)
            .shutdown_strategy(carbon_core::pipeline::ShutdownStrategy::Immediate)
            .build()?;

        let _thread = tokio::spawn(async move {
            if let Err(e) = pipeline.run().await {
                log::error!("Pipeline run error: {:?}", e);
            }
        });

        log::info!("Start subscription for Pumpfun Amm ...");

        return Ok(datasource_cancellation_token.clone());

    }

}

pub struct GenericAccount {
    data: Vec<u8>
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
                data: GenericAccount { data: account.data.clone() },
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

        let do_steps = async || -> Result<(), anyhow::Error> {

            let arc_state = solana_state::get_solana_state();
            if account.lamports == 0 {
                arc_state.clean_zero_account(metadata.pubkey.clone());
            } else {
                arc_state.handle_account_update(metadata.pubkey.clone(), account);
            }

            Ok(())

        };
        
        if let Err(err) = do_steps().await {
            log::error!("error in process: {}", err);
        }

        Ok(())
    }
}

pub struct GenericAccountDeletionProcessor;
#[async_trait]
impl Processor for GenericAccountDeletionProcessor {
    type InputType = carbon_core::datasource::AccountDeletion;

    async fn process(
        &mut self,
        account: Self::InputType,
        _metrics: Arc<MetricsCollection>,
    ) -> CarbonResult<()> {

        let do_steps = async || -> Result<(), anyhow::Error> {

            let arc_state = solana_state::get_solana_state();
            arc_state.clean_zero_account(account.pubkey);

            Ok(())

        };
        
        if let Err(err) = do_steps().await {
            log::error!("error in process: {}", err);
        }

        Ok(())
    }
}
