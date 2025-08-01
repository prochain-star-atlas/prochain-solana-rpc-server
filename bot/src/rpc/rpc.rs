use crate::rpc::rpc_client_service::RpcClientService;

use {
    solana_account_decoder::UiAccount,
    solana_client::{
        rpc_config::*,
        rpc_response::{Response as RpcResponse, *},
    },
    solana_sdk::pubkey::Pubkey,
};

use std::str::FromStr;

use solana_account_decoder::parse_token::UiTokenAmount;
use solana_sdk::{commitment_config::CommitmentConfig, epoch_info::EpochInfo};
use solana_signature::Signature;
use solana_transaction_status::{EncodedTransactionWithStatusMeta, TransactionStatus};
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;

pub const MAX_REQUEST_PAYLOAD_SIZE: usize = 50 * (1 << 10); // 50kB

#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize, Clone)]
pub struct EncodedConfirmedTransactionWithStatusMetaClonable {
    pub slot: u64,
    pub transaction: EncodedTransactionWithStatusMeta,
    pub block_time: Option<i64>,
}

#[rpc(server)]
pub trait Rpc {

    #[method(name = "getAccountInfo")]
    async fn get_account_info(
        &self,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Option<UiAccount>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getMultipleAccounts")]
    async fn get_multiple_accounts(
        &self,
        pubkey_strs: Vec<String>,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Vec<Option<UiAccount>>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getProgramAccounts")]
    async fn get_program_accounts_full(
        &self,
        program_id_str: String,
        config: Option<RpcProgramAccountsConfig>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getNonCachedProgramAccounts")]
    async fn get_non_cached_program_accounts_full(
        &self,
        program_id_str: String,
        config: Option<RpcProgramAccountsConfig>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getTokenSupply")]
    async fn get_token_supply(
        &self,
        token_id_str: String,
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<RpcResponse<UiTokenAmount>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getSlot")]
    async fn get_slot(
        &self,
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<u64, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getTokenAccountsByOwner")]
    async fn get_token_accounts_by_owner(
        &self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<RpcResponse<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getNonCachedTokenAccountsByOwner")]
    async fn get_non_cached_token_accounts_by_owner(
        &self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<RpcResponse<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getTokenAccountBalance")]
    async fn get_token_accounts_balance(
        &self,
        program_id_str: String,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<RpcResponse<UiTokenAmount>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getLatestBlockhash")]
    async fn get_latest_blockhash(&self) -> Result<RpcResponse<RpcBlockhash>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getLatestBlockhashWithCommitment")]
    async fn get_latest_blockhash_with_commitment(&self, commitment: CommitmentConfig) -> Result<RpcResponse<RpcBlockhash>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "sendTransaction")]
    async fn send_transaction(
        &self,
        data: String,
        config: Option<RpcSendTransactionConfig>,
    ) -> Result<String, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getEpochInfo")]
    async fn get_epoch_info_with_commitment(
        &self,
        config: Option<RpcEpochConfig>) -> Result<EpochInfo, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getSignatureStatuses")]
    async fn get_signature_statuses(
        &self,
        signatures: Vec<String>,
    ) -> Result<RpcResponse<Vec<Option<TransactionStatus>>>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getTransaction")]
    async fn get_transaction(
        &self,
        signature_str: String,
        config: Option<RpcEncodingConfigWrapper<RpcTransactionConfig>>,
    ) -> Result<Option<EncodedConfirmedTransactionWithStatusMetaClonable>, jsonrpsee::types::error::ErrorObjectOwned>;

    #[method(name = "getBalance")]
    async fn get_balance(
        &self,
        program_id_str: String) -> Result<solana_client::rpc_response::Response<u64>, jsonrpsee::types::error::ErrorObjectOwned>;

}

pub struct RpcServerImpl;

#[async_trait]
impl RpcServer for RpcServerImpl {

    async fn get_account_info(
        &self,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Option<UiAccount>>, jsonrpsee::types::error::ErrorObjectOwned> {
        let pk = Pubkey::from_str(pubkey_str.as_str()).unwrap();
        let acc = RpcClientService::new().get_account_info(&pk, config, None).await;
        Ok(acc.unwrap())
    }

    async fn get_multiple_accounts(
        &self,
        pubkey_strs: Vec<String>,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<RpcResponse<Vec<Option<UiAccount>>>, jsonrpsee::types::error::ErrorObjectOwned> {
        let pks: Vec<Pubkey> = pubkey_strs.into_iter().map(|f| { Pubkey::from_str(f.as_str()).unwrap() }).collect();
        let acc = RpcClientService::new().get_multiple_accounts(pks, config, None).await;
        Ok(acc.unwrap())
    }

    async fn get_program_accounts_full(
        &self,
        program_id_str: String,
        config: Option<RpcProgramAccountsConfig>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let pk = Pubkey::from_str(program_id_str.as_str()).unwrap();
        let acc = RpcClientService::new().get_program_accounts(&pk, config, None).await;
        Ok(acc.unwrap())
    }
    
    async fn get_non_cached_program_accounts_full(
        &self,
        program_id_str: String,
        config: Option<RpcProgramAccountsConfig>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let pk = Pubkey::from_str(program_id_str.as_str()).unwrap();
        let acc = RpcClientService::new().get_non_cached_program_accounts(&pk, config, None).await;
        Ok(acc.unwrap())
    }

    async fn get_token_supply(
        &self,
        token_id_str: String,
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<solana_client::rpc_response::Response<UiTokenAmount>, jsonrpsee::types::error::ErrorObjectOwned>   {
        let acc = RpcClientService::new().get_token_supply(token_id_str, config, None).await;
        Ok(acc.unwrap())
    }
    
    async fn get_slot(
        &self, 
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<u64, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_slot(config).await;
        Ok(acc.unwrap())
    }
    
    async fn get_token_accounts_by_owner(&self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_token_account_by_owner(program_id_str, token_account_filter, config, None).await;
        Ok(acc.unwrap())
    }

    async fn get_non_cached_token_accounts_by_owner(&self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<Vec<RpcKeyedAccount>>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_non_cached_token_account_by_owner(program_id_str, token_account_filter, config).await;
        Ok(acc.unwrap())
    }
    
    async fn get_token_accounts_balance(
        &self,
        program_id_str: String,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<UiTokenAmount>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_token_accounts_balance(program_id_str, config, None).await;
        Ok(acc.unwrap())
    }
    
    async fn get_latest_blockhash(
        &self
    ) -> Result<Response<RpcBlockhash>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_latest_blockhash().await;
        Ok(acc.unwrap())
    }

    async fn get_latest_blockhash_with_commitment(
        &self,
        commitment: CommitmentConfig
    ) -> Result<Response<RpcBlockhash>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_latest_blockhash_with_commitment(commitment).await;
        Ok(acc.unwrap())
    }

    async fn send_transaction(
        &self,
        transaction: String,
        config: Option<RpcSendTransactionConfig>
    ) -> Result<String, jsonrpsee::types::error::ErrorObjectOwned> {
        let acc = RpcClientService::new().send_transaction(transaction, config).await;
        Ok(acc.unwrap())
    }

    async fn get_epoch_info_with_commitment(
        &self,
        config: Option<RpcEpochConfig>
    ) -> Result<EpochInfo, jsonrpsee::types::error::ErrorObjectOwned> {
        let acc = RpcClientService::new().get_epoch_info_with_commitment(config).await;
        Ok(acc.unwrap())
    }

    async fn get_signature_statuses(
        &self,
        signatures: Vec<String>
    ) -> Result<Response<Vec<Option<TransactionStatus>>>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_signature_statuses(signatures).await;
        Ok(acc.unwrap())
    }

    async fn get_transaction(
        &self,
        signature_str: String,
        config: Option<RpcEncodingConfigWrapper<RpcTransactionConfig>>
    ) -> Result<Option<EncodedConfirmedTransactionWithStatusMetaClonable>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let signature = Signature::from_str(&signature_str.as_str());
        let acc = RpcClientService::new().get_transaction(signature.unwrap(), config).await;
        Ok(acc.unwrap())
    }

    async fn get_balance(
        &self,
        addr: String
    ) -> Result<Response<u64>, jsonrpsee::types::error::ErrorObjectOwned>  {
        let acc = RpcClientService::new().get_balance(&Pubkey::from_str(addr.as_str()).unwrap()).await;
        Ok(acc.unwrap())
    }

}
