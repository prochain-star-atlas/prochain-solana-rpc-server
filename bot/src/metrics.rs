use log::info;
use metrics::{histogram};
use metrics_exporter_prometheus::PrometheusBuilder;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::net::SocketAddr;
use tokio::net::TcpListener;

async fn get_available_port(listen_address: [u8; 4]) -> u16 {
    let socket_address = SocketAddr::from((listen_address, 0));
    TcpListener::bind(socket_address)
        .await
        .unwrap_or_else(|e| {
            panic!("Unable to bind to an available port on address {socket_address}: {:?}", e);
        })
        .local_addr()
        .expect("Unable to obtain local address from TcpListener")
        .port()
}

pub async fn add_metrics_exporter() {


    let local = [0, 0, 0, 0];
    //let port = get_available_port(local).await;
    let socket_address = SocketAddr::from((local, 9132));

    let builder = PrometheusBuilder::new();
    let _1 = {
        builder.with_http_listener(socket_address).install().unwrap_or_else(
            |e| panic!("failed to create Prometheus recorder and http listener: {:?}", e),
        )
    };
    info!("started metrics on: {}:{}", socket_address.ip(), socket_address.port());

    // let now = Instant::now(); 
    // let sys_now = SystemTime::now();
    // let clock: Clock = Clock::new();

    histogram!("prochain_executor_rust_bot_start", "bot start" => "Time").record(SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as f64);
}