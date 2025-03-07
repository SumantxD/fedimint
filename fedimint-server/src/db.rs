use std::collections::BTreeSet;
use std::fmt::Debug;

use fedimint_core::api::ClientConfigDownloadToken;
use fedimint_core::db::{DatabaseVersion, MigrationMap, MODULE_GLOBAL_PREFIX};
use fedimint_core::encoding::{Decodable, Encodable};
use fedimint_core::epoch::{SerdeSignature, SignedEpochOutcome};
use fedimint_core::{impl_db_lookup, impl_db_record, PeerId, TransactionId};
use serde::Serialize;
use strum_macros::EnumIter;

use crate::consensus::AcceptedTransaction;

pub const GLOBAL_DATABASE_VERSION: DatabaseVersion = DatabaseVersion(0);

#[repr(u8)]
#[derive(Clone, EnumIter, Debug)]
pub enum DbKeyPrefix {
    AcceptedTransaction = 0x02,
    DropPeer = 0x03,
    RejectedTransaction = 0x04,
    EpochHistory = 0x05,
    LastEpoch = 0x06,
    ClientConfigSignature = 0x07,
    ConsensusUpgrade = 0x08,
    ClientConfigDownload = 0x09,
    Module = MODULE_GLOBAL_PREFIX,
}

impl std::fmt::Display for DbKeyPrefix {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct AcceptedTransactionKey(pub TransactionId);

#[derive(Debug, Encodable, Decodable)]
pub struct AcceptedTransactionKeyPrefix;

impl_db_record!(
    key = AcceptedTransactionKey,
    value = AcceptedTransaction,
    db_prefix = DbKeyPrefix::AcceptedTransaction,
    notify_on_modify = true,
);
impl_db_lookup!(
    key = AcceptedTransactionKey,
    query_prefix = AcceptedTransactionKeyPrefix
);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct RejectedTransactionKey(pub TransactionId);

#[derive(Debug, Encodable, Decodable)]
pub struct RejectedTransactionKeyPrefix;

impl_db_record!(
    key = RejectedTransactionKey,
    value = String,
    db_prefix = DbKeyPrefix::RejectedTransaction,
    notify_on_modify = true,
);
impl_db_lookup!(
    key = RejectedTransactionKey,
    query_prefix = RejectedTransactionKeyPrefix
);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct DropPeerKey(pub PeerId);

#[derive(Debug, Encodable, Decodable)]
pub struct DropPeerKeyPrefix;

impl_db_record!(
    key = DropPeerKey,
    value = (),
    db_prefix = DbKeyPrefix::DropPeer,
);
impl_db_lookup!(key = DropPeerKey, query_prefix = DropPeerKeyPrefix);

#[derive(Debug, Copy, Clone, Encodable, Decodable, Serialize)]
pub struct EpochHistoryKey(pub u64);

#[derive(Debug, Encodable, Decodable)]
pub struct EpochHistoryKeyPrefix;

impl_db_record!(
    key = EpochHistoryKey,
    value = SignedEpochOutcome,
    db_prefix = DbKeyPrefix::EpochHistory,
);
impl_db_lookup!(key = EpochHistoryKey, query_prefix = EpochHistoryKeyPrefix);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct LastEpochKey;

impl_db_record!(
    key = LastEpochKey,
    value = EpochHistoryKey,
    db_prefix = DbKeyPrefix::LastEpoch
);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct ClientConfigSignatureKey;

#[derive(Debug, Encodable, Decodable)]
pub struct ClientConfigSignatureKeyPrefix;

impl_db_record!(
    key = ClientConfigSignatureKey,
    value = SerdeSignature,
    db_prefix = DbKeyPrefix::ClientConfigSignature,
);
impl_db_lookup!(
    key = ClientConfigSignatureKey,
    query_prefix = ClientConfigSignatureKeyPrefix
);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct ConsensusUpgradeKey;

impl_db_record!(
    key = ConsensusUpgradeKey,
    value = BTreeSet<PeerId>,
    db_prefix = DbKeyPrefix::ConsensusUpgrade,
);

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct ClientConfigDownloadKeyPrefix;

#[derive(Debug, Encodable, Decodable, Serialize)]
pub struct ClientConfigDownloadKey(pub ClientConfigDownloadToken);

impl_db_record!(
    key = ClientConfigDownloadKey,
    value = u64,
    db_prefix = DbKeyPrefix::ClientConfigDownload
);
impl_db_lookup!(
    key = ClientConfigDownloadKey,
    query_prefix = ClientConfigDownloadKeyPrefix
);

pub fn get_global_database_migrations<'a>() -> MigrationMap<'a> {
    MigrationMap::new()
}

#[cfg(test)]
mod fedimint_migration_tests {
    use std::collections::BTreeSet;

