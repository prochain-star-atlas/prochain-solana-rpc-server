use {
    crate::rpc::request_processor::JsonRpcRequestProcessor,
    jsonrpc_core::{futures::future, BoxFuture, Error, Result},
    jsonrpc_derive::rpc,
    log::*,
    serde::{Deserialize, Serialize},
    solana_account_decoder::UiAccount,
    solana_client::{
        rpc_config::*,
        rpc_filter::RpcFilterType,
        rpc_request::MAX_MULTIPLE_ACCOUNTS,
        rpc_response::{Response as RpcResponse, *},
    },
    solana_sdk::pubkey::Pubkey,
    solana_sdk::hash::Hash
};

pub const MAX_REQUEST_PAYLOAD_SIZE: usize = 50 * (1 << 10); // 50kB

/// Wrapper for rpc return types of methods that provide responses both with and without context.
/// Main purpose of this is to fix methods that lack context information in their return type,
/// without breaking backwards compatibility.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionalContext<T> {
    Context(RpcResponse<T>),
    NoContext(T),
}

fn verify_filter(input: &RpcFilterType) -> Result<()> {
    input
        .verify()
        .map_err(|e| Error::invalid_params(format!("Invalid param: {:?}", e)))
}

fn verify_pubkey(input: &str) -> Result<Pubkey> {
    input
        .parse()
        .map_err(|e| Error::invalid_params(format!("Invalid param: {:?}", e)))
}

pub mod rpc_accounts {
    use std::str::FromStr;

