
use crate::systemd::WatchdogManager;
use anyhow::Result;
use azure_iot_sdk::client::*;
use futures_util::{FutureExt, StreamExt};
use log::{debug, error, info};
use serde_json::json;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook_tokio::Signals;
use std::future::{pending, Future};
use tokio::{
    select,
    sync::mpsc,
    time::{interval, Duration, Interval},
};

pub struct Twin {
    iothub_client: Box<dyn IotHub>,
    authenticated_once: bool,
    tx_reported_properties: mpsc::Sender<serde_json::Value>,
    rx_reported_properties: mpsc::Receiver<serde_json::Value>,
}

impl Twin {
    pub fn new(client: Box<dyn IotHub>) -> Self {
        let (tx_reported_properties, rx_reported_properties) = mpsc::channel(100);

        Twin {
            iothub_client: client,
            tx_reported_properties: tx_reported_properties.clone(),
            rx_reported_properties,
            authenticated_once: false,
        }
    }

    pub async fn init(&mut self) -> Result<()> {        // report version
        self.tx_reported_properties
            .send(json!({
                "module-version": env!("CARGO_PKG_VERSION"),
                "azure-sdk-version": IotHubClient::sdk_version_string()
            }))
            .await?;

        Ok(())
    }

    async fn handle_connection_status(&mut self, auth_status: AuthenticationStatus) -> Result<()> {
        info!("auth_status: {auth_status:#?}");

        match auth_status {
            AuthenticationStatus::Authenticated => {
                self.authenticated_once = true;
            }
            AuthenticationStatus::Unauthenticated(reason) => {
                anyhow::ensure!(
                    matches!(reason, UnauthenticatedReason::ExpiredSasToken),
                    "No connection. Reason: {reason:?}"
                );
            }
        }

        Ok(())
    }

    async fn handle_desired(
        &mut self,
        state: TwinUpdateState,
        desired: serde_json::Value,
    ) -> Result<()> {
        info!("desired: {state:#?}, {desired}");

        match state {
            TwinUpdateState::Partial => {
                /*                 if let Some(gc) = desired.get("general_consent") {
                    self.feature::<DeviceUpdateConsent>()?
                        .update_general_consent(gc.as_array())
                        .await?;
                }

                if let Some(inf) = desired.get("include_network_filter") {
                    self.feature_mut::<NetworkStatus>()?
                        .update_include_network_filter(inf.as_array())
                        .await?;
                } */
            }
            TwinUpdateState::Complete => {
                /*                 if desired.get("desired").is_none() {
                    bail!("handle_desired: 'desired' missing while TwinUpdateState::Complete")
                }

                self.feature::<DeviceUpdateConsent>()?
                    .update_general_consent(desired["desired"]["general_consent"].as_array())
                    .await?;

                self.feature_mut::<NetworkStatus>()?
                    .update_include_network_filter(
                        desired["desired"]["include_network_filter"].as_array(),
                    )
                    .await?; */
            }
        }
        Ok(())
    }

    async fn handle_direct_method(
        &self,
        (method_name, payload, tx_result): (String, serde_json::Value, DirectMethodResult),
    ) -> Result<()> {
        info!("handle_direct_method: {method_name} with payload: {payload}");

        let result = match method_name.as_str() {
            "factory_reset" => Ok(Some(json!({}))),
            &_ => todo!(),
        };

        if tx_result.send(result).is_err() {
            error!("handle_direct_method: receiver dropped");
        }

        Ok(())
    }

    pub async fn run(connection_string: Option<&str>) -> Result<()> {
        let (tx_connection_status, mut rx_connection_status) = mpsc::channel(100);
        let (tx_twin_desired, mut rx_twin_desired) = mpsc::channel(100);
        let (tx_direct_method, mut rx_direct_method) = mpsc::channel(100);

        let mut signals = Signals::new(TERM_SIGNALS)?;

        let mut sd_notify_interval = if let Some(micros) = WatchdogManager::init() {
            let micros = micros / 2;
            debug!("trigger watchdog interval: {micros}µs");
            Some(interval(Duration::from_micros(micros)))
        } else {
            None
        };

        let client = match IotHubClient::client_type() {
            _ if connection_string.is_some() => IotHubClient::from_connection_string(
                connection_string.unwrap(),
                Some(tx_connection_status.clone()),
                Some(tx_twin_desired.clone()),
                Some(tx_direct_method.clone()),
                None,
            )?,
            ClientType::Device | ClientType::Module => {
                IotHubClient::from_identity_service(
                    Some(tx_connection_status.clone()),
                    Some(tx_twin_desired.clone()),
                    Some(tx_direct_method.clone()),
                    None,
                )
                .await?
            }
            ClientType::Edge => IotHubClient::from_edge_environment(
                Some(tx_connection_status.clone()),
                Some(tx_twin_desired.clone()),
                Some(tx_direct_method.clone()),
                None,
            )?,
        };

        let mut twin = Self::new(client);

        loop {
            select! (
                _ =  notify_some_interval(&mut sd_notify_interval) => {
                    WatchdogManager::notify()?;
                },
                _ = signals.next() => {
                    signals.handle().close();
                    twin.iothub_client.shutdown().await;
                    return Ok(())
                },
                status = rx_connection_status.recv() => {
                    twin.handle_connection_status(status.unwrap()).await?;
                },
                desired = rx_twin_desired.recv() => {
                    let (state, desired) = desired.unwrap();
                    twin.handle_desired(state, desired).await.unwrap_or_else(|e| error!("twin update desired properties: {e:#}"));
                },
                reported = twin.rx_reported_properties.recv() => {
                    twin.iothub_client.twin_report(reported.unwrap())?
                },
                direct_methods = rx_direct_method.recv() => {
                    twin.handle_direct_method(direct_methods.unwrap()).await?
                },
            );
        }
    }
}

pub fn notify_some_interval(
    interval: &mut Option<Interval>,
) -> impl Future<Output = tokio::time::Instant> + '_ {
    match interval.as_mut() {
        Some(i) => i.tick().left_future(),
        None => pending().right_future(),
    }
}