    use bitcoin::{secp256k1, KeyPair};
    use bitcoin_hashes::Hash;
    use fedimint_core::api::ClientConfigDownloadToken;
    use fedimint_core::core::DynInput;
    use fedimint_core::db::{apply_migrations, DatabaseTransaction};
    use fedimint_core::epoch::{
        ConsensusItem, ConsensusUpgrade, EpochOutcome, SerdeSignature, SerdeSignatureShare,
        SignedEpochOutcome,
    };
    use fedimint_core::module::registry::ModuleDecoderRegistry;
    use fedimint_core::transaction::Transaction;
    use fedimint_core::{PeerId, ServerModule, TransactionId};
    use fedimint_dummy_common::{DummyInput, DummyOutput};
    use fedimint_dummy_server::Dummy;
    use fedimint_testing::{prepare_snapshot, validate_migrations, BYTE_32, BYTE_8};
    use futures::StreamExt;
    use rand::distributions::{Distribution, Standard};
    use rand::rngs::OsRng;
    use rand::Rng;
    use secp256k1_zkp::Message;
    use strum::IntoEnumIterator;
    use threshold_crypto::SignatureShare;

    use super::{
        AcceptedTransactionKey, ClientConfigSignatureKey, ConsensusUpgradeKey, DropPeerKey,
        EpochHistoryKey, LastEpochKey, RejectedTransactionKey,
    };
    use crate::consensus::AcceptedTransaction;
    use crate::core::DynOutput;
    use crate::db::{
        get_global_database_migrations, AcceptedTransactionKeyPrefix, ClientConfigDownloadKey,
        ClientConfigDownloadKeyPrefix, ClientConfigSignatureKeyPrefix, DbKeyPrefix,
        DropPeerKeyPrefix, EpochHistoryKeyPrefix, RejectedTransactionKeyPrefix,
        GLOBAL_DATABASE_VERSION,
    };

