use std::{cmp::min, collections::HashMap, str::FromStr, time::Duration};

/// The JSON request processor
/// This takes the request from the client and load the information from the datastore.
use {
    crate::{rpc::{
        rpc::OptionalContext,
        rpc_service::JsonRpcConfig,
    }, solana_state::{ProchainAccountInfo, SolanaStateManager}}, jsonrpc_core::{Metadata, Result}, log::*, serde_json::json, solana_account_decoder::{UiAccount, UiAccountEncoding}, solana_client::{
        rpc_config::{RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcTokenAccountsFilter, RpcTransactionLogsConfig}, rpc_custom_error::RpcCustomError, rpc_filter::RpcFilterType, rpc_request::RpcRequest, rpc_response::{Response as RpcResponse, *}
    }, solana_sdk::{clock::Slot, pubkey::Pubkey
    }, std::sync::Arc
};
use bincode::Options;
use solana_client::{rpc_config::{RpcEncodingConfigWrapper, RpcEpochConfig, RpcSendTransactionConfig, RpcTransactionConfig}, rpc_filter::Memcmp, rpc_request::TokenAccountsFilter};
use jsonrpc_core::{types::error, types::Error};
use solana_inline_spl::{
    token::{SPL_TOKEN_ACCOUNT_MINT_OFFSET, SPL_TOKEN_ACCOUNT_OWNER_OFFSET},
    token_2022::{self, ACCOUNTTYPE_ACCOUNT},
};
use base64::{prelude::BASE64_STANDARD};
use solana_account_decoder::{
    parse_account_data::{AccountAdditionalDataV2, SplTokenAdditionalData}, parse_token::{
        get_token_account_mint, is_known_spl_token_id, token_amount_to_ui_amount_v2, UiTokenAmount
    }, UiAccountData, UiDataSliceConfig, MAX_BASE58_BYTES
};
use solana_transaction_status::{EncodedConfirmedTransactionWithStatusMeta, TransactionBinaryEncoding, TransactionStatus, UiTransactionEncoding};
use spl_token_2022::{
    extension::StateWithExtensions,
    solana_program::program_pack::Pack,
    state::{Account as TokenAccount, Mint},
};
use solana_sdk::{commitment_config::{CommitmentConfig, CommitmentLevel}, epoch_info::EpochInfo, stake::config, transaction::VersionedTransaction};
use solana_sdk::pubkey::PUBKEY_BYTES;
use itertools::Itertools;
use solana_sdk::account::ReadableAccount;
type RpcCustomResult<T> = std::result::Result<T, RpcCustomError>;

use solana_inline_spl::{token::GenericTokenAccount, token_2022::Account};
use solana_sdk::signature::Signature;
use base64::Engine;
use std::any::type_name;
use solana_sdk::hash::Hash;

use solana_perf::packet::PACKET_DATA_SIZE;

const SPL_TOKEN_ACCOUNT_LENGTH: usize = 165;

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    pub config: JsonRpcConfig,
    pub sol_client: Arc<solana_client::rpc_client::RpcClient>,
    pub sol_state: Arc<SolanaStateManager>
}

impl Metadata for JsonRpcRequestProcessor {}

fn new_response<T>(slot: i64, value: T) -> RpcResponse<T> {
    let context = RpcResponseContext { slot: slot as Slot, api_version: None };
    Response { context, value }
}

/// Encode the account loaded to the UiAccount
fn encode_account<T: ReadableAccount>(
    account: &T,
    pubkey: &Pubkey,
    encoding: UiAccountEncoding,
    data_slice: Option<UiDataSliceConfig>,
) -> Result<UiAccount> {
    if (encoding == UiAccountEncoding::Binary || encoding == UiAccountEncoding::Base58)
        && data_slice
            .map(|s| min(s.length, account.data().len().saturating_sub(s.offset)))
            .unwrap_or(account.data().len())
            > MAX_BASE58_BYTES
    {
        let message = format!("Encoded binary (base 58) data should be less than {MAX_BASE58_BYTES} bytes, please use Base64 encoding.");
        Err(error::Error {
            code: error::ErrorCode::InvalidRequest,
            message,
            data: None,
        })
    } else {
        Ok(UiAccount::encode(
            pubkey, account, encoding, None, data_slice,
        ))
    }
}

fn optimize_filters(filters: &mut [RpcFilterType]) {
    filters.iter_mut().for_each(|filter_type| {
        if let RpcFilterType::Memcmp(compare) = filter_type {
            if let Err(err) = compare.convert_to_raw_bytes() {
                // All filters should have been previously verified
                warn!("Invalid filter: bytes could not be decoded, {err}");
            }
        }
    })
}

pub fn filter_allows(filter: &RpcFilterType, account: &ProchainAccountInfo) -> bool {
    match filter {
        RpcFilterType::DataSize(size) => account.data().len() as u64 == *size,
        RpcFilterType::Memcmp(compare) => compare.bytes_match(account.data()),
        RpcFilterType::TokenAccountState => Account::valid_account_data(account.data()),
    }
}

