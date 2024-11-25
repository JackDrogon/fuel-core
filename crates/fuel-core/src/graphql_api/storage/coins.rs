use fuel_core_storage::{
    blueprint::plain::Plain,
    codec::{
        postcard::Postcard,
        primitive::{
            utxo_id_to_bytes,
            Primitive,
        },
        raw::Raw,
    },
    structured_storage::TableWithBlueprint,
    Mappable,
};
use fuel_core_types::{
    entities::{
        coins::coin::Coin,
        Message,
    },
    fuel_tx::{
        Address,
        AssetId,
        TxId,
        UtxoId,
    },
    fuel_types::Nonce,
};

use crate::graphql_api::indexation;

use self::indexation::coins_to_spend::{
    IndexedCoinType,
    NON_RETRYABLE_BYTE,
    RETRYABLE_BYTE,
};

use super::balances::ItemAmount;

// TODO: Reuse `fuel_vm::storage::double_key` macro.
pub fn owner_coin_id_key(owner: &Address, coin_id: &UtxoId) -> OwnedCoinKey {
    let mut default = [0u8; Address::LEN + TxId::LEN + 2];
    default[0..Address::LEN].copy_from_slice(owner.as_ref());
    let utxo_id_bytes: [u8; TxId::LEN + 2] = utxo_id_to_bytes(coin_id);
    default[Address::LEN..].copy_from_slice(utxo_id_bytes.as_ref());
    default
}

/// The storage table for the index of coins to spend.

// In the implementation of getters we use the explicit panic with the message (`expect`)
// when the key is malformed (incorrect length). This is a bit of a code smell, but it's
// consistent with how the `double_key!` macro works. We should consider refactoring this
// in the future.
pub struct CoinsToSpendIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoinsToSpendIndexKey([u8; CoinsToSpendIndexKey::LEN]);

impl Default for CoinsToSpendIndexKey {
    fn default() -> Self {
        Self([0u8; CoinsToSpendIndexKey::LEN])
    }
}

impl CoinsToSpendIndexKey {
    const LEN: usize = Address::LEN
        + AssetId::LEN
        + u8::BITS as usize / 8
        + u64::BITS as usize / 8
        + TxId::LEN
        + 2;

    pub fn from_coin(coin: &Coin) -> Self {
        let address_bytes = coin.owner.as_ref();
        let asset_id_bytes = coin.asset_id.as_ref();
        let amount_bytes = coin.amount.to_be_bytes();
        let utxo_id_bytes = utxo_id_to_bytes(&coin.utxo_id);

        let mut arr = [0; CoinsToSpendIndexKey::LEN];
        let mut offset = 0;
        arr[offset..offset + Address::LEN].copy_from_slice(address_bytes);
        offset += Address::LEN;
        arr[offset..offset + AssetId::LEN].copy_from_slice(asset_id_bytes);
        offset += AssetId::LEN;
        arr[offset..offset + u8::BITS as usize / 8].copy_from_slice(&NON_RETRYABLE_BYTE);
        offset += u8::BITS as usize / 8;
        arr[offset..offset + u64::BITS as usize / 8].copy_from_slice(&amount_bytes);
        offset += u64::BITS as usize / 8;
        arr[offset..].copy_from_slice(&utxo_id_bytes);
        Self(arr)
    }

