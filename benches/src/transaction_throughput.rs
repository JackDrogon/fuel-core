#[cfg(test)]
mod tests {
    use fuel_core::{service::config::Trigger, upgradable_executor::native_executor::ports::TransactionExt};
    use fuel_core_chain_config::CoinConfig;
    use fuel_core_storage::transactional::AtomicView;
    use fuel_core_types::{
        fuel_asm::{
            op,
            RegId,
        },
        fuel_crypto::*,
        fuel_tx::{
            input::coin::{CoinPredicate, CoinSigned}, AssetId, Finalizable, Input, Output, Transaction, TransactionBuilder
        },
        fuel_vm::{
            checked_transaction::{
                CheckPredicateParams,
                EstimatePredicates,
            },
            interpreter::MemoryInstance,
            predicate::EmptyStorage,
        },
    };
    use rand::{
        rngs::StdRng,
        Rng,
        SeedableRng,
    };
    use test_helpers::builder::{
        local_chain_config,
        TestContext,
        TestSetupBuilder,
    };
    fn checked_parameters() -> CheckPredicateParams {
        local_chain_config().consensus_parameters.into()
    }

    #[test]
    fn test_txs() {
        let n = std::env::var("BENCH_TXS_NUMBER")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap();

        let use_txs_file = std::env::var("USE_TXS_FILE")
        .ok()
        .and_then(|s| s.parse::<bool>().ok())
        .unwrap_or(false);
    
        #[cfg(feature = "parallel-executor")]
        let number_of_cores = std::env::var("FUEL_BENCH_CORES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap();

        let generator = |rng: &mut StdRng, secret_key: SecretKey| {
            let predicate = op::ret(RegId::ONE).to_bytes().to_vec();
            let owner = Input::predicate_owner(&predicate);
            let mut tx = TransactionBuilder::script(vec![], vec![])
                .script_gas_limit(10000)
                .add_unsigned_coin_input(
                    secret_key,
                    rng.gen(),
                    1000,
                    Default::default(),
                    Default::default(),
                )
                .add_input(Input::coin_predicate(
                    rng.gen(),
                    owner,
                    1000,
                    Default::default(),
                    Default::default(),
                    Default::default(),
                    predicate.clone(),
                    vec![],
                ))
                .add_output(Output::coin(rng.gen(), 50, AssetId::default()))
                .add_output(Output::change(rng.gen(), 0, AssetId::default()))
                .finalize();
            tx.estimate_predicates(
                &checked_parameters(),
                MemoryInstance::new(),
                &EmptyStorage,
            )
            .expect("Predicate check failed");
            tx.into()
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _drop = rt.enter();

        let mut rng = rand::rngs::StdRng::seed_from_u64(2322u64);

        let start_transaction_generation = std::time::Instant::now();
        let transactions = if use_txs_file {
            let transactions: Vec<Transaction> = serde_json::from_reader(std::fs::File::open("transactions.json").unwrap()).unwrap();
            println!(
                "Loaded {} transactions in {:?} ms.",
                transactions.len(),
                start_transaction_generation.elapsed().as_millis()
            );
            transactions
        } else {
            let mut transactions: Vec<Transaction> = Vec::with_capacity(n as usize);
            let secret_key = SecretKey::random(&mut rng);
            for _ in 0..n {
                transactions.push(generator(&mut rng, secret_key));
            }
            transactions
        };
        println!(
            "Generated {} transactions in {:?} ms.",
            n,
            start_transaction_generation.elapsed().as_millis()
        );
        // serde_json::to_writer(std::fs::File::create("transactions.json").unwrap(), &transactions)
        // .unwrap();


        let mut test_builder = TestSetupBuilder::new(2322);
        // setup genesis block with coins that transactions can spend
        // We don't use the function to not have to convert Script to transactions
        test_builder.initial_coins.extend(
            transactions
                .iter()
                .flat_map(|t| t.inputs().unwrap())
                .filter_map(|input| {
                    if let Input::CoinSigned(CoinSigned {
                        amount,
                        owner,
                        asset_id,
                        utxo_id,
                        tx_pointer,
                        ..
                    })
                    | Input::CoinPredicate(CoinPredicate {
                        amount,
                        owner,
                        asset_id,
                        utxo_id,
                        tx_pointer,
                        ..
                    }) = input
                    {
                        Some(CoinConfig {
                            tx_id: *utxo_id.tx_id(),
                            output_index: utxo_id.output_index(),
                            tx_pointer_block_height: tx_pointer.block_height(),
                            tx_pointer_tx_idx: tx_pointer.tx_index(),
                            owner: *owner,
                            amount: *amount,
                            asset_id: *asset_id,
                        })
                    } else {
                        None
                    }
                }),
        );
        // disable automated block production
        test_builder.trigger = Trigger::Never;
        test_builder.utxo_validation = true;
        test_builder.number_threads_pool_verif = number_of_cores;
        test_builder.gas_limit = Some(10_000_000_000);
        test_builder.block_size_limit = Some(1_000_000_000_000);
        test_builder.max_txs = transactions.len();
        #[cfg(feature = "parallel-executor")]
        {
            test_builder.executor_number_of_cores = number_of_cores;
        }

        // spin up node
        let _ = rt.block_on(async move {
            // start the producer node
            let TestContext { srv, client, .. } = test_builder.finalize().await;

            // insert all transactions
            for tx in transactions {
                srv.shared.txpool_shared_state.insert(tx).await.unwrap();
            }
            let _ = client.produce_blocks(1, None).await;

            // sanity check block to ensure the transactions were actually processed
            let block = srv
                .shared
                .database
                .on_chain()
                .latest_view()
                .unwrap()
                .get_sealed_block_by_height(&1.into())
                .unwrap()
                .unwrap();
            assert_eq!(block.entity.transactions().len(), (n + 1) as usize);
            block
        });
    }
}

fuel_core_trace::enable_tracing!();
