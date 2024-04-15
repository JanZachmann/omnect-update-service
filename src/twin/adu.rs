use anyhow::{ensure, Context, Result};
use serde::Serialize;
use serde_json::json;
use std::fs::OpenOptions;
use tokio::sync::mpsc::Sender;

#[macro_export]
macro_rules! adu_config_path {
    () => {{
        if cfg!(feature = "mock") {
            "testfiles/du-config.json"
        } else {
            "/etc/adu/du-config.json"
        }
    }};
}

#[macro_export]
macro_rules! sw_versions_path {
    () => {{
        if cfg!(feature = "mock") {
            "testfiles/sw-versions"
        } else {
            "/etc/sw-versions"
        }
    }};
}

#[derive(Serialize)]
struct DeviceInformation {
    __t: String,
    manufacturer: String,
    model: String,
    osName: String,
    swVersion: String,
    processorArchitecture: String,
    processorManufacturer: String,
    totalMemory: u32,
    totalStorage: u32,
}

#[derive(Serialize)]
struct DeviceProperties {
    manufacturer: String,
    model: String,
    compatibilityid: String,
    contractModelId: String,
    aduVer: String,
}

#[derive(Serialize)]
struct Agent {
    deviceProperties: DeviceProperties,
    compatPropertyNames: String,
}

#[derive(Serialize)]
struct DeviceUpdate {
    __t: String,
    agent: Agent,
}

pub struct Adu {
    tx_reported_properties: Sender<serde_json::Value>,
    device_info: DeviceInformation,
    device_update: DeviceUpdate,
}

impl Adu {
    pub fn new(tx_reported_properties: Sender<serde_json::Value>) -> Result<Self> {
        let du_config: serde_json::Value = serde_json::from_reader(
            OpenOptions::new()
                .read(true)
                .create(false)
                .open(adu_config_path!())
                .context("cannot read du-config.json")?,
        )
        .context("cannot parse du-config.json")?;

        let sw_versions =
            std::fs::read_to_string(sw_versions_path!()).context("cannot read sw-versions")?;

        let sw_versions: Vec<&str> = sw_versions.split(' ').collect();

        // ToDo: validate by regex?
        ensure!(
            sw_versions.len() == 2,
            "sw-versions: unexpected number of entries"
        );

        // ToDo 1: read cpu arch and manufacturer
        // ToDo 2: periodically read and report memory consumption
        let device_info = DeviceInformation {
            __t: "c".to_owned(),
            manufacturer: du_config["manufacturer"].as_str().unwrap().to_owned(),
            model: du_config["model"].as_str().unwrap().to_owned(),
            osName: sw_versions[0].to_owned(),
            swVersion: sw_versions[1].to_owned(),
            processorArchitecture: "aarch64".to_owned(),
            processorManufacturer: "ARM".to_owned(),
            totalMemory: 123456,
            totalStorage: 654321,
        };

        let device_properties = DeviceProperties {
            manufacturer: du_config["agents"]["manufacturer"]
                .as_str()
                .unwrap()
                .to_owned(),
            model: du_config["agents"]["model"].as_str().unwrap().to_owned(),
            compatibilityid: du_config["agents"]["additionalDeviceProperties"]["compatibilityid"]
                .as_str()
                .unwrap()
                .to_owned(),
            contractModelId: "dtmi:azure:iot:deviceUpdateContractModel;3".to_owned(),
            aduVer: "DU;agent/1.1.0".to_owned(),
        };

        let agent = Agent {
            deviceProperties: device_properties,
            compatPropertyNames: du_config["compatPropertyNames"]
                .as_str()
                .unwrap()
                .to_owned(),
        };

        let device_update = DeviceUpdate {
            __t: "c".to_owned(),
            agent,
        };

        Ok(Adu {
            tx_reported_properties,
            device_info,
            device_update,
        })
    }

    pub async fn report_initial_state(&self) -> Result<()> {
        self.report_device_info().await?;
        self.report_device_update().await
    }

    async fn report_device_info(&self) -> Result<()> {
        self.tx_reported_properties
            .send(json!({
                "deviceInformation": serde_json::to_value(&self.device_info)?
            }))
            .await
            .context("report_consent: report_impl")
    }

