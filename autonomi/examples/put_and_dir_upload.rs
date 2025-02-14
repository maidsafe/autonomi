use autonomi::{Bytes, Client};
use test_utils::evm::get_funded_wallet;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_env("RUST_LOG"))
        .init();

    let client = Client::init().await?;
    let wallet = get_funded_wallet();

    // Put and fetch data.
    let (_cost, data_addr) = client
        .data_put_public(Bytes::from("Hello, World"), (&wallet).into())
        .await?;
    let _data_fetched = client.data_get_public(&data_addr).await?;

    // Put and fetch directory from local file system.
    let (_cost, dir_addr) = client
        .dir_upload_public("files/to/upload".into(), wallet.into())
        .await?;
    client
        .dir_download_public(&dir_addr, "files/downloaded".into())
        .await?;

    Ok(())
}