    pub fn from_message(message: &Message, base_asset_id: &AssetId) -> Self {
        let address_bytes = message.recipient().as_ref();
        let asset_id_bytes = base_asset_id.as_ref();
        let amount_bytes = message.amount().to_be_bytes();
        let nonce_bytes = message.nonce().as_slice();

        let mut arr = [0; CoinsToSpendIndexKey::LEN];
        let mut offset = 0;
        arr[offset..offset + Address::LEN].copy_from_slice(address_bytes);
        offset += Address::LEN;
        arr[offset..offset + AssetId::LEN].copy_from_slice(&asset_id_bytes);
        offset += AssetId::LEN;
        arr[offset..offset + u8::BITS as usize / 8].copy_from_slice(
            if message.has_retryable_amount() {
                &RETRYABLE_BYTE
            } else {
                &NON_RETRYABLE_BYTE
            },
        );
        offset += u8::BITS as usize / 8;
        arr[offset..offset + u64::BITS as usize / 8].copy_from_slice(&amount_bytes);
        offset += u64::BITS as usize / 8;
        arr[offset..offset + Nonce::LEN].copy_from_slice(&nonce_bytes);
        offset += Nonce::LEN;
        arr[offset..].copy_from_slice(&indexation::coins_to_spend::MESSAGE_PADDING_BYTES);
        Self(arr)
    }

    pub fn from_slice(slice: &[u8]) -> Result<Self, core::array::TryFromSliceError> {
        Ok(Self(slice.try_into()?))
    }

    pub fn owner(&self) -> Address {
        let address_start = 0;
        let address_end = address_start + Address::LEN;
        let address: [u8; Address::LEN] = self.0[address_start..address_end]
            .try_into()
            .expect("should have correct bytes");
        Address::new(address)
    }

    pub fn asset_id(&self) -> AssetId {
        let offset = Address::LEN;

        let asset_id_start = offset;
        let asset_id_end = asset_id_start + AssetId::LEN;
        let asset_id: [u8; AssetId::LEN] = self.0[asset_id_start..asset_id_end]
            .try_into()
            .expect("should have correct bytes");
        AssetId::new(asset_id)
    }

    pub fn retryable_flag(&self) -> u8 {
        let mut offset = Address::LEN + AssetId::LEN;
        self.0[offset]
    }

    // TODO[RC]: Use `ItemAmount` consistently
    pub fn amount(&self) -> ItemAmount {
        let mut offset = Address::LEN + AssetId::LEN + u8::BITS as usize / 8;
        let amount_start = offset;
        let amount_end = amount_start + u64::BITS as usize / 8;
        let amount = u64::from_be_bytes(
            self.0[amount_start..amount_end]
                .try_into()
                .expect("should have correct bytes"),
        );
        amount
    }

    pub fn foreign_key_bytes(
        &self,
    ) -> &[u8; CoinsToSpendIndexKey::LEN
            - Address::LEN
            - AssetId::LEN
            - u8::BITS as usize / 8
            - u64::BITS as usize / 8] {
        let mut offset =
            Address::LEN + AssetId::LEN + u8::BITS as usize / 8 + u64::BITS as usize / 8;
        self.0[offset..]
            .try_into()
            .expect("should have correct bytes")
    }

    // TODO[RC]: Test this
    pub fn utxo_id(&self) -> UtxoId {
        let mut offset = 0;
        offset += Address::LEN;
        offset += AssetId::LEN;
        offset += ItemAmount::BITS as usize / 8;

        let txid_start = 0 + offset;
        let txid_end = txid_start + TxId::LEN;

        let output_index_start = txid_end;

        let tx_id: [u8; TxId::LEN] = self.0[txid_start..txid_end]
            .try_into()
            .expect("TODO[RC]: Fix this");
        let output_index = u16::from_be_bytes(
            self.0[output_index_start..]
                .try_into()
                .expect("TODO[RC]: Fix this"),
        );
        UtxoId::new(TxId::from(tx_id), output_index)
    }
}

impl TryFrom<&[u8]> for CoinsToSpendIndexKey {
    type Error = core::array::TryFromSliceError;
    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        CoinsToSpendIndexKey::from_slice(slice)
    }
}

impl AsRef<[u8]> for CoinsToSpendIndexKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Mappable for CoinsToSpendIndex {
    type Key = Self::OwnedKey;
    type OwnedKey = CoinsToSpendIndexKey;
    type Value = Self::OwnedValue;
    type OwnedValue = u8;
}

