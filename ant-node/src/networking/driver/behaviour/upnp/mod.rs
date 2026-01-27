// Copyright (C) 2026 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

//! Implementation of UPnP port mapping for libp2p.
//!
//! This crate provides a `tokio::Behaviour` which
//! implements the [`libp2p_swarm::NetworkBehaviour`] trait.
//! This struct will automatically try to map the ports externally to internal
//! addresses on the gateway.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub(crate) mod behaviour;
pub(crate) mod tokio;
