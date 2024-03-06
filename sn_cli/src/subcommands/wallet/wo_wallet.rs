// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::{helpers::verify_spend, watch_only_wallet_from_pk, WalletApiHelper};

use bls::{PublicKey, PK_SIZE};
use clap::Parser;
use color_eyre::{
    eyre::{bail, eyre},
    Result,
};
use dialoguer::Confirm;
use sn_client::transfers::{
    CashNoteRedemption, DerivationIndex, MainPubkey, NanoTokens, OfflineTransfer, SignedSpend,
    UniquePubkey, WalletError, WatchOnlyWallet,
};
use sn_client::{Client, ClientEvent};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    str::FromStr,
};
use walkdir::WalkDir;

const DEFAULT_RECEIVE_ONLINE_WALLET_DIR: &str = "receive_online";
const ROYALTY_TRANSFER_NOTIF_TOPIC: &str = "ROYALTY_TRANSFER_NOTIFICATION";

// Please do not remove the blank lines in these doc comments.
// They are used for inserting line breaks when the help menu is rendered in the UI.
#[derive(Parser, Debug)]
pub enum WatchOnlyWalletCmds {
    /// Print the watch-only wallets addresses.
    Addresses,
    /// Print the wallet balance.
    Balance {
        /// The hex-encoded public key of an existing watch-only wallet.
        #[clap(name = "public key")]
        pk: Option<String>,
    },
    /// Deposit CashNotes from the received directory to the chosen watch-only wallet.
    /// Or Read a hex encoded CashNote from stdin.
    ///
    /// The default received directory is platform specific:
    ///  - Linux: $HOME/.local/share/safe/client/\<pk\>/cash_notes
    ///  - macOS: $HOME/Library/Application Support/safe/client/\<pk\>/cash_notes
    ///  - Windows: C:\Users\{username}\AppData\Roaming\safe\client\\<pk\>\cash_notes
    ///
    /// If you find the default path unwieldy, you can also set the RECEIVED_CASHNOTES_PATH environment
    /// variable to a path you would prefer to work with.
    #[clap(verbatim_doc_comment)]
    Deposit {
        /// Read a hex encoded CashNote from stdin.
        #[clap(long, default_value = "false")]
        stdin: bool,
        /// The hex encoded CashNote.
        #[clap(long)]
        cash_note: Option<String>,
        /// The hex-encoded public key of an existing watch-only wallet to deposit into it.
        #[clap(name = "public key")]
        pk: String,
    },
    /// Create a watch-only wallet from the given (hex-encoded) key.
    Create {
        /// Hex-encoded main public key.
        #[clap(name = "public key")]
        pk: String,
    },
    /// Builds an unsigned transaction to be signed offline. It requires an existing watch-only wallet.
    Transaction {
        /// Hex-encoded public key of the source watch-only wallet.
        #[clap(name = "from")]
        from: String,
        /// The number of SafeNetworkTokens to transfer.
        #[clap(name = "amount")]
        amount: String,
        /// Hex-encoded public address of the recipient.
        #[clap(name = "to")]
        to: String,
    },
    /// This command will create the cash note for the recipient and broadcast it to the network.
    ///
    /// This cash note can then be shared with the recipient, who can then
    /// use the 'deposit' command to use/claim the funds.
    Broadcast {
        /// Hex-encoded signed transaction.
        #[clap(name = "signed Tx")]
        signed_tx: String,
        /// Avoid prompts by assuming `yes` as the answer.
        #[clap(long, name = "force", default_value = "false")]
        force: bool,
    },
    /// Listen for transfer notifications from the network over gossipsub protocol.
    ///
    /// Transfers will be deposited to the watch-only wallet.
    ///
    /// Only cash notes owned by the provided public key will be accepted, verified to be valid
    /// against the network, and deposited onto a locally stored watch-only wallet.
    ReceiveOnline {
        /// Hex-encoded main public key
        #[clap(name = "public key")]
        pk: String,
        /// Optional path where to store the wallet
        #[clap(name = "path")]
        path: Option<PathBuf>,
    },
    /// Verify a spend on the Network.
    Verify {
        /// The Network address or hex encoded UniquePubkey of the Spend to verify
        #[clap(name = "spend")]
        spend_address: String,
        /// Verify all the way to Genesis
        ///
        /// Used for auditing, note that this might take a very long time
        /// Analogous to verifying an UTXO through the entire blockchain in Bitcoin
        #[clap(long, default_value = "false")]
        genesis: bool,
    },
}

