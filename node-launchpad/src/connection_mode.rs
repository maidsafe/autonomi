// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use ant_service_management::NodeServiceData;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter, Result};
use strum::{Display, EnumIter};

#[derive(Clone, Copy, Debug, Default, EnumIter, Eq, Hash, PartialEq)]
pub enum ConnectionMode {
    #[default]
    Automatic,
    UPnP,
    CustomPorts,
}

impl Display for ConnectionMode {
    fn fmt(&self, f: &mut Formatter) -> Result {
        match self {
            ConnectionMode::UPnP => write!(f, "UPnP"),
            ConnectionMode::CustomPorts => write!(f, "Custom Ports"),
            ConnectionMode::Automatic => write!(f, "Automatic"),
        }
    }
}

impl<'de> Deserialize<'de> for ConnectionMode {
    fn deserialize<D>(deserializer: D) -> std::result::Result<ConnectionMode, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "UPnP" => Ok(ConnectionMode::UPnP),
            "Custom Ports" => Ok(ConnectionMode::CustomPorts),
            "Automatic" => Ok(ConnectionMode::Automatic),
            _ => Err(serde::de::Error::custom(format!(
                "Invalid ConnectionMode: {s:?}"
            ))),
        }
    }
}

impl Serialize for ConnectionMode {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            ConnectionMode::UPnP => "UPnP",
            ConnectionMode::CustomPorts => "Custom Ports",
            ConnectionMode::Automatic => "Automatic",
        };
        serializer.serialize_str(s)
    }
}

#[derive(Default, Debug, Clone, Serialize, Display)]
pub enum NodeConnectionMode {
    UPnP,
    Relay,
    Manual,
    #[default]
    Unknown,
}

impl From<&NodeServiceData> for NodeConnectionMode {
    fn from(nsd: &NodeServiceData) -> Self {
        if nsd.no_upnp {
            Self::UPnP
        } else {
            Self::Manual
        }
    }
}