fn get_spl_token_owner_filter(program_id: &Pubkey, filters: &[RpcFilterType]) -> Option<Pubkey> {
    if !is_known_spl_token_id(program_id) {
        return None;
    }
    let mut data_size_filter: Option<u64> = None;
    let mut memcmp_filter: Option<&[u8]> = None;
    let mut owner_key: Option<Pubkey> = None;
    let mut incorrect_owner_len: Option<usize> = None;
    let mut token_account_state_filter = false;
    let account_packed_len = TokenAccount::get_packed_len();
    for filter in filters {
        match filter {
            RpcFilterType::DataSize(size) => data_size_filter = Some(*size),
            RpcFilterType::Memcmp(memcmp) => {
                let offset = memcmp.offset();
                if let Some(bytes) = memcmp.raw_bytes_as_ref() {
                    if offset == account_packed_len && *program_id == token_2022::id() {
                        memcmp_filter = Some(bytes);
                    } else if offset == SPL_TOKEN_ACCOUNT_OWNER_OFFSET {
                        if bytes.len() == PUBKEY_BYTES {
                            owner_key = Pubkey::try_from(bytes).ok();
                        } else {
                            incorrect_owner_len = Some(bytes.len());
                        }
                    }
                }
            }
            RpcFilterType::TokenAccountState => token_account_state_filter = true,
        }
    }
    if data_size_filter == Some(account_packed_len as u64)
        || memcmp_filter == Some(&[ACCOUNTTYPE_ACCOUNT])
        || token_account_state_filter
    {
        if let Some(incorrect_owner_len) = incorrect_owner_len {
            info!(
                "Incorrect num bytes ({:?}) provided for spl_token_owner_filter",
                incorrect_owner_len
            );
        }
        owner_key
    } else {
        debug!("spl_token program filters do not match by-owner index requisites");
        None
    }
}

fn get_spl_token_mint_filter(program_id: &Pubkey, filters: &[RpcFilterType]) -> Option<Pubkey> {
    if !is_known_spl_token_id(program_id) {
        return None;
    }
    let mut data_size_filter: Option<u64> = None;
    let mut memcmp_filter: Option<&[u8]> = None;
    let mut mint: Option<Pubkey> = None;
    let mut incorrect_mint_len: Option<usize> = None;
    let mut token_account_state_filter = false;
    let account_packed_len = TokenAccount::get_packed_len();
    for filter in filters {
        match filter {
            RpcFilterType::DataSize(size) => data_size_filter = Some(*size),
            RpcFilterType::Memcmp(memcmp) => {
                let offset = memcmp.offset();
                if let Some(bytes) = memcmp.raw_bytes_as_ref() {
                    if offset == account_packed_len && *program_id == token_2022::id() {
                        memcmp_filter = Some(bytes);
                    } else if offset == SPL_TOKEN_ACCOUNT_MINT_OFFSET {
                        if bytes.len() == PUBKEY_BYTES {
                            mint = Pubkey::try_from(bytes).ok();
                        } else {
                            incorrect_mint_len = Some(bytes.len());
                        }
                    }
                }
            }
            RpcFilterType::TokenAccountState => token_account_state_filter = true,
        }
    }
    if data_size_filter == Some(account_packed_len as u64)
        || memcmp_filter == Some(&[ACCOUNTTYPE_ACCOUNT])
        || token_account_state_filter
    {
        if let Some(incorrect_mint_len) = incorrect_mint_len {
            info!(
                "Incorrect num bytes ({:?}) provided for spl_token_mint_filter",
                incorrect_mint_len
            );
        }
        mint
    } else {
        debug!("spl_token program filters do not match by-mint index requisites");
        None
    }
}

impl JsonRpcRequestProcessor {
    pub fn new(config: JsonRpcConfig, sol_client: Arc<solana_client::rpc_client::RpcClient>, sol_state: Arc<SolanaStateManager>) -> Self {
        Self {
            config,
            sol_client,
            sol_state
        }
    }

    fn get_token_program_id_and_mint(
        &self,
        token_account_filter: TokenAccountsFilter,
        force_refresh: Option<bool>
    ) -> Result<(Pubkey, Option<Pubkey>)> {
        match token_account_filter {
            TokenAccountsFilter::Mint(mint) => {
                let (mint_owner, _) = self.get_mint_owner_and_additional_data(&mint, force_refresh)?;
                if !is_known_spl_token_id(&mint_owner) {
                    return Err(Error::invalid_params(
                        "Invalid param: not a Token mint".to_string(),
                    ));
                }
                Ok((mint_owner, Some(mint)))
            }
            TokenAccountsFilter::ProgramId(program_id) => {
                if is_known_spl_token_id(&program_id) {
                    Ok((program_id, None))
                } else {
                    Err(Error::invalid_params(
                        "Invalid param: unrecognized Token program id".to_string(),
                    ))
                }
            }
        }
    }

    fn get_prochain_account(&self, pubkey: &Pubkey, config: RpcAccountInfoConfig, force_refresh: Option<bool>) -> Option<ProchainAccountInfo> {

        let force_refresh_var = force_refresh.unwrap_or(false);

        let cached_acc = self.sol_state.get_account_info(pubkey.clone());

        if cached_acc.is_some() && !force_refresh_var {

            info!("[MEMORY] get_account_info request received: {}", pubkey.to_string());

            let c_acc = cached_acc.unwrap();

            if c_acc.is_err() {

                return None;

            }

            let c_acc = c_acc.unwrap();
            return Some(c_acc);

        } else {

            info!("[RPC] get_account_info request received: {}", pubkey.to_string());

            let res = self.sol_client.get_account_with_config(pubkey, config);

            match res {

                Ok(val) => {

                    match val.value {

                        Some(acc) => {

                            let acc_pk = acc;
            
                            let pa = ProchainAccountInfo {
                                data: acc_pk.data,
                                executable: acc_pk.executable,
                                lamports: acc_pk.lamports,
                                owner: Pubkey::try_from(acc_pk.owner).unwrap(),
                                pubkey: pubkey.clone(),
                                rent_epoch: acc_pk.rent_epoch,
                                slot: 0,
                                txn_signature: None,
                                write_version: 0,
                                last_update: chrono::offset::Utc::now()
                            };
            
                            self.sol_state.add_account_info(pubkey.clone(), pa.clone());
                            let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
                            vec_acc.push(pubkey.to_string());
                            crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
                            crate::oracles::create_subscription_oracle::refresh();
                
                            return Some(pa.clone());

                        },
                        None => {

                            return None;

                        }
                        
                    }

                },

                Err(err) => {

                    error!("[RPC] get_account_info request received: {}, {}", pubkey.to_string(), err.to_string());

                    return None;
                }
            }
                        
        }

    }