pub(crate) async fn wo_wallet_cmds_without_client(
    cmds: &WatchOnlyWalletCmds,
    root_dir: &Path,
) -> Result<()> {
    match cmds {
        WatchOnlyWalletCmds::Addresses => {
            let wallets = get_watch_only_wallets(root_dir)?;
            println!(
                "Addresses of {} watch-only wallets found at {}:",
                wallets.len(),
                root_dir.display()
            );
            for (wo_wallet, _) in wallets {
                println!("- {:?}", wo_wallet.address());
            }
            Ok(())
        }
        WatchOnlyWalletCmds::Balance { pk } => {
            if let Some(pk) = pk {
                let main_pk = MainPubkey::from_hex(pk)?;
                let watch_only_wallet = watch_only_wallet_from_pk(main_pk, root_dir)?;
                println!("{}", watch_only_wallet.balance());
            } else {
                let wallets = get_watch_only_wallets(root_dir)?;
                println!(
                    "Balances of {} watch-only wallets found at {}:",
                    wallets.len(),
                    root_dir.display()
                );
                let mut total = NanoTokens::zero();
                for (wo_wallet, folder_name) in wallets {
                    let balance = wo_wallet.balance();
                    println!("{folder_name}: {balance}");
                    total = total
                        .checked_add(balance)
                        .ok_or(eyre!("Failed to add to total balance"))?;
                }
                println!("Total: {total}");
            }
            Ok(())
        }
        WatchOnlyWalletCmds::Deposit {
            stdin,
            cash_note,
            pk,
        } => {
            let main_pk = MainPubkey::from_hex(pk)?;
            let mut wallet = WalletApiHelper::watch_only_from_pk(main_pk, root_dir)?;
            wallet.deposit(*stdin, cash_note.as_deref())
        }
        WatchOnlyWalletCmds::Create { pk } => {
            let pk = PublicKey::from_hex(pk)
                .map_err(|err| eyre!("Failed to parse hex-encoded PK: {err:?}"))?;
            let main_pk = MainPubkey::new(pk);
            let main_pubkey = main_pk.public_key();
            let watch_only_wallet = watch_only_wallet_from_pk(main_pk, root_dir)?;
            let balance = watch_only_wallet.balance();
            println!("Watch-only wallet created (balance {balance}) for main public key: {main_pubkey:?}.");
            Ok(())
        }
        WatchOnlyWalletCmds::Transaction { from, amount, to } => {
            build_unsigned_transaction(from, amount, to, root_dir)
        }
        cmd => Err(eyre!("{cmd:?} requires us to be connected to the Network")),
    }
}

pub(crate) async fn wo_wallet_cmds(
    cmds: WatchOnlyWalletCmds,
    client: &Client,
    root_dir: &Path,
    verify_store: bool,
) -> Result<()> {
    match cmds {
        WatchOnlyWalletCmds::Broadcast { signed_tx, force } => {
            broadcast_signed_spends(signed_tx, client, verify_store, force).await
        }
        WatchOnlyWalletCmds::ReceiveOnline { pk, path } => {
            let wallet_dir = path.unwrap_or(root_dir.join(DEFAULT_RECEIVE_ONLINE_WALLET_DIR));
            listen_notifs_and_deposit(&wallet_dir, client, pk).await
        }
        WatchOnlyWalletCmds::Verify {
            spend_address,
            genesis,
        } => verify_spend(spend_address, genesis, client).await,
        cmd => Err(eyre!(
            "{cmd:?} has to be processed before connecting to the network"
        )),
    }
}

