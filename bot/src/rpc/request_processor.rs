use std::{cmp::min, collections::HashMap};

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
use solana_client::{rpc_filter::Memcmp, rpc_request::TokenAccountsFilter};
use jsonrpc_core::{types::error, types::Error};
use solana_inline_spl::{
    token::{SPL_TOKEN_ACCOUNT_MINT_OFFSET, SPL_TOKEN_ACCOUNT_OWNER_OFFSET},
    token_2022::{self, ACCOUNTTYPE_ACCOUNT},
};
use solana_account_decoder::{
    parse_account_data::{AccountAdditionalDataV2, SplTokenAdditionalData}, parse_token::{
        get_token_account_mint, is_known_spl_token_id, token_amount_to_ui_amount_v2, UiTokenAmount
    }, UiAccountData, UiDataSliceConfig, MAX_BASE58_BYTES
};
use spl_token_2022::{
    extension::StateWithExtensions,
    solana_program::program_pack::Pack,
    state::{Account as TokenAccount, Mint},
};
use solana_sdk::{commitment_config::CommitmentConfig, stake::config};
use solana_sdk::pubkey::PUBKEY_BYTES;
use itertools::Itertools;
use solana_sdk::account::ReadableAccount;
type RpcCustomResult<T> = std::result::Result<T, RpcCustomError>;

use solana_inline_spl::{token::GenericTokenAccount, token_2022::Account};

