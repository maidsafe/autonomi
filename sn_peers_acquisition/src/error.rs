use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
#[allow(missing_docs)]
pub enum Error {
    #[error("Could not parse the supplied multiaddr or socket address")]
    InvalidPeerAddr,
    #[error("Could not obtain network contacts from {0} after {1} retries")]
    NetworkContactsUnretrievable(String, usize),
    #[error("No valid multaddr was present in the contacts file at {0}")]
    NoMultiAddrObtainedFromNetworkContacts(String),
    #[error("Could not obtain peers through any available options")]
    PeersNotObtained,
    #[cfg(feature = "network-contacts")]
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[cfg(feature = "network-contacts")]
    #[error(transparent)]
    UrlParseError(#[from] url::ParseError),
}
