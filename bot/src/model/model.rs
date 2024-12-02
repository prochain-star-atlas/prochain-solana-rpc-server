use utoipa::ToSchema;


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ToSchema, PartialEq)]
pub struct GrpcYellowstoneSubscription {
    pub name: String,
    pub accounts: Vec<String>,
    pub token_accounts: Vec<String>,
    pub owners: Vec<String>
}