fn get_watch_only_wallets(root_dir: &Path) -> Result<Vec<(WatchOnlyWallet, String)>> {
    let mut wallets = vec![];
    for entry in WalkDir::new(root_dir.display().to_string())
        .into_iter()
        .flatten()
    {
        if let Some(file_name) = entry.path().file_name().and_then(|name| name.to_str()) {
            if file_name.starts_with("pk_") {
                let wallet_dir = root_dir.join(file_name);
                if let Ok(wo_wallet) = WatchOnlyWallet::load_from_path(&wallet_dir) {
                    wallets.push((wo_wallet, file_name.to_string()));
                }
            }
        }
    }
    if wallets.is_empty() {
        bail!("No watch-only wallets found at {}", root_dir.display());
    }

    Ok(wallets)
}

fn build_unsigned_transaction(from: &str, amount: &str, to: &str, root_dir: &Path) -> Result<()> {
    let main_pk = MainPubkey::from_hex(from)?;
    let mut wallet = watch_only_wallet_from_pk(main_pk, root_dir)?;
    let amount = match NanoTokens::from_str(amount) {
        Ok(amount) => amount,
        Err(err) => {
            println!("The amount cannot be parsed. Nothing sent.");
            return Err(err.into());
        }
    };
    let to = match MainPubkey::from_hex(to) {
        Ok(to) => to,
        Err(err) => {
            println!("Error while parsing the recipient's 'to' key: {err:?}");
            return Err(err.into());
        }
    };

    let unsigned_transfer = wallet.build_unsigned_transaction(vec![(amount, to)], None)?;

    println!(
        "The unsigned transaction has been successfully created:\n\n{}\n",
        hex::encode(rmp_serde::to_vec(&unsigned_transfer)?)
    );
    println!("Please copy the above text, sign it offline with 'wallet sign' cmd, and then use the signed transaction to broadcast it with 'wallet broadcast' cmd.");

    Ok(())
}

async fn broadcast_signed_spends(
    signed_tx: String,
    client: &Client,
    verify_store: bool,
    force: bool,
) -> Result<()> {
    let (signed_spends, output_details, change_id): (
        BTreeSet<SignedSpend>,
        BTreeMap<UniquePubkey, (MainPubkey, DerivationIndex)>,
        UniquePubkey,
    ) = rmp_serde::from_slice(&hex::decode(signed_tx)?)?;

    println!("The signed transaction has been successfully decoded:");
    let mut transaction = None;
    for (i, signed_spend) in signed_spends.iter().enumerate() {
        println!("\nSpending input #{i}:");
        println!("\tKey: {}", signed_spend.unique_pubkey().to_hex());
        println!("\tAmount: {}", signed_spend.token());
        let linked_tx = signed_spend.spent_tx();
        if let Some(ref tx) = transaction {
            if tx != &linked_tx {
                bail!("Transaction seems corrupted, not all Spends (inputs) refer to the same transaction");
            }
        } else {
            transaction = Some(linked_tx);
        }

        if let Err(err) = signed_spend.verify(signed_spend.spent_tx_hash()) {
            bail!("Transaction is invalid: {err:?}");
        }
    }

    let tx = if let Some(tx) = transaction {
        for (i, output) in tx.outputs.iter().enumerate() {
            println!("\nOutput #{i}:");
            println!("\tKey: {}", output.unique_pubkey.to_hex());
            println!("\tAmount: {}", output.amount);
        }
        tx
    } else {
        bail!("Transaction is corrupted, no transaction information found.");
    };

    if !force {
        println!(
            "\n** Please make sure the above information is correct before broadcasting it. **\n"
        );
        let confirmation = Confirm::new()
            .with_prompt("Do you want to broadcast the above transaction?")
            .interact()?;

        if !confirmation {
            println!("Transaction was not broadcasted.");
            return Ok(());
        }
    }

    println!("Broadcasting the transaction to the network...");
    let transfer = OfflineTransfer::from_transaction(signed_spends, tx, change_id, output_details)?;

    // return the first CashNote (assuming there is only one because we only sent to one recipient)
    let cash_note = match &transfer.created_cash_notes[..] {
        [cashnote] => cashnote.to_hex()?,
        [_multiple, ..] => bail!("Multiple CashNotes were returned from the transaction when only one was expected. This is a BUG."),
        [] =>bail!("No CashNotes were built from the Tx.")
    };

    // send to network
    client
        .send_spends(transfer.all_spend_requests.iter(), verify_store)
        .await
        .map_err(|err| {
            eyre!("The transfer was not successfully registered in the network: {err:?}")
        })?;

    println!("Transaction broadcasted!.");

    println!("The recipient's cash note has been successfully created.");
    println!("Please share this to the recipient:\n\n{cash_note}\n");
    println!("The recipient can then use the wallet 'deposit' command to verify the transfer, and/or be able to use the funds.\n");

    if let Some(cash_note) = transfer.change_cash_note {
        println!(
            "A change cash note has also been created:\n\n{}\n",
            cash_note.to_hex()?
        );
        println!("You should use the wallet 'deposit' command to be able to use these funds.\n");
    }

    Ok(())
}

