use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RouteSnapshotDto {
    pub routes: Vec<String>
}