    async fn report_device_update(&self) -> Result<()> {
        self.tx_reported_properties
            .send(json!({
                "deviceUpdate": serde_json::to_value(&self.device_update)?
            }))
            .await
            .context("report_consent: report_impl")
    }
}
/*
"DeviceInformation" {
  __t: "c",
  "manufacturer": "conplement-AG",
  "model": "OMNECT-raspberrypi4-64",
  "osName": "OMNECT-gateway-devel",
  "swVersion": "4.0.17.356884934",
  "processorArchitecture": "aarch64",
  "processorManufacturer": "ARM",
  "totalMemory": 3884332,
  "totalStorage": 28828168
},

"deviceUpdate": {
    "__t": "c",
    "agent": {
        "deviceProperties": {
            "manufacturer": "conplement-ag",
            "model": "omnect-raspberrypi4-64-gateway-devel",
            "compatibilityid": "2",
            "contractModelId": "dtmi:azure:iot:deviceUpdateContractModel;3",
            "aduVer": "DU;agent/1.1.0"
        },
        "compatPropertyNames": "manufacturer,model,compatibilityid",
        "lastInstallResult": {
            "stepResults": {
                "step_0": {
                    "resultCode": 603,
                    "extendedResultCodes": "00000000"
                },
                "step_1": {
                    "resultCode": 603,
                    "extendedResultCodes": "00000000",
                    "resultDetails": ""
                }
            },
            "resultCode": 700,
            "extendedResultCodes": "00000000,A0000FFF",
            "resultDetails": ""
        },
        "state": 0,
        "workflow": {
            "action": 3,
            "id": "6423dd30aacfd9abd4f75103-2024310125857"
        },
        "installedUpdateId": "{\"provider\":\"conplement-AG\",\"name\":\"OMNECT-gateway-devel\",\"version\":\"4.0.17.356884934\"}"
    },
    "service": {
        "value": {
            "workflow": {
                "action": 3,
                "id": "6423dd30aacfd9abd4f75103-2024310125857"
            },
            "updateManifest": "{\"manifestVersion\":\"5\",\"updateId\":{\"provider\":\"conplement-AG\",\"name\":\"OMNECT-gateway-devel\",\"version\":\"4.0.17.356884934\"},\"compatibility\":[{\"compatibilityid\":\"2\",\"manufacturer\":\"conplement-ag\",\"model\":\"omnect-raspberrypi4-64-gateway-devel\"}],\"instructions\":{\"steps\":[{\"handler\":\"omnect/swupdate_consent:1\",\"files\":[\"f06b052ef8f38d681\"],\"handlerProperties\":{\"installedCriteria\":\"OMNECT-gateway-devel 4.0.17.356884934\"}},{\"handler\":\"microsoft/swupdate:2\",\"files\":[\"f06b052ef8f38d681\",\"fa1e0bfac52315908\"],\"handlerProperties\":{\"installedCriteria\":\"OMNECT-gateway-devel 4.0.17.356884934\",\"swuFileName\":\"OMNECT-gateway-devel_4.0.17.356884934_raspberrypi4-64.swu\",\"arguments\":\"\",\"scriptFileName\":\"OMNECT-gateway-devel_4.0.17.356884934_raspberrypi4-64.swu.sh\"}}]},\"files\":{\"f06b052ef8f38d681\":{\"fileName\":\"OMNECT-gateway-devel_4.0.17.356884934_raspberrypi4-64.swu\",\"sizeInBytes\":207453184,\"hashes\":{\"sha256\":\"hgGl5j56skgp48m8f6a3hb5PS47XF7cW1eeRCtXFA08=\"}},\"fa1e0bfac52315908\":{\"fileName\":\"OMNECT-gateway-devel_4.0.17.356884934_raspberrypi4-64.swu.sh\",\"sizeInBytes\":25405,\"hashes\":{\"sha256\":\"TvjNmFoidHG1P/ytKaApnASRP4p7QKERRWB+8I84hq4=\"}}},\"createdDateTime\":\"2024-04-04T19:18:19.0123317Z\"}",
            "rootKeyPackageUrl": "http://omnect-cp-dev-instance--omnect-cp-dev-adu.b.nlu.dl.adu.microsoft.com/WestEurope/rootkeypackages/rootkeypackage-1.json"
        },
        "ac": 200,
        "ad": "",
        "av": 3
    }
}, */
