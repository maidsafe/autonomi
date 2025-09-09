// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// Allow unwrap_used and expect_used in this TUI crate - to be refactored
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
// Allow unused_async - to be refactored
#![allow(clippy::unused_async)]

pub mod action;
pub mod app;
pub mod components;
pub mod config;
pub mod connection_mode;
pub mod focus;
pub mod keybindings;
pub mod log_management;
pub mod mode;
pub mod node_management;
pub mod node_stats;
pub mod runtime;
pub mod style;
pub mod system;
pub mod tui;
pub mod utils;
pub mod widgets;

pub mod test_utils;

#[macro_use]
extern crate tracing;