impl TableWithBlueprint for CoinsToSpendIndex {
    type Blueprint = Plain<Raw, Primitive<1>>;
    type Column = super::Column;

    fn column() -> Self::Column {
        Self::Column::CoinsToSpend
    }
}

/// The storage table of owned coin ids. Maps addresses to owned coins.
pub struct OwnedCoins;
/// The storage key for owned coins: `Address ++ UtxoId`
pub type OwnedCoinKey = [u8; Address::LEN + TxId::LEN + 2];

impl Mappable for OwnedCoins {
    type Key = Self::OwnedKey;
    type OwnedKey = OwnedCoinKey;
    type Value = Self::OwnedValue;
    type OwnedValue = ();
}

impl TableWithBlueprint for OwnedCoins {
    type Blueprint = Plain<Raw, Postcard>;
    type Column = super::Column;

    fn column() -> Self::Column {
        Self::Column::OwnedCoins
    }
}

#[cfg(test)]
mod test {
    use fuel_core_types::{
        entities::relayer::message::MessageV1,
        fuel_tx::MessageId,
        fuel_types::Nonce,
    };

    use super::*;

    impl rand::distributions::Distribution<CoinsToSpendIndexKey>
        for rand::distributions::Standard
    {
        fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> CoinsToSpendIndexKey {
            let mut bytes = [0u8; CoinsToSpendIndexKey::LEN];
            rng.fill_bytes(bytes.as_mut());
            CoinsToSpendIndexKey(bytes)
        }
    }

    fn generate_key(rng: &mut impl rand::Rng) -> <OwnedCoins as Mappable>::Key {
        let mut bytes = [0u8; 66];
        rng.fill(bytes.as_mut());
        bytes
    }

    fuel_core_storage::basic_storage_tests!(
        OwnedCoins,
        [0u8; 66],
        <OwnedCoins as Mappable>::Value::default(),
        <OwnedCoins as Mappable>::Value::default(),
        generate_key
    );

    fuel_core_storage::basic_storage_tests!(
        CoinsToSpendIndex,
        <CoinsToSpendIndex as Mappable>::Key::default(),
        <CoinsToSpendIndex as Mappable>::Value::default()
    );

    fn merge_foreign_key_bytes<A, B, const N: usize>(a: A, b: B) -> [u8; N]
    where
        A: AsRef<[u8]>,
        B: AsRef<[u8]>,
    {
        a.as_ref()
            .into_iter()
            .copied()
            .chain(b.as_ref().into_iter().copied())
            .collect::<Vec<_>>()
            .try_into()
            .expect("should have correct length")
    }

    #[test]
    fn key_from_coin() {
        let owner = Address::new([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
            0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
            0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        ]);

        let asset_id = AssetId::new([
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C,
            0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
        ]);

        let retryable_flag = NON_RETRYABLE_BYTE;

        let amount = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        assert_eq!(amount.len(), u64::BITS as usize / 8);

        let tx_id = TxId::new([
            0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C,
            0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
            0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
        ]);

        let output_index = [0xFE, 0xFF];
        let utxo_id = UtxoId::new(tx_id, u16::from_be_bytes(output_index));

        let coin = Coin {
            owner,
            asset_id,
            amount: u64::from_be_bytes(amount),
            utxo_id,
            tx_pointer: Default::default(),
        };

        let key = CoinsToSpendIndexKey::from_coin(&coin);

        let key_bytes: [u8; CoinsToSpendIndexKey::LEN] =
            key.as_ref().try_into().expect("should have correct length");

        assert_eq!(
            key_bytes,
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23,
                0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
                0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B,
                0x3C, 0x3D, 0x3E, 0x3F, 0x01, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
                0x47, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A,
                0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66,
                0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0xFE, 0xFF,
            ]
        );

