use std::{
    error::Error,
    future::{self, Ready},
    net::Ipv4Addr,
};

use actix_web::{
    middleware::Logger,
    web::{self},
    App,  HttpServer,
};
use utoipa::{OpenApi};
use crate::http::{solana, staratlas};
use utoipa_rapidoc::RapiDoc;
use utoipa_swagger_ui::SwaggerUi;

pub fn start_httpd() {


    let _1 = std::thread::spawn(|| {
    
        let _t = start_internal_httpd().is_ok();

    });

}

#[tokio::main]
async fn start_internal_httpd() -> Result<(), impl Error> {
    //env_logger::init();

    #[derive(OpenApi)]
    #[openapi(
        nest(
            (path = "/solana-api", api = solana::SolanaApi),
            (path = "/staratlas-api", api = staratlas::StarAtlasApi),
        )
    )]
    struct ApiDoc;

    // Make instance variable of ApiDoc so all worker threads gets the same instance.
    let openapi = ApiDoc::openapi();

    HttpServer::new(move || {
        // This factory closure is called on each worker thread independently.
        App::new()
            .wrap(Logger::default())
            //.service(web::scope("/todoapi").configure(todo::configure(store.clone())))
            .service(web::scope("/solana-api").configure(solana::configure()))
            .service(web::scope("/staratlas-api").configure(staratlas::configure()))
            //.service(Redoc::with_url("/redoc", openapi.clone()))
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}").url("/api-docs/openapi.json", openapi.clone()),
            )
            // There is no need to create RapiDoc::with_openapi because the OpenApi is served
            // via SwaggerUi. Instead we only make rapidoc to point to the existing doc.
            //
            // If we wanted to serve the schema, the following would work:
            .service(RapiDoc::with_openapi("/api-docs/openapi2.json", openapi.clone()).path("/rapidoc"))
            //.service(RapiDoc::new("/api-docs/openapi.json").path("/rapidoc"))
            //.service(Scalar::with_url("/scalar", openapi.clone()))
    })
    .bind((Ipv4Addr::UNSPECIFIED, 4477))?
    .run()
    .await
}