    /// Get account infor for a single account with the pubkey.
    pub async fn get_account_info(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
        force_refresh: Option<bool>
    ) -> Result<RpcResponse<Option<UiAccount>>> {

        let config = config.unwrap_or_default();

        let pro_account = self.get_prochain_account(pubkey, config.clone(), force_refresh);

        let slot: u64 = self.sol_state.get_slot();

        if pro_account.is_some() {

            let ui_account = UiAccount::encode(pubkey, &pro_account.unwrap().clone(), config.clone().encoding.unwrap(), None, config.clone().data_slice);
            
            return Ok(RpcResponse {
                context: RpcResponseContext {
                    api_version: None,
                    slot: slot.clone()
                },
                value: Some(ui_account)
            });
        }

        return Ok(RpcResponse {
            context: RpcResponseContext {
                api_version: None,
                slot: slot.clone()
            },
            value: None
        });

    }

    /// Load multiple accounts
    pub async fn get_multiple_accounts(
        &self,
        pubkeys: Vec<Pubkey>,
        config: Option<RpcAccountInfoConfig>,
        force_refresh: Option<bool>
    ) -> Result<RpcResponse<Vec<Option<UiAccount>>>> {

        info!("getting get_multiple_accounts is called for {:?}", pubkeys);
        let config = config.unwrap_or_default();

        let force_refresh_var = force_refresh.unwrap_or(false);

        let mut accounts = Vec::new();
        let slot = self.sol_state.get_slot();

        for pubkey in pubkeys {

            let ui_account;
            let cached_acc = self.sol_state.get_account_info(pubkey.clone());

            if cached_acc.is_some() && !force_refresh_var {

                info!("[MEMORY] get_account_info request received: {}", pubkey.to_string());

                let c_acc = cached_acc.unwrap();

                if c_acc.is_ok() {
                    let c_acc_c = c_acc.unwrap();
                    ui_account = encode_account(&c_acc_c, &c_acc_c.pubkey, config.clone().encoding.unwrap(), config.clone().data_slice).unwrap();
                    accounts.push(Some(ui_account));
                }

            } else {

                info!("[RPC] get_account_info request received: {}", pubkey.to_string());

                let res = self.sol_client.get_account_with_config(&pubkey, config.clone()).unwrap();
                let acc_pk = res.value.clone().unwrap_or_default();
                self.sol_state.add_account_info(pubkey.clone(), ProchainAccountInfo {
                    data: acc_pk.data,
                    executable: acc_pk.executable,
                    lamports: acc_pk.lamports,
                    owner: Pubkey::try_from(acc_pk.owner).unwrap(),
                    pubkey: pubkey.clone(),
                    rent_epoch: acc_pk.rent_epoch,
                    slot: 0,
                    txn_signature: None,
                    write_version: 0,
                    last_update: chrono::offset::Utc::now()
                });
                let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
                vec_acc.push(pubkey.to_string());
                crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
                crate::oracles::create_subscription_oracle::refresh();
                let acc_nw = res.value.clone().unwrap_or_default();
                ui_account = UiAccount::encode(&pubkey, &acc_nw, config.clone().encoding.unwrap(), None, config.clone().data_slice);
                accounts.push(Some(ui_account));

            }
        }

        Ok(RpcResponse {
            context: RpcResponseContext { slot: slot, api_version: None },
            value: accounts,
        })
    }

    fn get_additional_mint_data(&self, data: &[u8]) -> Result<SplTokenAdditionalData> {
        StateWithExtensions::<Mint>::unpack(data)
            .map_err(|_| {
                Error::invalid_params("Invalid param: Token mint could not be unpacked".to_string())
            })
            .map(|mint| {
                SplTokenAdditionalData {
                    decimals: mint.base.decimals,
                    interest_bearing_config: None,
                }
            })
    }

    pub fn get_mint_owner_and_additional_data(
        &self,
        mint: &Pubkey,
        force_refresh: Option<bool>
    ) -> Result<(Pubkey, SplTokenAdditionalData)> {
        if mint.to_string() == spl_token::native_mint::id().to_string() {
            Ok((
                Pubkey::try_from(spl_token::id().to_string().as_str()).unwrap(),
                SplTokenAdditionalData::with_decimals(spl_token::native_mint::DECIMALS),
            ))
        } else {
            let config = RpcAccountInfoConfig { commitment: Some(CommitmentConfig::confirmed()), encoding: None, data_slice: None, min_context_slot: None };
            let mint_account = self.get_prochain_account(&mint.clone(), config, force_refresh);

            if mint_account.is_some() {
                let mintc = mint_account.unwrap();
                let mint_data = self.get_additional_mint_data(mintc.data()).unwrap();
                return Ok((*mintc.owner(), mint_data));
            }

            return Ok((Pubkey::try_from(spl_token::id().to_string().as_str()).unwrap(), SplTokenAdditionalData::default()));
        }
    }

