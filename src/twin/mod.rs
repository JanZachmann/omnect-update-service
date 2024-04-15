use crate::systemd::WatchdogManager;
mod adu;
use crate::twin::adu::Adu;
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
    adu: Adu,
}

impl Twin {
    pub fn new(client: Box<dyn IotHub>) -> Result<Self> {
        let (tx_reported_properties, rx_reported_properties) = mpsc::channel(100);

        let adu = Adu::new(tx_reported_properties.clone())?;

        Ok(Twin {
            iothub_client: client,
            tx_reported_properties: tx_reported_properties.clone(),
            rx_reported_properties,
            authenticated_once: false,
            adu,
        })
    }

    async fn init(&mut self) -> Result<()> {
        // report version
        self.tx_reported_properties
            .send(json!({
                "module-version": env!("CARGO_PKG_VERSION"),
                "azure-sdk-version": IotHubClient::sdk_version_string()
            }))
            .await?;

        self.adu.report_initial_state().await
    }

    async fn handle_connection_status(&mut self, auth_status: AuthenticationStatus) -> Result<()> {
        info!("auth_status: {auth_status:#?}");

        match auth_status {
            AuthenticationStatus::Authenticated => {
                self.authenticated_once = true;
                self.init().await?;
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

    pub async fn run() -> Result<()> {
        let (tx_connection_status, mut rx_connection_status) = mpsc::channel(100);
        let (tx_twin_desired, mut rx_twin_desired) = mpsc::channel(100);

        let mut signals = Signals::new(TERM_SIGNALS)?;

        let mut sd_notify_interval = if let Some(micros) = WatchdogManager::init() {
            let micros = micros / 2;
            debug!("trigger watchdog interval: {micros}µs");
            Some(interval(Duration::from_micros(micros)))
        } else {
            None
        };

        let builder = IotHubClient::builder()
            .observe_connection_state(tx_connection_status)
            .observe_desired_properties(tx_twin_desired)
            .pnp_model_id("dtmi:azure:iot:deviceUpdateModel;3");

        let mut twin = if cfg!(feature = "mock") {
            Self::new(
                builder
                    .build_module_client(&std::env::var("CONNECTION_STRING").unwrap())
                    .unwrap(),
            )?
        } else {
            Self::new(builder.build_module_client_from_identity().await.unwrap())?
        };

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
            );
        }
    }
}

fn notify_some_interval(
    interval: &mut Option<Interval>,
) -> impl Future<Output = tokio::time::Instant> + '_ {
    match interval.as_mut() {
        Some(i) => i.tick().left_future(),
        None => pending().right_future(),
    }
}
