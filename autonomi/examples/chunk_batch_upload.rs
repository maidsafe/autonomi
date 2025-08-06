// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

// This example demonstrates how to upload multiple chunks of data in a single transaction using the `chunk_batch_upload` method.

use ant_protocol::storage::DataTypes;
use autonomi::{Client, Wallet, data::DataAddress, self_encryption::encrypt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::init_local().await?;
    let wallet = Wallet::new_from_private_key(
        client.evm_network().clone(),
        "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    )?;

    // Step 1: Encrypt your data using self-encryption
    let (data_map, chunks) = encrypt("Hello, World!".into())?;

    // Step 2: Collect all chunks (data map + content chunks)
    let mut all_chunks = vec![&data_map];
    all_chunks.extend(chunks.iter());

    // Step 3: Get storage quotes for all chunks
    let quote = client
        .get_store_quotes(
            DataTypes::Chunk,
            all_chunks
                .iter()
                .map(|chunk| (*chunk.address().xorname(), chunk.size())),
        )
        .await?;

    // Step 4: Pay for all chunks at once and get receipt
    wallet
        .pay_for_quotes(quote.payments())
        .await
        .map_err(|err| err.0)?;
    let receipt = autonomi::client::payment::receipt_from_store_quotes(quote);

    // Step 5: Upload all chunks with the payment receipt
    client.chunk_batch_upload(all_chunks, &receipt).await?;

    // Step 6: Fetch the data using the DataMap address
    let addr = DataAddress::new(*data_map.address().xorname());
    let data = client.data_get_public(&addr).await?;
    // Prints `Hello, World!`
    println!("{}", String::from_utf8_lossy(&data));

    Ok(())
}
