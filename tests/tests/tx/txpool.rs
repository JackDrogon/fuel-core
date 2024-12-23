//! Tests the behaviour of the tx pool.

use crate::helpers::{
    TestContext,
    TestSetupBuilder,
};
use fuel_core_poa::Trigger;
use fuel_core_types::{
    fuel_asm::*,
    fuel_crypto::*,
    fuel_tx,
    fuel_tx::*,
};
use itertools::Itertools;
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};

#[tokio::test]
async fn txs_max_script_gas_limit() {
    const MAX_GAS_LIMIT: u64 = 5_000_000_000;
    let mut rng = StdRng::seed_from_u64(2322);
    let mut test_builder = TestSetupBuilder::new(2322);
    test_builder.gas_limit = Some(MAX_GAS_LIMIT);
    // initialize 10 random transactions that transfer coins
    let transactions = (1..=10)
        .map(|i| {
            TransactionBuilder::script(
                op::ret(RegId::ONE).to_bytes().into_iter().collect(),
                vec![],
            )
            .script_gas_limit(MAX_GAS_LIMIT / 2)
            .add_unsigned_coin_input(
                SecretKey::random(&mut rng),
                rng.gen(),
                1000 + i,
                Default::default(),
                Default::default(),
            )
            .add_output(Output::Change {
                amount: 0,
                asset_id: Default::default(),
                to: rng.gen(),
            })
            .finalize()
        })
        .collect_vec();

    // setup genesis block with coins that transactions can spend
    test_builder.config_coin_inputs_from_transactions(&transactions);
    test_builder.trigger = Trigger::Never;

    // spin up node
    let TestContext { client, srv, .. } = test_builder.finalize().await;

    // submit transactions and verify their status
    let txs = transactions
        .clone()
        .into_iter()
        .map(fuel_tx::Transaction::from)
        .collect::<Vec<_>>();
    for tx in txs {
        srv.shared.txpool_shared_state.insert(tx).await.unwrap();
    }

    client.produce_blocks(1, None).await.unwrap();
    let block = client.block_by_height(1.into()).await.unwrap().unwrap();
    assert_eq!(
        block.transactions.len(),
        transactions.len() + 1 // coinbase
    )
}
