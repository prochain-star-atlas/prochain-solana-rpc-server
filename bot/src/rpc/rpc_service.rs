
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::FutureExt;
use jsonrpsee::core::async_trait;
use jsonrpsee::core::middleware::{Batch, BatchEntry, BatchEntryErr, Notification, RpcServiceBuilder, RpcServiceT};
use jsonrpsee::http_client::HttpClient;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::middleware::http::{HostFilterLayer, ProxyGetRequestLayer};
use jsonrpsee::server::{
	ServerConfig, ServerHandle, StopHandle, TowerServiceBuilder, serve_with_graceful_shutdown, stop_channel,
};
use jsonrpsee::types::{ErrorObject, ErrorObjectOwned, Request};
use jsonrpsee::ws_client::{HeaderValue, WsClientBuilder};
use jsonrpsee::{MethodResponse, Methods};
use tokio::net::TcpListener;
use tower::Service;
use tower_http::cors::CorsLayer;
use tracing_subscriber::util::SubscriberInitExt;

use crate::rpc::rpc::{RpcServer, RpcServerImpl};

#[derive(Default, Clone, Debug)]
pub struct Metrics {
	opened_ws_connections: Arc<AtomicUsize>,
	closed_ws_connections: Arc<AtomicUsize>,
	http_calls: Arc<AtomicUsize>,
	success_http_calls: Arc<AtomicUsize>,
}

pub async fn run_server(metrics: Metrics) -> anyhow::Result<ServerHandle> {
	let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], 14555))).await?;

	// This state is cloned for every connection
	// all these types based on Arcs and it should
	// be relatively cheap to clone them.
	//
	// Make sure that nothing expensive is cloned here
	// when doing this or use an `Arc`.
	#[derive(Clone)]
	struct PerConnection<RpcMiddleware, HttpMiddleware> {
		methods: Methods,
		stop_handle: StopHandle,
		metrics: Metrics,
		svc_builder: TowerServiceBuilder<RpcMiddleware, HttpMiddleware>,
	}

	// Each RPC call/connection get its own `stop_handle`
	// to able to determine whether the server has been stopped or not.
	//
	// To keep the server running the `server_handle`
	// must be kept and it can also be used to stop the server.
	let (stop_handle, server_handle) = stop_channel();

	let per_conn = PerConnection {
		methods: RpcServerImpl.into_rpc().into(),
		stop_handle: stop_handle.clone(),
		metrics,
		svc_builder: jsonrpsee::server::Server::builder()
			.set_config(ServerConfig::builder().max_connections(100).build())
			.set_http_middleware(
				tower::ServiceBuilder::new()
					.layer(CorsLayer::permissive())
			)
			.to_service_builder(),
	};

	tokio::spawn(async move {
		loop {
			// The `tokio::select!` macro is used to wait for either of the
			// listeners to accept a new connection or for the server to be
			// stopped.
			let sock = tokio::select! {
				res = listener.accept() => {
					match res {
						Ok((stream, _remote_addr)) => stream,
						Err(e) => {
							tracing::error!("failed to accept v4 connection: {:?}", e);
							continue;
						}
					}
				}
				_ = per_conn.stop_handle.clone().shutdown() => break,
			};
			let per_conn2 = per_conn.clone();

			let svc = tower::service_fn(move |req| {

				let PerConnection { methods, stop_handle, metrics, svc_builder } = per_conn2.clone();

				let mut svc = svc_builder.build(methods, stop_handle);

                // HTTP.
                async move {
                    tracing::info!("Opened HTTP connection");
                    metrics.http_calls.fetch_add(1, Ordering::Relaxed);
                    let rp = svc.call(req).await;

                    if rp.is_ok() {
                        metrics.success_http_calls.fetch_add(1, Ordering::Relaxed);
                    }

                    tracing::info!("Closed HTTP connection");
                    rp
                }
                .boxed()
			});

			tokio::spawn(serve_with_graceful_shutdown(sock, svc, stop_handle.clone().shutdown()));
		}
	});

	Ok(server_handle)
}