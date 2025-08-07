// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::networking::{NetworkError, Result};
use futures::Future;
use hyper::{Body, Method, Request, Response, Server, StatusCode, service::Service};
use prometheus_client::{encoding::text::encode, registry::Registry};
use std::time::Duration;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::watch;

/// The types of metrics that are exposed via the various endpoints.
#[derive(Default, Debug)]
pub(crate) struct MetricsRegistries {
    pub standard_metrics: Registry,
    pub extended_metrics: Registry,
    pub metadata: Registry,
    pub metadata_extended: Registry,
}

const METRICS_CONTENT_TYPE: &str = "application/openmetrics-text;charset=utf-8;version=1.0.0";

/// Runs the metrics server on the specified port.
/// Returns a `watch::Sender<bool>` that can be used to signal the server to shut down.
pub(crate) fn run_metrics_server(registries: MetricsRegistries, port: u16) -> watch::Sender<bool> {
    let addr = ([127, 0, 0, 1], port).into();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    #[allow(clippy::let_underscore_future)]
    let _ = tokio::spawn(async move {
        let server = {
            let mut retries = 0;
            loop {
                match Server::try_bind(&addr) {
                    Ok(server) => {
                        info!(
                            "Successfully bound metrics server to {} after {} retries",
                            addr, retries
                        );
                        break server.serve(MakeMetricService::new(registries));
                    }
                    Err(err) => {
                        retries += 1;
                        if retries >= 5 {
                            error!(
                                "Failed to bind metrics server to {addr} after 5 retries: {err}",
                            );
                            return;
                        }
                        warn!(
                            "Failed to bind metrics server to {addr}: {err}. Retrying in 1 second... (attempt {retries}/5)",
                        );
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        };
        // keep these for programs that might be grepping this info
        info!("Metrics server on http://{}/metrics", server.local_addr());
        println!("Metrics server on http://{}/metrics", server.local_addr());

        info!(
            "Metrics server on http://{} Available endpoints: /metrics, /metrics_extended, /metadata, /metadata_extended",
            server.local_addr()
        );

        // run the server with graceful shutdown
        let graceful = server.with_graceful_shutdown(async {
            if shutdown_rx.changed().await.is_ok() && *shutdown_rx.borrow() {
                info!("Received shutdown signal, shutting down metrics server...");
            };
        });

        if let Err(err) = graceful.await {
            error!("Metrics server error on {addr}: {err:?}");
        } else {
            info!("Metrics server on {addr} shut down gracefully");
        }
    });

    shutdown_tx
}

type SharedRegistry = Arc<Mutex<Registry>>;

pub(crate) struct MetricService {
    standard_registry: SharedRegistry,
    extended_registry: SharedRegistry,
    metadata: SharedRegistry,
    metadata_extended: SharedRegistry,
}

impl MetricService {
    fn get_standard_metrics_registry(&mut self) -> SharedRegistry {
        Arc::clone(&self.standard_registry)
    }

    fn get_extended_metrics_registry(&mut self) -> SharedRegistry {
        Arc::clone(&self.extended_registry)
    }

    fn get_metadata_registry(&mut self) -> SharedRegistry {
        Arc::clone(&self.metadata)
    }

    fn get_metadata_extended_registry(&mut self) -> SharedRegistry {
        Arc::clone(&self.metadata_extended)
    }

    fn respond_with_metrics(&mut self) -> Result<Response<String>> {
        let mut response: Response<String> = Response::default();

        let _ = response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            METRICS_CONTENT_TYPE
                .try_into()
                .map_err(|_| NetworkError::NetworkMetricError)?,
        );

        let reg = self.get_standard_metrics_registry();
        let reg = reg.lock().map_err(|_| NetworkError::NetworkMetricError)?;
        encode(&mut response.body_mut(), &reg).map_err(|err| {
            error!("Failed to encode the standard metrics Registry {err:?}");
            NetworkError::NetworkMetricError
        })?;

        *response.status_mut() = StatusCode::OK;

        Ok(response)
    }

    fn respond_with_metrics_extended(&mut self) -> Result<Response<String>> {
        let mut response: Response<String> = Response::default();

        let _ = response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            METRICS_CONTENT_TYPE
                .try_into()
                .map_err(|_| NetworkError::NetworkMetricError)?,
        );

        let standard_registry = self.get_standard_metrics_registry();
        let standard_registry = standard_registry
            .lock()
            .map_err(|_| NetworkError::NetworkMetricError)?;
        encode(&mut response.body_mut(), &standard_registry).map_err(|err| {
            error!("Failed to encode the standard metrics Registry {err:?}");
            NetworkError::NetworkMetricError
        })?;

        // remove the EOF line from the response
        let mut buffer = response.body().split("\n").collect::<Vec<&str>>();
        let _ = buffer.pop();
        let _ = buffer.pop();
        buffer.push("\n");
        let mut buffer = buffer.join("\n");
        let _ = buffer.pop();
        *response.body_mut() = buffer;

        let extended_registry = self.get_extended_metrics_registry();
        let extended_registry = extended_registry
            .lock()
            .map_err(|_| NetworkError::NetworkMetricError)?;
        encode(&mut response.body_mut(), &extended_registry).map_err(|err| {
            error!("Failed to encode the standard metrics Registry {err:?}");
            NetworkError::NetworkMetricError
        })?;

        *response.status_mut() = StatusCode::OK;

        Ok(response)
    }

    // send a json response of the metadata key, value
    fn respond_with_metadata(&mut self) -> Result<Response<String>> {
        let mut response: Response<String> = Response::default();

        let _ = response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            METRICS_CONTENT_TYPE
                .try_into()
                .map_err(|_| NetworkError::NetworkMetricError)?,
        );

        let reg = self.get_metadata_registry();
        let reg = reg.lock().map_err(|_| NetworkError::NetworkMetricError)?;
        encode(&mut response.body_mut(), &reg).map_err(|err| {
            error!("Failed to encode the metadata Registry {err:?}");
            NetworkError::NetworkMetricError
        })?;

        *response.status_mut() = StatusCode::OK;

        Ok(response)
    }

    fn respond_with_metadata_extended(&mut self) -> Result<Response<String>> {
        let mut response: Response<String> = Response::default();

        let _ = response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            METRICS_CONTENT_TYPE
                .try_into()
                .map_err(|_| NetworkError::NetworkMetricError)?,
        );

        let reg = self.get_metadata_extended_registry();
        let reg = reg.lock().map_err(|_| NetworkError::NetworkMetricError)?;
        encode(&mut response.body_mut(), &reg).map_err(|err| {
            error!("Failed to encode the metadata Registry {err:?}");
            NetworkError::NetworkMetricError
        })?;

        *response.status_mut() = StatusCode::OK;

        Ok(response)
    }

    fn respond_with_404_not_found(&mut self) -> Response<String> {
        let mut resp = Response::default();
        *resp.status_mut() = StatusCode::NOT_FOUND;
        *resp.body_mut() = "Not found try localhost:[port]/metrics".to_string();
        resp
    }

    fn respond_with_500_server_error(&mut self) -> Response<String> {
        let mut resp = Response::default();
        *resp.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
        *resp.body_mut() = "Something went wrong with the Metrics server".to_string();
        resp
    }
}

