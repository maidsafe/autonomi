use autonomi::files::{Metadata, PrivateArchive};
use autonomi::{Bytes, Client};
use test_utils::evm::get_funded_wallet;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_env("RUST_LOG"))
        .init();

    let client = Client::init().await?;
    let wallet = get_funded_wallet();

    // Upload 10MiB of random data and verify it by fetching it back.
    let data = Bytes::from("Hello, World!");
    let (_cost, data_map) = client.data_put(data.clone(), (&wallet).into()).await?;
    let data_fetched = client.data_get(&data_map).await?;
    assert_eq!(data, data_fetched);

    // Upload the data as part of an archive, giving it the name `test.txt`.
    let mut archive = PrivateArchive::new();
    archive.add_file(
        "test.txt".into(),
        data_map,
        Metadata::new_with_size(data.len() as u64),
    );

    // Upload the archive to the network.
    let (_cost, archive_data_map) = client.archive_put(&archive, (&wallet).into()).await?;
    let archive_fetched = client.archive_get(&archive_data_map).await?;
    assert_eq!(archive, archive_fetched);

    println!("Archive uploaded successfully");

    Ok(())
}
