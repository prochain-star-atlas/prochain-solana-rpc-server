use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteSnapshotDto {
    pub routes: Vec<String>
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UserFleetCargoItem {

    pub publicKey: String,
    pub tokenMint: String,
    pub tokenAmount: u64

}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UserFleetInstanceRequest {

    pub userId: String,
    pub publicKey: String,

    pub cargoHold: String,
    pub fuelTank: String,
    pub ammoBank: String,

    pub foodToken: String,
    pub fuelToken: String,
    pub ammoToken: String,
    pub sduToken: String,

    pub forceRefresh: bool
    
}

#[derive(Serialize, Deserialize, Debug, Clone, ToSchema)]
pub struct UserFleetInstanceResponse {

    pub userId: String,
    pub publicKey: String,
    pub fleetAcctInfo: String,

    pub foodCnt: u64,
    pub fuelCnt: u64,
    pub ammoCnt: u64,
    pub sduCnt: u64, 

    pub cargoHold: String,
    pub fuelTank: String,
    pub ammoBank: String,

    pub foodToken: String,
    pub fuelToken: String,
    pub ammoToken: String,
    pub sduToken: String,

    pub fleetCargo: Vec<UserFleetCargoItem>,
    
}