    pub fn get_parsed_token_accounts(
        &self,
        keyed_accounts: Vec<ProchainAccountInfo>,
        force_refresh: Option<bool>
    ) -> Vec<RpcKeyedAccount>
    {
        let mut mint_data: HashMap<Pubkey, AccountAdditionalDataV2> = HashMap::new();
        let r: Vec<RpcKeyedAccount> = keyed_accounts.iter().filter_map(move |ka| {
            let additional_data = get_token_account_mint(&ka.data()).and_then(|mint_pubkey| {
                mint_data.get(&mint_pubkey).cloned().or_else(|| {
                    let (_, data) = self.get_mint_owner_and_additional_data(&mint_pubkey, force_refresh).ok()?;
                    let data = AccountAdditionalDataV2 {
                        spl_token_additional_data: Some(data),
                    };
                    mint_data.insert(mint_pubkey, data);
                    Some(data)
                })
            });
    
            let maybe_encoded_account = UiAccount::encode(
                &ka.pubkey,
                ka,
                UiAccountEncoding::JsonParsed,
                additional_data,
                None,
            );
            if let UiAccountData::Json(_) = &maybe_encoded_account.data {
                Some(RpcKeyedAccount {
                    pubkey: ka.pubkey.to_string(),
                    account: maybe_encoded_account,
                })
            } else {
                None
            }
        }).collect();

        return r;
    }