    /// Create a database with version 0 data. The database produced is not
    /// intended to be real data or semantically correct. It is only
    /// intended to provide coverage when reading the database
    /// in future code versions. This function should not be updated when
    /// database keys/values change - instead a new function should be added
    /// that creates a new database backup that can be tested.
    async fn create_db_with_v0_data(mut dbtx: DatabaseTransaction<'_>) {
        let accepted_tx_id = AcceptedTransactionKey(TransactionId::from_slice(&BYTE_32).unwrap());

        let (sk, _) = secp256k1::generate_keypair(&mut OsRng);
        let secp = secp256k1::Secp256k1::new();
        let key_pair = KeyPair::from_secret_key(&secp, &sk);
        let schnorr = secp.sign_schnorr(&Message::from_slice(&BYTE_32).unwrap(), &key_pair);
        let transaction = Transaction {
            inputs: vec![DynInput::from_typed(0, DummyInput)],
            outputs: vec![DynOutput::from_typed(0, DummyOutput)],
            signature: Some(schnorr),
        };

        let accepted_tx = AcceptedTransaction {
            epoch: 6,
            transaction: transaction.clone(),
        };
        dbtx.insert_new_entry(&accepted_tx_id, &accepted_tx).await;

        dbtx.insert_new_entry(
            &RejectedTransactionKey(TransactionId::from_slice(&BYTE_32).unwrap()),
            &"Test Rejection Reason".to_string(),
        )
        .await;
        dbtx.insert_new_entry(&DropPeerKey(1.into()), &()).await;

        let epoch_history_key = EpochHistoryKey(6);

        let sig_share = SignatureShare(Standard.sample(&mut OsRng));
        let consensus_items = vec![
            ConsensusItem::ConsensusUpgrade(ConsensusUpgrade),
            ConsensusItem::ClientConfigSignatureShare(SerdeSignatureShare(sig_share.clone())),
            ConsensusItem::EpochOutcomeSignatureShare(SerdeSignatureShare(sig_share)),
            ConsensusItem::Transaction(transaction),
        ];

        let epoch_outcome = EpochOutcome {
            epoch: 6,
            last_hash: Some(secp256k1::hashes::sha256::Hash::hash(&BYTE_8)),
            items: vec![(0.into(), consensus_items)],
            rejected_txs: BTreeSet::new(),
        };

        let signed_epoch_outcome = SignedEpochOutcome {
            outcome: epoch_outcome,
            hash: secp256k1::hashes::sha256::Hash::hash(&BYTE_8),
            signature: Some(SerdeSignature(Standard.sample(&mut OsRng))),
        };
        dbtx.insert_new_entry(&epoch_history_key, &signed_epoch_outcome)
            .await;

        dbtx.insert_new_entry(&LastEpochKey, &epoch_history_key)
            .await;

        let serde_sig = SerdeSignature(Standard.sample(&mut OsRng));
        dbtx.insert_new_entry(&ClientConfigSignatureKey, &serde_sig)
            .await;

        dbtx.insert_new_entry(
            &ClientConfigDownloadKey(ClientConfigDownloadToken(OsRng::default().gen())),
            &0,
        )
        .await;

        let mut peers: BTreeSet<PeerId> = BTreeSet::new();
        peers.insert(0.into());
        peers.insert(1.into());
        peers.insert(2.into());
        dbtx.insert_new_entry(&ConsensusUpgradeKey, &peers).await;
        dbtx.commit_tx().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepare_migration_snapshots() {
        prepare_snapshot(
            "global-v0",
            |dbtx| {
                Box::pin(async move {
                    create_db_with_v0_data(dbtx).await;
                })
            },
            ModuleDecoderRegistry::from_iter([(0, <Dummy as ServerModule>::decoder())]),
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_migrations() {
        validate_migrations(
            "global",
            |db| async move {
                apply_migrations(
                    &db,
                    "Global".to_string(),
                    GLOBAL_DATABASE_VERSION,
                    get_global_database_migrations(),
                )
                .await
                .expect("Error applying migrations to temp database");

                // Verify that all of the data from the global namespace can be read. If a
                // database migration failed or was not properly supplied,
                // the struct will fail to be read.
                let mut dbtx = db.begin_transaction().await;

                for prefix in DbKeyPrefix::iter() {
                    match prefix {
                        DbKeyPrefix::AcceptedTransaction => {
                                let accepted_transactions = dbtx
                                    .find_by_prefix(&AcceptedTransactionKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let num_accepted_transactions = accepted_transactions.len();
                                assert!(
                                    num_accepted_transactions > 0,
                                    "validate_migrations was not able to read any AcceptedTransactions"
                                );
                            }
                            DbKeyPrefix::DropPeer => {
                                let dropped_peers = dbtx
                                    .find_by_prefix(&DropPeerKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let num_dropped_peers = dropped_peers.len();
                                assert!(
                                    num_dropped_peers > 0,
                                    "validate_migrations was not able to read any DroppedPeers"
                                );
                            }
                            DbKeyPrefix::RejectedTransaction => {
                                let rejected_transactions = dbtx
                                    .find_by_prefix(&RejectedTransactionKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let num_rejected_transactions = rejected_transactions.len();
                                assert!(
                                    num_rejected_transactions > 0,
                                    "validate_migrations was not able to read any RejectedTransactions"
                                );
                            }
                            DbKeyPrefix::EpochHistory => {
                                let epoch_history = dbtx
                                    .find_by_prefix(&EpochHistoryKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let num_epochs = epoch_history.len();
                                assert!(
                                    num_epochs > 0,
                                    "validate_migrations was not able to read any EpochHistory"
                                );
                            }
                            DbKeyPrefix::LastEpoch => {
                                assert!(dbtx.get_value(&LastEpochKey).await.is_some());
                            }
                            DbKeyPrefix::ClientConfigSignature => {
                                let client_config_sigs = dbtx
                                    .find_by_prefix(&ClientConfigSignatureKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let num_sigs = client_config_sigs.len();
                                assert!(
                                    num_sigs > 0,
                                    "validate_migrations was not able to read any ClientConfigSignatures"
                                );
                            }
                            DbKeyPrefix::ConsensusUpgrade => {
                                assert!(dbtx.get_value(&ConsensusUpgradeKey).await.is_some());
                            }
                            DbKeyPrefix::ClientConfigDownload => {
                                let downloads = dbtx
                                    .find_by_prefix(&ClientConfigDownloadKeyPrefix)
                                    .await
                                    .collect::<Vec<_>>()
                                    .await;
                                let downloads_len = downloads.len();
                                assert!(
                                    downloads_len > 0,
                                    "validate_migrations was not able to read any ClientConfigDownloadKey"
                                );
                            }
                            // Module prefix is reserved for modules, no migration testing is needed
                            DbKeyPrefix::Module => {}
                    }
                }
            },
            ModuleDecoderRegistry::from_iter([(
                0,
                <Dummy as ServerModule>::decoder(),
            )]),
        )
        .await;
    }
}