    use solana_account_decoder::parse_token::UiTokenAmount;
    use solana_client::rpc_client::SerializableTransaction;
    use solana_runtime::commitment;
    use solana_sdk::{commitment_config::CommitmentConfig, epoch_info::EpochInfo, signature::Signature, transaction::VersionedTransaction};
    use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, TransactionStatus};
    

    use super::*;

    #[rpc]
    pub trait AccountsData {
        type Metadata;

        #[rpc(meta, name = "getAccountInfo")]
        fn get_account_info(
            &self,
            meta: Self::Metadata,
            pubkey_str: String,
            config: Option<RpcAccountInfoConfig>,
        ) -> BoxFuture<Result<RpcResponse<Option<UiAccount>>>>;

        #[rpc(meta, name = "getMultipleAccounts")]
        fn get_multiple_accounts(
            &self,
            meta: Self::Metadata,
            pubkey_strs: Vec<String>,
            config: Option<RpcAccountInfoConfig>,
        ) -> BoxFuture<Result<RpcResponse<Vec<Option<UiAccount>>>>>;

        #[rpc(meta, name = "getProgramAccounts")]
        fn get_program_accounts_full(
            &self,
            meta: Self::Metadata,
            program_id_str: String,
            config: Option<RpcProgramAccountsConfig>
        ) -> BoxFuture<Result<OptionalContext<Vec<RpcKeyedAccount>>>>;

        #[rpc(meta, name = "getTokenSupply")]
        fn get_token_supply(
            &self,
            meta: Self::Metadata,
            token_id_str: String,
            config: Option<RpcTransactionLogsConfig>
        ) -> BoxFuture<Result<solana_client::rpc_response::Response<UiTokenAmount>>>;

        #[rpc(meta, name = "getSlot")]
        fn get_slot(
            &self,
            meta: Self::Metadata,
            config: Option<RpcTransactionLogsConfig>
        ) -> BoxFuture<Result<u64>>;

        #[rpc(meta, name = "getTokenAccountsByOwner")]
        fn get_token_accounts_by_owner(
            &self,
            meta: Self::Metadata,
            program_id_str: String,
            token_account_filter: RpcTokenAccountsFilter,
            config: Option<RpcAccountInfoConfig>
        ) -> BoxFuture<Result<solana_client::rpc_response::Response<Vec<RpcKeyedAccount>>>>;

        #[rpc(meta, name = "getNonCachedTokenAccountsByOwner")]
        fn get_non_cached_token_accounts_by_owner(
            &self,
            meta: Self::Metadata,
            program_id_str: String,
            token_account_filter: RpcTokenAccountsFilter,
            config: Option<RpcAccountInfoConfig>
        ) -> BoxFuture<Result<solana_client::rpc_response::Response<Vec<RpcKeyedAccount>>>>;

        #[rpc(meta, name = "getTokenAccountBalance")]
        fn get_token_accounts_balance(
            &self,
            meta: Self::Metadata,
            program_id_str: String,
            config: Option<RpcAccountInfoConfig>
        ) -> BoxFuture<Result<solana_client::rpc_response::Response<UiTokenAmount>>>;

        #[rpc(meta, name = "getLatestBlockhash")]
        fn get_latest_blockhash(
            &self,
            meta:Self::Metadata) -> BoxFuture<Result<solana_client::rpc_response::Response<RpcBlockhash>>>;

        #[rpc(meta, name = "getLatestBlockhash")]
        fn get_latest_blockhash_with_commitment(
            &self,
            meta:Self::Metadata,
            commitment: CommitmentConfig) -> BoxFuture<Result<solana_client::rpc_response::Response<RpcBlockhash>>>;

        #[rpc(meta, name = "sendTransaction")]
        fn send_transaction(
            &self,
            meta:Self::Metadata,
            data: String,
            config: Option<RpcSendTransactionConfig>,
        ) -> BoxFuture<Result<String>>;

        #[rpc(meta, name = "getEpochInfo")]
        fn get_epoch_info_with_commitment(
            &self,
            meta:Self::Metadata,
            config: Option<RpcEpochConfig>) -> BoxFuture<Result<EpochInfo>>;

        #[rpc(meta, name = "getSignatureStatuses")]
        fn get_signature_statuses(
            &self,
            meta:Self::Metadata,
            signatures: Vec<String>,
        ) -> BoxFuture<Result<solana_client::rpc_response::Response<Vec<Option<TransactionStatus>>>>>;

        #[rpc(meta, name = "getTransaction")]
        fn get_transaction(
            &self,
            meta: Self::Metadata,
            signature_str: String,
            config: Option<RpcEncodingConfigWrapper<RpcTransactionConfig>>,
        ) -> BoxFuture<Result<Option<EncodedConfirmedTransactionWithStatusMeta>>>;

    }

    pub struct AccountsDataImpl;

    impl AccountsData for AccountsDataImpl {
        type Metadata = JsonRpcRequestProcessor;

        fn get_account_info(
            &self,
            meta: Self::Metadata,
            pubkey_str: String,
            config: Option<RpcAccountInfoConfig>,
        ) -> BoxFuture<Result<RpcResponse<Option<UiAccount>>>> {
            
            let pubkey = verify_pubkey(&pubkey_str);
            match pubkey {
                Err(err) => Box::pin(future::err(err)),
                Ok(pubkey) => Box::pin(async move { meta.get_account_info(&pubkey, config, None).await }),
            }
            
        }

        fn get_multiple_accounts(
            &self,
            meta: Self::Metadata,
            pubkey_strs: Vec<String>,
            config: Option<RpcAccountInfoConfig>,
        ) -> BoxFuture<Result<RpcResponse<Vec<Option<UiAccount>>>>> {
            info!(
                "get_multiple_accounts rpc request received: {:?}",
                pubkey_strs.len()
            );

            let max_multiple_accounts = meta
                .config
                .max_multiple_accounts
                .unwrap_or(MAX_MULTIPLE_ACCOUNTS);
            if pubkey_strs.len() > max_multiple_accounts {
                return Box::pin(future::err(Error::invalid_params(format!(
                    "Too many inputs provided; max {}",
                    max_multiple_accounts
                ))));
            }
            let mut pubkeys = Vec::new();

            for pubkey in pubkey_strs {
                match verify_pubkey(&pubkey) {
                    Err(err) => {
                        return Box::pin(future::err(err));
                    }
                    Ok(pubkey) => {
                        pubkeys.push(pubkey);
                    }
                }
            }
            Box::pin(async move { meta.get_multiple_accounts(pubkeys, config, None).await })
        }

        fn get_program_accounts_full(
            &self,
            meta: Self::Metadata,
            program_id_str: String,
            config: Option<RpcProgramAccountsConfig>
        ) -> BoxFuture<Result<OptionalContext<Vec<RpcKeyedAccount>>>> {
            info!(
                "get_program_accounts program_id_str: {:?}",
                program_id_str
            );

            let result = verify_pubkey(&program_id_str);
            if let Err(err) = result {
                return Box::pin(future::err(err));
            }
            let program_id = result.unwrap();

            Box::pin(async move {
                meta.get_program_accounts(&program_id, config, None)
                    .await
            })

            /*
            let result = verify_pubkey(&program_id_str);
            if let Err(err) = result {
                return Box::pin(future::err(err));
            }
            let program_id = result.unwrap();
            let (config, filters, with_context) = if let Some(config) = config {
                (
                    Some(config.account_config),
                    config.filters.unwrap_or_default(),
                    config.with_context.unwrap_or_default(),
                )
            } else {
                (None, vec![], false)
            };
            if filters.len() > MAX_GET_PROGRAM_ACCOUNT_FILTERS {
                let err = Error::invalid_params(format!(
                    "Too many filters provided; max {}",
                    MAX_GET_PROGRAM_ACCOUNT_FILTERS
                ));
                return Box::pin(future::err(err));
            }
            for filter in &filters {
                let result = verify_filter(filter);
                if let Err(err) = result {
                    return Box::pin(future::err(err));
                }
            }
            Box::pin(async move {
                meta.get_program_accounts(&program_id, config, filters, with_context)
                    .await
            })
            */
        }
        
        fn get_token_supply(&self,
            meta:Self::Metadata,
            token_id_str:String,
            config:Option<RpcTransactionLogsConfig>) -> BoxFuture<Result<solana_client::rpc_response::Response<UiTokenAmount>>>  {

                Box::pin(async move { meta.get_token_supply(token_id_str, config, None).await })
        
        }
        
        fn get_slot(&self, meta:Self::Metadata, config: Option<RpcTransactionLogsConfig>) -> BoxFuture<Result<u64> >  {
            Box::pin(async move { meta.get_slot(config).await })
        }
        
        fn get_token_accounts_by_owner(&self,
            meta:Self::Metadata,
            program_id_str:String,
            token_account_filter:RpcTokenAccountsFilter,
            config:Option<RpcAccountInfoConfig>) -> BoxFuture<Result<Response<Vec<RpcKeyedAccount>>>> {
            
            Box::pin(async move { meta.get_token_account_by_owner(program_id_str, token_account_filter, config, None).await })

        }

        fn get_non_cached_token_accounts_by_owner(&self,
            meta:Self::Metadata,
            program_id_str:String,
            token_account_filter:RpcTokenAccountsFilter,
            config:Option<RpcAccountInfoConfig>) -> BoxFuture<Result<Response<Vec<RpcKeyedAccount>>>> {
            
            Box::pin(async move { meta.get_non_cached_token_account_by_owner(program_id_str, token_account_filter, config).await })

        }
        
        fn get_token_accounts_balance(
                &self,
                meta:Self::Metadata,
                program_id_str:String,
                config:Option<RpcAccountInfoConfig>) -> BoxFuture<Result<Response<UiTokenAmount>>> {
            
                    Box::pin(async move { meta.get_token_accounts_balance(program_id_str, config, None).await })

        }
        
        fn get_latest_blockhash(
            &self,
            meta:Self::Metadata) -> BoxFuture<Result<Response<RpcBlockhash>>> {
                Box::pin(async move { meta.get_latest_blockhash().await })
        }

        fn get_latest_blockhash_with_commitment(
            &self,
            meta:Self::Metadata,
            commitment: CommitmentConfig) -> BoxFuture<Result<Response<RpcBlockhash>>> {
                Box::pin(async move { meta.get_latest_blockhash_with_commitment(commitment).await })
        }

        fn send_transaction(
            &self,
            meta:Self::Metadata,
            transaction: String,
            config: Option<RpcSendTransactionConfig>) -> BoxFuture<Result<String>> {
                Box::pin(async move { meta.send_transaction(transaction, config).await })
        }

        fn get_epoch_info_with_commitment(
            &self,
            meta:Self::Metadata,
            config: Option<RpcEpochConfig>) -> BoxFuture<Result<EpochInfo>> {
                Box::pin(async move { meta.get_epoch_info_with_commitment(config).await })
        }

        fn get_signature_statuses(
            &self,
            meta:Self::Metadata,
            signatures: Vec<String>,
        ) -> BoxFuture<Result<Response<Vec<Option<TransactionStatus>>>>> {
            Box::pin(async move { meta.get_signature_statuses(signatures) })
        }

        fn get_transaction(
            &self,
            meta: Self::Metadata,
            signature_str: String,
            config: Option<RpcEncodingConfigWrapper<RpcTransactionConfig>>,
        ) -> BoxFuture<Result<Option<EncodedConfirmedTransactionWithStatusMeta>>> {
            
            let signature = Signature::from_str(&signature_str.as_str());

            Box::pin(async move { meta.get_transaction(signature.unwrap(), config).await })
        }

    }
}