impl Service<Request<Body>> for MetricService {
    type Response = Response<String>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let req_path = req.uri().path();
        let req_method = req.method();
        let resp = if (req_method == Method::GET) && (req_path == "/metrics") {
            // Encode and serve metrics from registry.
            match self.respond_with_metrics() {
                Ok(resp) => resp,
                Err(_) => self.respond_with_500_server_error(),
            }
        } else if req_method == Method::GET && req_path == "/metrics_extended" {
            // Encode and serve metrics from registry.
            match self.respond_with_metrics_extended() {
                Ok(resp) => resp,
                Err(_) => self.respond_with_500_server_error(),
            }
        } else if req_method == Method::GET && req_path == "/metadata" {
            match self.respond_with_metadata() {
                Ok(resp) => resp,
                Err(_) => self.respond_with_500_server_error(),
            }
        } else if req_method == Method::GET && req_path == "/metadata_extended" {
            match self.respond_with_metadata_extended() {
                Ok(resp) => resp,
                Err(_) => self.respond_with_500_server_error(),
            }
        } else {
            self.respond_with_404_not_found()
        };
        Box::pin(async { Ok(resp) })
    }
}

pub(crate) struct MakeMetricService {
    standard_registry: SharedRegistry,
    extended_registry: SharedRegistry,
    metadata: SharedRegistry,
    metadata_extended: SharedRegistry,
}

impl MakeMetricService {
    pub(crate) fn new(registries: MetricsRegistries) -> MakeMetricService {
        MakeMetricService {
            standard_registry: Arc::new(Mutex::new(registries.standard_metrics)),
            extended_registry: Arc::new(Mutex::new(registries.extended_metrics)),
            metadata: Arc::new(Mutex::new(registries.metadata)),
            metadata_extended: Arc::new(Mutex::new(registries.metadata_extended)),
        }
    }
}

impl<T> Service<T> for MakeMetricService {
    type Response = MetricService;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let standard_registry = Arc::clone(&self.standard_registry);
        let extended_registry = Arc::clone(&self.extended_registry);
        let metadata = Arc::clone(&self.metadata);
        let metadata_extended = Arc::clone(&self.metadata_extended);

        let fut = async move {
            Ok(MetricService {
                standard_registry,
                extended_registry,
                metadata,
                metadata_extended,
            })
        };
        Box::pin(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_metrics_server_graceful_restart() {
        let port = 8081; // Use a specific test port
        let registries = MetricsRegistries::default();

        let shutdown_tx_1 = run_metrics_server(registries, port);

        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(
            reqwest::get(&format!("http://127.0.0.1:{port}/metrics"))
                .await
                .is_ok(),
            "Metrics server should be running on port {port}"
        );

        let _ = shutdown_tx_1.send(true);

        let registries2 = MetricsRegistries::default();
        let shutdown_tx_2 = run_metrics_server(registries2, port);

        let result = timeout(Duration::from_secs(10), async {
            loop {
                match reqwest::get(&format!("http://127.0.0.1:{port}/metrics")).await {
                    Ok(response) => {
                        println!(
                            "Second server responded successfully with status: {}",
                            response.status()
                        );
                        break; // Server is working, break out of loop
                    }
                    Err(err) => {
                        println!("Server not responding, retrying... Error: {err}");
                        // Server not responding, wait a bit and check if the spawn task finished
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        // If we reach here multiple times, it indicates the server failed to start
                        // due to port binding issues
                    }
                }
            }
        })
        .await;

        // Clean up
        let _ = shutdown_tx_2.send(true);

        // The test verifies that the server can restart successfully after graceful shutdown
        // With tokio::sync::watch, the server shuts down gracefully allowing for proper restart
        assert!(
            result.is_ok(),
            "Server should restart successfully after graceful shutdown"
        );
    }
}