async fn listen_notifs_and_deposit(root_dir: &Path, client: &Client, pk_hex: String) -> Result<()> {
    let mut wallet = match MainPubkey::from_hex(&pk_hex) {
        Ok(main_pk) => watch_only_wallet_from_pk(main_pk, root_dir)?,
        Err(err) => return Err(eyre!("Failed to parse hex-encoded public key: {err:?}")),
    };

    let main_pk = wallet.address();
    let pk = main_pk.public_key();

    client.subscribe_to_topic(ROYALTY_TRANSFER_NOTIF_TOPIC.to_string())?;
    let mut events_receiver = client.events_channel();

    println!("Current balance in local wallet: {}", wallet.balance());
    println!("Listening to transfers notifications for {pk:?}... (press Ctrl+C to exit)");
    println!();

    while let Ok(event) = events_receiver.recv().await {
        let cash_notes = match event {
            ClientEvent::GossipsubMsg { topic, msg } => {
                // we assume it's a notification of a transfer as that's the only topic we've subscribed to
                match try_decode_transfer_notif(&msg) {
                    Err(err) => {
                        println!("GossipsubMsg received on topic '{topic}' couldn't be decoded as transfer notif: {err:?}");
                        continue;
                    }
                    Ok((key, _)) if key != pk => continue,
                    Ok((key, cashnote_redemptions)) => {
                        println!("New transfer notification received for {key:?}, containing {} CashNoteRedemption/s.", cashnote_redemptions.len());
                        match client
                            .verify_cash_notes_redemptions(main_pk, &cashnote_redemptions)
                            .await
                        {
                            Err(err) => {
                                println!("At least one of the CashNoteRedemptions received is invalid, dropping them: {err:?}");
                                continue;
                            }
                            Ok(cash_notes) => cash_notes,
                        }
                    }
                }
            }
            _other_event => continue,
        };

        cash_notes.iter().for_each(|cn| {
            let value = match cn.value() {
                Ok(value) => value.to_string(),
                Err(err) => {
                    println!("Failed to obtain cash note value: {err}");
                    "unknown".to_string()
                }
            };
            println!(
                "CashNote received with {:?}, value: {value}",
                cn.unique_pubkey(),
            );
        });

        match wallet.deposit_and_store_to_disk(&cash_notes) {
            Ok(()) => {}
            Err(err @ WalletError::Io(_)) => {
                println!("ERROR: Failed to deposit the received cash notes: {err}");
                println!();
                println!("WARNING: we'll try to reload/recreate the local wallet now, but if it was corrupted there could have been lost funds.");
                println!();
                wallet.reload_from_disk_or_recreate()?;
                wallet.deposit_and_store_to_disk(&cash_notes)?;
            }
            Err(other_err) => return Err(other_err.into()),
        }

        println!(
            "New balance after depositing received CashNote/s: {}",
            wallet.balance()
        );
        println!();
    }

    Ok(())
}

fn try_decode_transfer_notif(msg: &[u8]) -> Result<(PublicKey, Vec<CashNoteRedemption>)> {
    let mut key_bytes = [0u8; PK_SIZE];
    key_bytes.copy_from_slice(
        msg.get(0..PK_SIZE)
            .ok_or_else(|| eyre!("msg doesn't have enough bytes"))?,
    );
    let key = PublicKey::from_bytes(key_bytes)?;
    let cashnote_redemptions: Vec<CashNoteRedemption> = rmp_serde::from_slice(&msg[PK_SIZE..])?;
    Ok((key, cashnote_redemptions))
}