    fn get_filtered_spl_token_accounts_by_owner(
        &self,
        program_id: &Pubkey,
        owner_key: &Pubkey,
        mut filters: Vec<RpcFilterType>,
        sort_results: bool,
    ) -> Vec<ProchainAccountInfo> {
        // The by-owner accounts index checks for Token Account state and Owner address on
        // inclusion. However, due to the current AccountsDb implementation, an account may remain
        // in storage as a zero-lamport AccountSharedData::Default() after being wiped and reinitialized in
        // later updates. We include the redundant filters here to avoid returning these accounts.
        //
        // Filter on Token Account state
        filters.push(RpcFilterType::TokenAccountState);
        // Filter on Owner address
        filters.push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            SPL_TOKEN_ACCOUNT_OWNER_OFFSET,
            owner_key.to_bytes().into(),
        )));
        optimize_filters(&mut filters);
        let filter_closure = |account: &ProchainAccountInfo| {
            filters
                .iter()
                .all(|filter_type| filter_allows(filter_type, account))
        };

        let mut filteredacc: Vec<ProchainAccountInfo> = vec![];
        let allacc = self.sol_state.get_accounts_by_owner(program_id.clone()).unwrap();
        allacc.iter().for_each(|f| {
            let acc_cached = self.sol_state.get_account_info(f.clone());

            if acc_cached.is_some() {

                let acc_cached_c = acc_cached.unwrap();

                if acc_cached_c.is_ok() {
                    let acc_cached_c_r = acc_cached_c.unwrap();
                    if filter_closure(&acc_cached_c_r) {
                        filteredacc.push(acc_cached_c_r);
                    }
                }

            }

        });
        filteredacc
    }

    fn get_filtered_spl_token_accounts_by_mint(
        &self,
        program_id: &Pubkey,
        mint_key: &Pubkey,
        mut filters: Vec<RpcFilterType>,
        sort_results: bool,
    ) -> Vec<ProchainAccountInfo> {
        // The by-mint accounts index checks for Token Account state and Mint address on inclusion.
        // However, due to the current AccountsDb implementation, an account may remain in storage
        // as be zero-lamport AccountSharedData::Default() after being wiped and reinitialized in later
        // updates. We include the redundant filters here to avoid returning these accounts.
        //
        // Filter on Token Account state
        filters.push(RpcFilterType::TokenAccountState);
        // Filter on Mint address
        filters.push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
            SPL_TOKEN_ACCOUNT_MINT_OFFSET,
            mint_key.to_bytes().into(),
        )));
        optimize_filters(&mut filters);
        let filter_closure = |account: &ProchainAccountInfo| {
            filters
                .iter()
                .all(|filter_type| filter_allows(filter_type, account))
        };

        let mut filteredacc: Vec<ProchainAccountInfo> = vec![];
        let allacc = self.sol_state.get_accounts_by_owner(program_id.clone()).unwrap();
        allacc.iter().for_each(|f| {
            let acc_cached = self.sol_state.get_account_info(f.clone());

            if acc_cached.is_some() {
                let acc_cached_c = acc_cached.unwrap();

                if acc_cached_c.is_ok() {

                    let acc_cached_c_r = acc_cached_c.unwrap();

                    if filter_closure(&acc_cached_c_r) {
                        filteredacc.push(acc_cached_c_r);
                    }

                }

            }

        });
        filteredacc

    }

    pub async fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        config: Option<RpcProgramAccountsConfig>,
        force_refresh: Option<bool>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>> {

        let force_refresh_var = force_refresh.unwrap_or(false);
        let mut c = config.unwrap_or_default();
        let commitment = c.account_config.commitment.unwrap_or(CommitmentConfig::confirmed());
        c.account_config.commitment = Some(commitment);
        let ac = c.clone().account_config;
        let mut ac_filters = c.clone().filters.unwrap_or_default();
        let encoding = ac.encoding.unwrap_or(UiAccountEncoding::Binary);
        let data_slice_config = ac.data_slice;
        let sort_results = false;

        let mut ar_keyed_acc: Vec<RpcKeyedAccount> = vec![];

        let lst_ps = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));

        if lst_ps.contains(&program_id.to_string()) && !force_refresh_var {

            info!("[MEMORY] get_program_accounts request received: {}", program_id.to_string());

            optimize_filters(&mut ac_filters);

            let filter_closure = |account: &ProchainAccountInfo| {
                ac_filters
                    .iter()
                    .all(|filter_type| filter_allows(filter_type, account))
            };

            let keyed_accounts = {

                if let Some(owner) = get_spl_token_owner_filter(program_id, &ac_filters) {
                    let res1 = self.get_filtered_spl_token_accounts_by_owner(
                        program_id,
                        &owner,
                        ac_filters,
                        sort_results,
                    );
                    res1
                } else if let Some(mint) = get_spl_token_mint_filter(program_id, &ac_filters) {
                    let res2 = self.get_filtered_spl_token_accounts_by_mint(
                        program_id,
                        &mint,
                        ac_filters,
                        sort_results,
                    );
                    res2
                } else {
                    let mut filteredacc: Vec<ProchainAccountInfo> = vec![];
                    let allacc = self.sol_state.get_accounts_by_owner(program_id.clone()).unwrap();
                    //let res_toto = self.sol_client.get_program_accounts(program_id).unwrap();
                    //info!("rpc account loaded before filtering: {}", res_toto.len());    
                    //info!("account loaded before filtering: {}", allacc.len());
                    allacc.iter().for_each(|f| {

                        let acc_cached = self.sol_state.get_account_info(f.clone());

                        if acc_cached.is_some() {
                            let acc_kk = acc_cached.unwrap();

                            if acc_kk.is_ok() {

                                let acc_kk_r = acc_kk.unwrap();

                                if filter_closure(&acc_kk_r) {
    
                                    //info!("account filtered in cache found {:?}", acc_kk);
                                    filteredacc.push(acc_kk_r);
    
                                }

                            }
    
                        }
                        
                    });
                    filteredacc
                }

            };

            let accounts = if is_known_spl_token_id(program_id) && encoding == UiAccountEncoding::JsonParsed
            {
                self.get_parsed_token_accounts(keyed_accounts, force_refresh)
            } else {
                keyed_accounts
                    .into_iter()
                    .map(|account| {
                        Ok(RpcKeyedAccount {
                            pubkey: account.pubkey.to_string(),
                            account: encode_account(&account, &account.pubkey, encoding, data_slice_config).unwrap(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            };

            ar_keyed_acc = accounts;

        } else {

            info!("[RPC] get_program_accounts request received: {}", program_id.to_string());

            let res = self.sol_client.get_program_accounts_with_config(program_id, c.clone());       

            if res.is_err() {
                return Err(jsonrpc_core::error::Error::invalid_params(res.err().unwrap().to_string()));
            }
    
            let ar_results = res.unwrap();
            let mut ar_pkey: Vec<Pubkey> = vec![];

            ar_results.iter().for_each(|f| {

                let res_simple_account = self.sol_client.get_account(&f.0.clone()).unwrap();

                ar_pkey.push(f.0.clone());
                self.sol_state.add_account_info(f.0.clone(), ProchainAccountInfo {
                    data: res_simple_account.data.clone(),
                    executable: res_simple_account.executable,
                    lamports: res_simple_account.lamports,
                    owner: Pubkey::try_from(res_simple_account.owner.to_bytes().to_vec()).unwrap(),
                    pubkey: f.0.clone(),
                    rent_epoch: res_simple_account.rent_epoch,
                    slot: 0,
                    txn_signature: None,
                    write_version: 0,
                    last_update: chrono::offset::Utc::now()
                });
                let ui_account = UiAccount::encode(&f.0, &res_simple_account.clone(), c.clone().account_config.encoding.unwrap(), None, c.clone().account_config.data_slice);
                let aa = RpcKeyedAccount {
                    pubkey: f.0.to_string(),
                    account: ui_account,
                };
                ar_keyed_acc.push(aa);

            });

            //self.sol_state. (program_id.clone(), ar_pkey);
            let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));
            vec_acc.push(program_id.to_string());
            let v: Vec<_> = vec_acc.into_iter().unique().collect();
            crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), v);
            crate::oracles::create_subscription_oracle::refresh_owner();

        }

        Ok(ar_keyed_acc).map(|result| match c.clone().with_context.unwrap_or_default() {
            true => OptionalContext::Context(new_response(0, result)),
            false => OptionalContext::NoContext(result),
        })

    }

    pub async fn get_token_supply(
        &self,
        token_id_str: String,
        config: Option<RpcTransactionLogsConfig>,
        force_refresh: Option<bool>
    ) -> Result<solana_client::rpc_response::Response<UiTokenAmount>> {

        let force_refresh_var = force_refresh.unwrap_or(false);
        let pk = Pubkey::try_from(token_id_str.as_str()).unwrap();
        let cached_acc = self.sol_state.get_account_info(pk.clone());
        let opt: RpcTransactionLogsConfig = config.unwrap_or(RpcTransactionLogsConfig { commitment: Some(CommitmentConfig::confirmed()) });

        let ui_token_amount: UiTokenAmount;

        if cached_acc.is_some() && !force_refresh_var {

            info!("[MEMORY] get_token_supply request received: {}", token_id_str);

            let c_acc = cached_acc.unwrap();

            if c_acc.is_err() {
                return Err(jsonrpc_core::Error::internal_error());
            }

            let c_acc_r = c_acc.unwrap();

            let mint = StateWithExtensions::<Mint>::unpack(c_acc_r.data()).map_err(|_| {
                Error::invalid_params("Invalid param: mint could not be unpacked".to_string())
            })?;

            ui_token_amount = token_amount_to_ui_amount_v2(
                mint.base.supply,
                &SplTokenAdditionalData {
                    decimals: mint.base.decimals,
                    interest_bearing_config: None
                },
            );

        } else {

            info!("[RPC] get_token_supply request received: {}", token_id_str);

            let res = self.sol_client.get_token_supply_with_commitment(&pk, opt.commitment.unwrap_or(CommitmentConfig::confirmed())).unwrap();
           
            ui_token_amount = res.value;

            let acc1 = self.sol_client.get_account(&pk).unwrap();
            self.sol_state.add_account_info(pk.clone(), ProchainAccountInfo {
                data: acc1.data,
                executable: acc1.executable,
                lamports: acc1.lamports,
                owner: acc1.owner,
                pubkey: pk.clone(),
                rent_epoch: acc1.rent_epoch,
                slot: 0,
                txn_signature: None,
                write_version: 0,
                last_update: chrono::offset::Utc::now()
            });

            // let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
            // vec_acc.push(pk.to_string());
            // crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
            // crate::oracles::create_subscription_oracle::refresh();

            let mut vec_acc_v = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
            if !vec_acc_v.contains(&pk.clone().to_string()) {
                vec_acc_v.push(pk.clone().to_string());
                crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_acc_v);
                crate::oracles::create_subscription_oracle::refresh_token_account();
            }

        }

        Ok(RpcResponse {
            context: RpcResponseContext {
                api_version: None,
                slot: self.sol_state.get_slot()
            },
            value: ui_token_amount
        })

    }

    pub async fn get_slot(
        &self,
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<u64> {

        info!("[MEMORY] get_slot request received");
        let cached_slot = self.sol_state.get_slot();
        Ok(cached_slot)

    }

    pub async fn get_token_account_by_owner(
        &self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>,
        force_refresh: Option<bool>
    ) -> Result<Response<Vec<RpcKeyedAccount>>> {

        let force_refresh_var = force_refresh.unwrap_or(false);
        let c = config.unwrap_or_default();
        let sort_results = false;
        let encoding = c.encoding.unwrap_or(UiAccountEncoding::Binary);
        let config = RpcAccountInfoConfig {
            encoding: c.encoding,
            commitment: Some(c.commitment.unwrap_or_default()),
            data_slice: c.data_slice,
            min_context_slot: None,
        };

        let lst_ps = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));

        if lst_ps.contains(&program_id_str) && !force_refresh_var {

            let taf = match token_account_filter {
                RpcTokenAccountsFilter::Mint(m) => TokenAccountsFilter::Mint(Pubkey::try_from(m.as_str()).unwrap()),
                RpcTokenAccountsFilter::ProgramId(p) => TokenAccountsFilter::ProgramId(Pubkey::try_from(p.as_str()).unwrap())
            };

            let pb = Pubkey::try_from(program_id_str.as_str()).unwrap(); 
            let (token_program_id, mint) = self.get_token_program_id_and_mint(taf, force_refresh)?;

            info!("[MEMORY] get_token_account_by_owner request received: {}", program_id_str);

            let mut filters = vec![];

            if let Some(mint) = mint {
                // Optional filter on Mint address
                filters.push(RpcFilterType::Memcmp(Memcmp::new_raw_bytes(
                    0,
                    mint.to_bytes().into(),
                )));
            }

            let keyed_accounts = self.get_filtered_spl_token_accounts_by_owner(
                &token_program_id,
                &pb,
                filters,
                sort_results,
            );
            let accounts = if encoding == UiAccountEncoding::JsonParsed {
                self.get_parsed_token_accounts(keyed_accounts, force_refresh)
            } else {
                keyed_accounts
                    .into_iter()
                    .map(|pai| {
                        Ok(RpcKeyedAccount {
                            pubkey: pai.pubkey.to_string(),
                            account: encode_account(&pai, &pai.pubkey, encoding, c.data_slice)?,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            };
            Ok(new_response(self.sol_state.get_slot() as i64, accounts))

        } else {

            info!("[RPC] get_token_account_by_owner request received: {}", program_id_str.to_string());
    
            let res_rpc: RpcResult<Vec<RpcKeyedAccount>> = self.sol_client.send(
                RpcRequest::GetTokenAccountsByOwner,
                json!([program_id_str, token_account_filter, config]),
            );
    
            let res_unwrapped = res_rpc;

            if res_unwrapped.is_err() {
                return Err(jsonrpc_core::error::Error::internal_error());
            }
    
            let ar_results = res_unwrapped.unwrap().clone();

            let mut ar_pkey: Vec<String> = vec![];

            ar_results.value.iter().for_each(|f| {
                let pb = Pubkey::try_from(f.pubkey.as_str()).unwrap();  
                let acc_raw = self.sol_client.get_account(&pb).unwrap();       
                ar_pkey.push(pb.to_string());

                let pca = ProchainAccountInfo {
                    data: acc_raw.data.clone(),
                    executable: acc_raw.executable,
                    lamports: acc_raw.lamports,
                    owner: acc_raw.owner.clone(),
                    pubkey: pb.clone(),
                    rent_epoch: acc_raw.rent_epoch,
                    slot: 0,
                    txn_signature: None,
                    write_version: 0,
                    last_update: chrono::offset::Utc::now()
                };

                self.sol_state.add_account_info(pb, pca.clone());

            });

            let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));
            if !vec_acc.contains(&program_id_str.clone()) {
                vec_acc.push(program_id_str.clone());
                
                crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), vec_acc);
                crate::oracles::create_subscription_oracle::refresh_owner();
            }

            let mut vec_acc_v = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
            let len_origin = vec_acc_v.len();
            vec_acc_v.append(&mut ar_pkey);
            let vec_acc_v_uniq: Vec<String> = vec_acc_v.into_iter().unique().collect();

            if len_origin != vec_acc_v_uniq.len() {
                crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_acc_v_uniq);
                crate::oracles::create_subscription_oracle::refresh_token_account();
            }

            let mut vec_acc_o = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
            if !vec_acc_o.contains(&program_id_str) {
                vec_acc_o.push(program_id_str);
                crate::oracles::create_subscription_oracle::set_mutex_token_owner_sub(String::from("sage"), vec_acc_o);
                crate::oracles::create_subscription_oracle::refresh_token_owner();
            }

            Ok(ar_results)
        }

    }

    pub async fn get_non_cached_token_account_by_owner(
        &self,
        program_id_str: String,
        token_account_filter: RpcTokenAccountsFilter,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<Vec<RpcKeyedAccount>>> {

        info!("[RPC] get_token_account_by_owner request received: {}", program_id_str.to_string());
    
        let res_rpc: RpcResult<Vec<RpcKeyedAccount>> = self.sol_client.send(
            RpcRequest::GetTokenAccountsByOwner,
            json!([program_id_str, token_account_filter, config.clone()]),
        );

        if res_rpc.is_err() {
            return Err(jsonrpc_core::error::Error::internal_error());
        }

        let ar_results = res_rpc.unwrap().clone();

        let mut ar_pkey: Vec<String> = vec![];

        ar_results.value.iter().for_each(|f| {
            let pb = Pubkey::try_from(f.pubkey.as_str()).unwrap();  
            let acc_raw = self.sol_client.get_account(&pb).unwrap();       
            ar_pkey.push(pb.to_string());

            let pca = ProchainAccountInfo {
                data: acc_raw.data.clone(),
                executable: acc_raw.executable,
                lamports: acc_raw.lamports,
                owner: acc_raw.owner.clone(),
                pubkey: pb.clone(),
                rent_epoch: acc_raw.rent_epoch,
                slot: 0,
                txn_signature: None,
                write_version: 0,
                last_update: chrono::offset::Utc::now()
            };

            self.sol_state.add_account_info(pb, pca.clone());

        });

        let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_program_sub(String::from("sage"));
        if !vec_acc.contains(&program_id_str.clone()) {
            vec_acc.push(program_id_str.clone());
            
            crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), vec_acc);
            crate::oracles::create_subscription_oracle::refresh_owner();
        }

        let mut vec_acc_v = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
        let len_origin = vec_acc_v.len();
        vec_acc_v.append(&mut ar_pkey);
        let vec_acc_v_uniq: Vec<String> = vec_acc_v.into_iter().unique().collect();

        if len_origin != vec_acc_v_uniq.len() {
            crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_acc_v_uniq);
            crate::oracles::create_subscription_oracle::refresh_token_account();
        }

        let mut vec_acc_o = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
        if !vec_acc_o.contains(&program_id_str) {
            vec_acc_o.push(program_id_str);
            crate::oracles::create_subscription_oracle::set_mutex_token_owner_sub(String::from("sage"), vec_acc_o);
            crate::oracles::create_subscription_oracle::refresh_token_owner();
        }

        Ok(ar_results)

    }

    pub async fn get_token_accounts_balance(
        &self,
        program_id_str:String,
        config: Option<RpcAccountInfoConfig>,
        force_refresh: Option<bool>
    ) -> Result<Response<UiTokenAmount>> {

        let force_refresh_var = force_refresh.unwrap_or(false);
        let pk = Pubkey::try_from(program_id_str.as_str()).unwrap();
        let cached_acc = self.sol_state.get_account_info(pk.clone());

        let ui_token_amount: UiTokenAmount;

        if cached_acc.is_some() && !force_refresh_var {

            info!("[MEMORY] get_token_accounts_balance request received: {}", program_id_str);

            let c_acc = cached_acc.unwrap();

            if c_acc.is_err() {
                return Err(jsonrpc_core::error::Error::internal_error());
            }

            let c_acc_r = c_acc.unwrap();

            let token_account = StateWithExtensions::<TokenAccount>::unpack(c_acc_r.data())
                .map_err(|_| Error::invalid_params("Invalid param: not a Token account".to_string()))?;
            let mint = Pubkey::try_from(token_account.base.mint.to_string().as_str()).unwrap();
            let (_, data) = self.get_mint_owner_and_additional_data(&mint, force_refresh)?;
            ui_token_amount = token_amount_to_ui_amount_v2(token_account.base.amount, &data);

        } else {

            info!("[RPC] get_token_accounts_balance request received: {}", program_id_str);

            let res = self.sol_client.get_token_account_balance(&pk).unwrap();
            ui_token_amount = res;

            let acc1 = self.sol_client.get_account(&pk).unwrap();
            self.sol_state.add_account_info(pk.clone(), ProchainAccountInfo {
                data: acc1.data,
                executable: acc1.executable,
                lamports: acc1.lamports,
                owner: acc1.owner,
                pubkey: pk.clone(),
                rent_epoch: acc1.rent_epoch,
                slot: 0,
                txn_signature: None,
                write_version: 0,
                last_update: chrono::offset::Utc::now()
            });

            // let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
            // vec_acc.push(pk.to_string());
            // crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
            // crate::oracles::create_subscription_oracle::refresh();

            let mut vec_acc_v = crate::oracles::create_subscription_oracle::get_mutex_token_sub(String::from("sage"));
            if !vec_acc_v.contains(&pk.clone().to_string()) {
                vec_acc_v.push(pk.clone().to_string());
                crate::oracles::create_subscription_oracle::set_mutex_token_sub(String::from("sage"), vec_acc_v);
                crate::oracles::create_subscription_oracle::refresh_token_account();
            }

        }

        Ok(RpcResponse {
            context: RpcResponseContext {
                api_version: None,
                slot: self.sol_state.get_slot()
            },
            value: ui_token_amount
        })

    }

    pub async fn get_latest_blockhash(
        &self
    ) -> Result<Response<RpcBlockhash>>  {

        info!("[RPC] get_latest_blockhash");

        let retries = 4;
        let mut count = 0;
        let mut result_final: (Hash, u64) = (Hash::new_unique(), 0);
        let mut success = false;
        loop {
            let result = self.sol_client.get_latest_blockhash_with_commitment(CommitmentConfig { commitment: CommitmentLevel::Confirmed });
    
            if result.is_ok() {
                result_final = result.unwrap();
                success = true;
                break;
            } else {
                if count > retries {
                    break;
                }
                count += 1;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if success {
            return Ok(new_response(self.sol_state.get_slot() as i64, RpcBlockhash { blockhash: result_final.0.to_string(), last_valid_block_height: result_final.1 }));
        }

        return Err(jsonrpc_core::Error::internal_error());

    }

    pub async fn get_latest_blockhash_with_commitment(
        &self,
        commitment: CommitmentConfig
    ) -> Result<Response<RpcBlockhash>>  {

        info!("[RPC] get_latest_blockhash");

        let retries = 4;
        let mut count = 0;
        let mut result_final: (Hash, u64) = (Hash::new_unique(), 0);
        let mut success = false;
        loop {
            let result = self.sol_client.get_latest_blockhash_with_commitment(CommitmentConfig { commitment: CommitmentLevel::Confirmed });
    
            if result.is_ok() {
                result_final = result.unwrap();
                success = true;
                break;
            } else {
                if count > retries {
                    break;
                }
                count += 1;
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if success {
            return Ok(new_response(self.sol_state.get_slot() as i64, RpcBlockhash { blockhash: result_final.0.to_string(), last_valid_block_height: result_final.1 }));
        }

        return Err(jsonrpc_core::Error::internal_error());

    }

    pub async fn send_transaction(
        &self,
        data: String,
        config: Option<RpcSendTransactionConfig>,
    ) -> Result<String> {
        info!("[RPC] send_transaction");

        let RpcSendTransactionConfig {
            skip_preflight,
            preflight_commitment,
            encoding,
            max_retries,
            min_context_slot,
        } = config.unwrap_or_default();
        let tx_encoding = encoding.unwrap_or(UiTransactionEncoding::Base58);
        let binary_encoding = tx_encoding.into_binary_encoding().ok_or_else(|| {
            Error::invalid_params(format!(
                "unsupported encoding: {tx_encoding}. Supported encodings: base58, base64"
            ))
        })?;
        let (wire_transaction, unsanitized_tx) =
            decode_and_deserialize::<VersionedTransaction>(data, binary_encoding)?;

        return Ok(self.sol_client.send_transaction_with_config(&unsanitized_tx, config.unwrap_or_default()).unwrap().to_string());
    }

    pub async fn get_epoch_info_with_commitment(
        &self,
        config: Option<RpcEpochConfig>,
    ) -> Result<EpochInfo> {
        let def = config.unwrap_or_default();
        let commit = def.commitment.unwrap_or_default();
        info!("[RPC] get_epoch_info_with_commitment");
        return Ok(self.sol_client.get_epoch_info_with_commitment(commit).unwrap());
    }

    pub fn get_signature_statuses(
        &self,
        signatures: Vec<String>,
    ) -> Result<Response<Vec<Option<TransactionStatus>>>> {
        info!("[RPC] get_epoch_info_with_commitment");
        
        let vec_sign: Vec<Signature> = signatures.iter().map(|f| Signature::from_str(f.as_str()).unwrap()).collect();

        let to = self.sol_client.get_signature_statuses(vec_sign.as_slice()).unwrap();

        Ok(RpcResponse {
            context: to.context,
            value: to.value
        })
    }

    pub async fn get_transaction(
        &self,
        signature: Signature,
        config: Option<RpcEncodingConfigWrapper<RpcTransactionConfig>>,
    ) -> Result<Option<EncodedConfirmedTransactionWithStatusMeta>> {

        let config = config
            .map(|config| config.convert_to_current())
            .unwrap_or_default();
        // let encoding = config.encoding.unwrap_or(UiTransactionEncoding::Json);

        // let max_supported_transaction_version = config.max_supported_transaction_version;
        // let commitment = config.commitment.unwrap_or_default();
        
        return Ok(Some(self.sol_client.get_transaction_with_config(&signature, config).unwrap()));

    }

    // pub async fn send_transaction(
    //     &self,
    //     transaction: VersionedTransaction,
    // ) -> Result<Signature>  {
    //     info!("[RPC] send_transaction");
    //     return Ok(self.sol_client.send_transaction(&transaction).unwrap());
    // }

}

const MAX_BASE58_SIZE: usize = 1683; // Golden, bump if PACKET_DATA_SIZE changes
const MAX_BASE64_SIZE: usize = 1644; // Golden, bump if PACKET_DATA_SIZE changes
fn decode_and_deserialize<T>(
    encoded: String,
    encoding: TransactionBinaryEncoding,
) -> Result<(Vec<u8>, T)>
where
    T: serde::de::DeserializeOwned,
{
    let wire_output = match encoding {
        TransactionBinaryEncoding::Base58 => {
            if encoded.len() > MAX_BASE58_SIZE {
                return Err(Error::invalid_params(format!(
                    "base58 encoded {} too large: {} bytes (max: encoded/raw {}/{})",
                    type_name::<T>(),
                    encoded.len(),
                    MAX_BASE58_SIZE,
                    PACKET_DATA_SIZE,
                )));
            }
            bs58::decode(encoded)
                .into_vec()
                .map_err(|e| Error::invalid_params(format!("invalid base58 encoding: {e:?}")))?
        }
        TransactionBinaryEncoding::Base64 => {
            if encoded.len() > MAX_BASE64_SIZE {
                return Err(Error::invalid_params(format!(
                    "base64 encoded {} too large: {} bytes (max: encoded/raw {}/{})",
                    type_name::<T>(),
                    encoded.len(),
                    MAX_BASE64_SIZE,
                    PACKET_DATA_SIZE,
                )));
            }
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|e| Error::invalid_params(format!("invalid base64 encoding: {e:?}")))?
        }
    };
    if wire_output.len() > PACKET_DATA_SIZE {
        return Err(Error::invalid_params(format!(
            "decoded {} too large: {} bytes (max: {} bytes)",
            type_name::<T>(),
            wire_output.len(),
            PACKET_DATA_SIZE
        )));
    }
    bincode::options()
        .with_limit(PACKET_DATA_SIZE as u64)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize_from(&wire_output[..])
        .map_err(|err| {
            Error::invalid_params(format!(
                "failed to deserialize {}: {}",
                type_name::<T>(),
                &err.to_string()
            ))
        })
        .map(|output| (wire_output, output))
}