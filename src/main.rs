// ToDo rm
#![allow(non_snake_case)]

pub mod systemd;
pub mod twin;

use azure_iot_sdk::client::*;
use env_logger::{Builder, Env, Target};
use log::{error, info};
use std::io::Write;
use std::process;
use twin::Twin;

#[tokio::main]
async fn main() {
    let mut builder;
    log_panics::init();

    if cfg!(debug_assertions) {
        builder = Builder::from_env(Env::default().default_filter_or("debug"));
    } else {
        builder = Builder::from_env(Env::default().default_filter_or("info"));
    }

    builder.format(|buf, record| match record.level() {
        log::Level::Info => writeln!(buf, "<6>{}: {}", record.target(), record.args()),
        log::Level::Warn => writeln!(buf, "<4>{}: {}", record.target(), record.args()),
        log::Level::Error => {
            eprintln!("<3>{}: {}", record.target(), record.args());
            Ok(())
        }
        _ => writeln!(buf, "<7>{}: {}", record.target(), record.args()),
    });

    builder.target(Target::Stdout).init();

    info!(
        "module version: {} ({})",
        env!("CARGO_PKG_VERSION"),
        env!("GIT_SHORT_REV")
    );
    info!("azure sdk version: {}", IotHubClient::sdk_version_string());

    if let Err(e) = Twin::run(Some("HostName=omnect-cp-dev-iot-hub.azure-devices.net;DeviceId=test-jza;ModuleId=AducIotAgent;SharedAccessKey=LLasex8ndfxepk2du9hm4hqL3GH4AiXZMccUxELHxoI=")).await {
    //if let Err(e) = Twin::run(None).await {
        error!("application error: {e:#}");

        process::exit(1);
    }

    info!("application shutdown")
}
