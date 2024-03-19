use crate::{
    database::{database_description::off_chain::OffChain, Database},
    graphql_api::worker_service,
    service::Config,
};
use fuel_core_chain_config::TableEntry;
use fuel_core_storage::{
    tables::{Coins, Messages},
    transactional::WriteTransaction,
};
use fuel_core_types::{entities::coins::coin::Coin, services::executor::Event};
use std::borrow::Cow;

fn process_messages(
    original_database: &mut Database<OffChain>,
    messages: Vec<TableEntry<Messages>>,
) -> anyhow::Result<()> {
    let mut database_transaction = original_database.write_transaction();

    let message_events = messages
        .into_iter()
        .map(|message| Cow::Owned(Event::MessageImported(message.value)));

    worker_service::process_executor_events(message_events, &mut database_transaction)?;

    database_transaction.commit()?;
    Ok(())
}

fn process_coins(
    original_database: &mut Database<OffChain>,
    coins: Vec<TableEntry<Coins>>,
) -> anyhow::Result<()> {
    let mut database_transaction = original_database.write_transaction();

    let coin_events = coins.iter().map(|coin_entry| {
        let coin = Coin {
            utxo_id: coin_entry.key,
            owner: *coin_entry.value.owner(),
            amount: *coin_entry.value.amount(),
            asset_id: *coin_entry.value.asset_id(),
            tx_pointer: *coin_entry.value.tx_pointer(),
        };
        Cow::Owned(Event::CoinCreated(coin))
    });

    worker_service::process_executor_events(coin_events, &mut database_transaction)?;

    database_transaction.commit()?;
    Ok(())
}

/// Performs the importing of the genesis block from the snapshot.
// TODO: The regenesis of the off-chain database should go in the same way as the on-chain database.
//  https://github.com/FuelLabs/fuel-core/issues/1619
pub fn execute_genesis_block(
    config: &Config,
    original_database: &mut Database<OffChain>,
) -> anyhow::Result<()> {
    for message_group in config.state_reader.read()? {
        process_messages(original_database, message_group?.data)?;
    }

    for coin_group in config.state_reader.read()? {
        process_coins(original_database, coin_group?.data)?;
    }

    Ok(())
}