use base64::Engine;

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
    ) -> Result<(Pubkey, Option<Pubkey>)> {
        match token_account_filter {
            TokenAccountsFilter::Mint(mint) => {
                let (mint_owner, _) = self.get_mint_owner_and_additional_data(&mint)?;
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

    fn get_prochain_account(&self, pubkey: &Pubkey, config: RpcAccountInfoConfig) -> ProchainAccountInfo {

        let cached_acc = self.sol_state.get_account_info(pubkey.clone());

        if cached_acc.is_some() {

            info!("[MEMORY] get_account_info request received: {}", pubkey.to_string());

            let c_acc = cached_acc.unwrap().unwrap();
            return c_acc;

        } else {

            info!("[RPC] get_account_info request received: {}", pubkey.to_string());

            let res = self.sol_client.get_account_with_config(pubkey, config).unwrap();
            let acc_pk = res.value.clone().unwrap_or_default();

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

            return pa.clone();
        }

    }

    /// Get account infor for a single account with the pubkey.
    pub async fn get_account_info(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Option<UiAccount>>> {

        let config = config.unwrap_or_default();

        let pro_account = self.get_prochain_account(pubkey, config.clone());

        let ui_account = UiAccount::encode(pubkey, &pro_account.clone(), config.clone().encoding.unwrap(), None, config.clone().data_slice);
        let slot: u64 = self.sol_state.get_slot();

        Ok(RpcResponse {
            context: RpcResponseContext {
                api_version: None,
                slot: slot
            },
            value: Some(ui_account)
        })

    }

    /// Load multiple accounts
    pub async fn get_multiple_accounts(
        &self,
        pubkeys: Vec<Pubkey>,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Vec<Option<UiAccount>>>> {

        info!("getting get_multiple_accounts is called for {:?}", pubkeys);
        let config = config.unwrap_or_default();

        let mut accounts = Vec::new();
        let slot = self.sol_state.get_slot();

        for pubkey in pubkeys {

            let ui_account;
            let cached_acc = self.sol_state.get_account_info(pubkey.clone());
            if cached_acc.is_some() {

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
    ) -> Result<(Pubkey, SplTokenAdditionalData)> {
        if mint.to_string() == spl_token::native_mint::id().to_string() {
            Ok((
                Pubkey::try_from(spl_token::id().to_string().as_str()).unwrap(),
                SplTokenAdditionalData::with_decimals(spl_token::native_mint::DECIMALS),
            ))
        } else {
            let config = RpcAccountInfoConfig { commitment: Some(CommitmentConfig::confirmed()), encoding: None, data_slice: None, min_context_slot: None };
            let mint_account = self.get_prochain_account(&mint.clone(), config);
            let mint_data = self.get_additional_mint_data(mint_account.data()).unwrap();
            Ok((*mint_account.owner(), mint_data))
        }
    }

    pub fn get_parsed_token_accounts(
        &self,
        keyed_accounts: Vec<ProchainAccountInfo>,
    ) -> Vec<RpcKeyedAccount>
    {
        let mut mint_data: HashMap<Pubkey, AccountAdditionalDataV2> = HashMap::new();
        let r: Vec<RpcKeyedAccount> = keyed_accounts.iter().filter_map(move |ka| {
            let additional_data = get_token_account_mint(&ka.data()).and_then(|mint_pubkey| {
                mint_data.get(&mint_pubkey).cloned().or_else(|| {
                    let (_, data) = self.get_mint_owner_and_additional_data(&mint_pubkey).ok()?;
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
        config: Option<RpcProgramAccountsConfig>
    ) -> Result<OptionalContext<Vec<RpcKeyedAccount>>> {

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

        if lst_ps.contains(&program_id.to_string()) {

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
                self.get_parsed_token_accounts(keyed_accounts)
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
            crate::oracles::create_subscription_oracle::refresh();
        }

        Ok(ar_keyed_acc).map(|result| match c.clone().with_context.unwrap_or_default() {
            true => OptionalContext::Context(new_response(0, result)),
            false => OptionalContext::NoContext(result),
        })

    }

    pub async fn get_token_supply(
        &self,
        token_id_str: String,
        config: Option<RpcTransactionLogsConfig>
    ) -> Result<solana_client::rpc_response::Response<UiTokenAmount>> {

        let pk = Pubkey::try_from(token_id_str.as_str()).unwrap();
        let cached_acc = self.sol_state.get_account_info(pk.clone());
        let opt: RpcTransactionLogsConfig = config.unwrap_or(RpcTransactionLogsConfig { commitment: Some(CommitmentConfig::confirmed()) });

        let ui_token_amount: UiTokenAmount;

        if cached_acc.is_some() {

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
            let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
            vec_acc.push(pk.to_string());
            crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
            crate::oracles::create_subscription_oracle::refresh();

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
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<Vec<RpcKeyedAccount>>> {

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

        if lst_ps.contains(&program_id_str) {

            let taf = match token_account_filter {
                RpcTokenAccountsFilter::Mint(m) => TokenAccountsFilter::Mint(Pubkey::try_from(m.as_str()).unwrap()),
                RpcTokenAccountsFilter::ProgramId(p) => TokenAccountsFilter::ProgramId(Pubkey::try_from(p.as_str()).unwrap())
            };

            let pb = Pubkey::try_from(program_id_str.as_str()).unwrap(); 
            let (token_program_id, mint) = self.get_token_program_id_and_mint(taf)?;

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
                self.get_parsed_token_accounts(keyed_accounts)
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

            let mut ar_pkey: Vec<Pubkey> = vec![];

            ar_results.value.iter().for_each(|f| {
                let pb = Pubkey::try_from(f.pubkey.as_str()).unwrap();  
                let acc_raw = self.sol_client.get_account(&pb).unwrap();       
                ar_pkey.push(pb);

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
            vec_acc.push(program_id_str);
            let v: Vec<_> = vec_acc.into_iter().unique().collect();
            crate::oracles::create_subscription_oracle::set_mutex_program_sub(String::from("sage"), v);
            crate::oracles::create_subscription_oracle::refresh();

            Ok(ar_results)
        }

    }

    pub async fn get_token_accounts_balance(
        &self,
        program_id_str:String,
        config: Option<RpcAccountInfoConfig>
    ) -> Result<Response<UiTokenAmount>> {

        let pk = Pubkey::try_from(program_id_str.as_str()).unwrap();
        let cached_acc = self.sol_state.get_account_info(pk.clone());

        let ui_token_amount: UiTokenAmount;

        if cached_acc.is_some() {

            info!("[MEMORY] get_token_accounts_balance request received: {}", program_id_str);

            let c_acc = cached_acc.unwrap();

            if c_acc.is_err() {
                return Err(jsonrpc_core::error::Error::internal_error());
            }

            let c_acc_r = c_acc.unwrap();

            let token_account = StateWithExtensions::<TokenAccount>::unpack(c_acc_r.data())
                .map_err(|_| Error::invalid_params("Invalid param: not a Token account".to_string()))?;
            let mint = Pubkey::try_from(token_account.base.mint.to_string().as_str()).unwrap();
            let (_, data) = self.get_mint_owner_and_additional_data(&mint)?;
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
            let mut vec_acc = crate::oracles::create_subscription_oracle::get_mutex_account_sub(String::from("sage"));
            vec_acc.push(pk.to_string());
            crate::oracles::create_subscription_oracle::set_mutex_account_sub(String::from("sage"), vec_acc);
            crate::oracles::create_subscription_oracle::refresh();

        }

        Ok(RpcResponse {
            context: RpcResponseContext {
                api_version: None,
                slot: self.sol_state.get_slot()
            },
            value: ui_token_amount
        })

    }

}
