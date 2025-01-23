use crate::client::{payment::PayError, quote::CostError, Client};
use ant_evm::{Amount, AttoTokens, EvmWallet, EvmWalletError};
use ant_networking::{GetRecordCfg, NetworkError, PutRecordCfg, ResponseQuorum, VerificationKind};
use ant_protocol::{
    storage::{try_serialize_record, DataTypes, PointerAddress, RecordKind},
    NetworkAddress,
};
use bls::SecretKey;
use libp2p::kad::Record;
use tracing::{debug, error, trace};

pub use ant_protocol::storage::Pointer;

#[derive(Debug, thiserror::Error)]
pub enum PointerError {
    #[error("Cost error: {0}")]
    Cost(#[from] CostError),
    #[error("Network error")]
    Network(#[from] NetworkError),
    #[error("Serialization error")]
    Serialization,
    #[error("Pointer could not be verified (corrupt)")]
    Corrupt,
    #[error("Payment failure occurred during pointer creation.")]
    Pay(#[from] PayError),
    #[error("Failed to retrieve wallet payment")]
    Wallet(#[from] EvmWalletError),
    #[error("Received invalid quote from node, this node is possibly malfunctioning, try another node by trying another pointer name")]
    InvalidQuote,
    #[error("Pointer already exists at this address: {0:?}")]
    PointerAlreadyExists(PointerAddress),
}

impl Client {
    /// Get a pointer from the network
    pub async fn pointer_get(&self, address: PointerAddress) -> Result<Pointer, PointerError> {
        todo!("update to newer version");
        let key = NetworkAddress::from_pointer_address(address).to_record_key();
        let record = self.network.get_local_record(&key).await?;

        match record {
            Some(record) => {
                let (_, pointer): (Vec<u8>, Pointer) = rmp_serde::from_slice(&record.value)
                    .map_err(|_| PointerError::Serialization)?;
                Ok(pointer)
            }
            None => Err(PointerError::Corrupt),
        }
    }

    /// Store a pointer on the network
    pub async fn pointer_put(
        &self,
        pointer: Pointer,
        wallet: &EvmWallet,
    ) -> Result<PointerAddress, PointerError> {
        let address = pointer.network_address();

        // pay for the pointer storage
        let xor_name = *address.xorname();
        debug!("Paying for pointer at address: {address:?}");
        let (payment_proofs, _skipped_payments) = self
            // TODO: define Pointer default size for pricing
            .pay(
                DataTypes::Pointer.get_index(),
                std::iter::once((xor_name, 128)),
                wallet,
            )
            .await
            .inspect_err(|err| {
                error!("Failed to pay for pointer at address: {address:?} : {err}")
            })?;

        // verify payment was successful
        let (proof, _price) = match payment_proofs.get(&xor_name) {
            Some((proof, price)) => (proof, price),
            None => {
                error!("Pointer at address: {address:?} was already paid for");
                return Err(PointerError::PointerAlreadyExists(address));
            }
        };

        let payees = proof.payees();

        let record = Record {
            key: NetworkAddress::from_pointer_address(address).to_record_key(),
            value: try_serialize_record(
                &(proof, &pointer),
                RecordKind::DataWithPayment(DataTypes::Pointer),
            )
            .map_err(|_| PointerError::Serialization)?
            .to_vec(),
            publisher: None,
            expires: None,
        };

        let get_cfg = GetRecordCfg {
            get_quorum: self
                .operation_config
                .pointer_operation_config
                .verification_quorum,
            retry_strategy: self
                .operation_config
                .pointer_operation_config
                .verification_retry_strategy,
            target_record: None,
            expected_holders: Default::default(),
        };

        let put_cfg = PutRecordCfg {
            put_quorum: ResponseQuorum::All,
            retry_strategy: self
                .operation_config
                .pointer_operation_config
                .write_retry_strategy,
            verification: Some((VerificationKind::Crdt, get_cfg)),
            use_put_record_to: Some(payees),
        };

        // store the pointer on the network
        debug!("Storing pointer at address {address:?} to the network");
        self.network
            .put_record(record, &put_cfg)
            .await
            .inspect_err(|err| {
                error!("Failed to put record - pointer {address:?} to the network: {err}")
            })?;

        Ok(address)
    }

    /// Calculate the cost of storing a pointer
    pub async fn pointer_cost(&self, key: SecretKey) -> Result<AttoTokens, PointerError> {
        let pk = key.public_key();
        trace!("Getting cost for pointer of {pk:?}");

        let address = PointerAddress::from_owner(pk);
        let xor = *address.xorname();
        // TODO: define default size of Pointer
        let store_quote = self
            .get_store_quotes(DataTypes::Pointer.get_index(), std::iter::once((xor, 128)))
            .await?;
        let total_cost = AttoTokens::from_atto(
            store_quote
                .0
                .values()
                .map(|quote| quote.price())
                .sum::<Amount>(),
        );
        debug!("Calculated the cost to create pointer of {pk:?} is {total_cost}");
        Ok(total_cost)
    }
}
