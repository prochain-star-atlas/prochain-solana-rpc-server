use actix_web::{
    get, web::ServiceConfig, HttpResponse, Responder
};
use serde::{Deserialize, Serialize};
use utoipa::{OpenApi, ToSchema};
use chrono::{DateTime, Utc};

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SystemDateTime {
    pub datetime: String,
    pub timestamp: i64,
    pub timezone: String,
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_system_datetime
    ),
    components(schemas(SystemDateTime))
)]
pub(super) struct SystemApi;

pub(super) fn configure() -> impl FnOnce(&mut ServiceConfig) {
    |config: &mut ServiceConfig| {
        config
            .service(get_system_datetime);
    }
}

#[utoipa::path(
    responses(
        (status = 200, description = "Get current system date and time", body = SystemDateTime)
    )
)]
#[get("/system/datetime")]
async fn get_system_datetime() -> impl Responder {
    let now: DateTime<Utc> = Utc::now();
    
    let system_datetime = SystemDateTime {
        datetime: now.to_rfc3339(),
        timestamp: now.timestamp(),
        timezone: "UTC".to_string(),
    };

    HttpResponse::Ok().json(system_datetime)
} 