        assert_eq!(key.owner(), owner);
        assert_eq!(key.asset_id(), asset_id);
        assert_eq!(key.retryable_flag(), retryable_flag[0]);
        assert_eq!(key.amount(), u64::from_be_bytes(amount));
        assert_eq!(
            key.foreign_key_bytes(),
            &merge_foreign_key_bytes(tx_id, output_index)
        );
    }

    #[test]
    fn key_from_non_retryable_message() {
        let owner = Address::new([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
            0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
            0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        ]);

        let base_asset_id = AssetId::new([
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C,
            0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
        ]);

        let amount = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        assert_eq!(amount.len(), u64::BITS as usize / 8);

        let retryable_flag = NON_RETRYABLE_BYTE;

        let nonce = Nonce::new([
            0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C,
            0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
            0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
        ]);

        let trailing_bytes = indexation::coins_to_spend::MESSAGE_PADDING_BYTES;

        let message = Message::V1(MessageV1 {
            recipient: owner,
            amount: u64::from_be_bytes(amount),
            nonce,
            sender: Default::default(),
            data: vec![],
            da_height: Default::default(),
        });

        let key = CoinsToSpendIndexKey::from_message(&message, &base_asset_id);

        let key_bytes: [u8; CoinsToSpendIndexKey::LEN] =
            key.as_ref().try_into().expect("should have correct length");

        assert_eq!(
            key_bytes,
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23,
                0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
                0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B,
                0x3C, 0x3D, 0x3E, 0x3F, 0x01, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
                0x47, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A,
                0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66,
                0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0xFF, 0xFF,
            ]
        );

        assert_eq!(key.owner(), owner);
        assert_eq!(key.asset_id(), base_asset_id);
        assert_eq!(key.retryable_flag(), retryable_flag[0]);
        assert_eq!(key.amount(), u64::from_be_bytes(amount));
        assert_eq!(
            key.foreign_key_bytes(),
            &merge_foreign_key_bytes(nonce, trailing_bytes)
        );
    }

    #[test]
    fn key_from_retryable_message() {
        let owner = Address::new([
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C,
            0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19,
            0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F,
        ]);

        let base_asset_id = AssetId::new([
            0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C,
            0x2D, 0x2E, 0x2F, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39,
            0x3A, 0x3B, 0x3C, 0x3D, 0x3E, 0x3F,
        ]);

        let amount = [0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47];
        assert_eq!(amount.len(), u64::BITS as usize / 8);

        let retryable_flag = RETRYABLE_BYTE;

        let nonce = Nonce::new([
            0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5C,
            0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69,
            0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F,
        ]);

        let trailing_bytes = indexation::coins_to_spend::MESSAGE_PADDING_BYTES;

        let message = Message::V1(MessageV1 {
            recipient: owner,
            amount: u64::from_be_bytes(amount),
            nonce,
            sender: Default::default(),
            data: vec![1],
            da_height: Default::default(),
        });

        let key = CoinsToSpendIndexKey::from_message(&message, &base_asset_id);

        let key_bytes: [u8; CoinsToSpendIndexKey::LEN] =
            key.as_ref().try_into().expect("should have correct length");

        assert_eq!(
            key_bytes,
            [
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x20, 0x21, 0x22, 0x23,
                0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2A, 0x2B, 0x2C, 0x2D, 0x2E, 0x2F,
                0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3A, 0x3B,
                0x3C, 0x3D, 0x3E, 0x3F, 0x00, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
                0x47, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A,
                0x5B, 0x5C, 0x5D, 0x5E, 0x5F, 0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66,
                0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0xFF, 0xFF,
            ]
        );

        assert_eq!(key.owner(), owner);
        assert_eq!(key.asset_id(), base_asset_id);
        assert_eq!(key.retryable_flag(), retryable_flag[0]);
        assert_eq!(key.amount(), u64::from_be_bytes(amount));
        assert_eq!(
            key.foreign_key_bytes(),
            &merge_foreign_key_bytes(nonce, trailing_bytes)
        );
    }
}
