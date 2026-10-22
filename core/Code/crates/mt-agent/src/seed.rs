//! The agent's own seed (Montana App, «An agent»: its own seed, born on its machine): the branches a
//! phone derives from the words, through the very derivation of the core, so every correspondent
//! meets the agent under the names a phone of the same words would stand under.

use mt_mnemonic::{
    mldsa_seed_for_role, mnemonic_to_master_seed, sha256_raw, MnemonicError, MASTER_SEED_LEN,
    MLDSA_SEED_LEN,
};
use zeroize::Zeroizing;

use crate::letter::LETTER_AUTHOR_DOMAIN;
use crate::wire::hex_lower;

/// The owner branch (MTPipe.ownerSecret): one value on every machine of one person.
pub const OWNER_ROLE: &[u8] = b"mt-owner-key";
pub const SECRET_LEN: usize = 32;
pub const REFERENCE_LEN: usize = 16;

pub struct AgentSeed {
    owner: Zeroizing<[u8; MLDSA_SEED_LEN]>,
    author: Zeroizing<[u8; MLDSA_SEED_LEN]>,
}

impl AgentSeed {
    pub fn from_words(words: &str) -> Result<Self, MnemonicError> {
        let master = Zeroizing::new(mnemonic_to_master_seed(words)?);
        Ok(Self::from_master(&master))
    }

    pub fn from_master(master: &[u8; MASTER_SEED_LEN]) -> Self {
        Self {
            owner: Zeroizing::new(mldsa_seed_for_role(master, OWNER_ROLE)),
            author: Zeroizing::new(mldsa_seed_for_role(master, LETTER_AUTHOR_DOMAIN)),
        }
    }

    pub fn owner_secret(&self) -> &[u8; MLDSA_SEED_LEN] {
        &self.owner
    }

    /// The key of the author field (MontanaQueueKeys.authorKey): the branch is named by the same
    /// bytes as the tag's domain, as the phone names it.
    pub fn author_key(&self) -> &[u8; MLDSA_SEED_LEN] {
        &self.author
    }

    /// The twin reference (MontanaSeed.twin): the owner branch filed as a correspondence, the name
    /// the agent's own tag at a door is made from.
    pub fn twin(&self) -> String {
        hex_lower(&sha256_raw(&self.owner[..])[..REFERENCE_LEN])
    }
}

/// The filing name of a correspondence (MTPipe.reference): the first sixteen bytes of SHA-256 of
/// its secret; it never leaves the machine.
pub fn reference(secret: &[u8]) -> Option<String> {
    if secret.len() != SECRET_LEN {
        return None;
    }
    Some(hex_lower(&sha256_raw(secret)[..REFERENCE_LEN]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master() -> [u8; MASTER_SEED_LEN] {
        let mut m = [0u8; MASTER_SEED_LEN];
        for (i, b) in m.iter_mut().enumerate() {
            *b = i as u8;
        }
        m
    }

    // Derived by Python hmac (RFC 5869 Expand) apart from the core and the phone, over sixty-four
    // distinct bytes: rejects an implementation without the block counter (d2c4bfaa...), one that
    // reverses the master seed (05fe3b44...) and one that swaps the two roles.
    #[test]
    fn branches_are_the_phones() {
        let s = AgentSeed::from_master(&master());
        assert_eq!(
            hex::encode(s.owner_secret()),
            "91fb9cb924a3f4ede49e9276633b70df3b5eef3789bce46a204e578013376620"
        );
        assert_eq!(
            hex::encode(s.author_key()),
            "3e7d6713f7426475ceb6a5640fcd8f27ee12be42ee770d9d437c794acd7618fd"
        );
    }

    // Rejects a reference cut from the tail of the hash (9a94dd83...) and one made of the master.
    #[test]
    fn the_twin_is_the_owner_filed() {
        let s = AgentSeed::from_master(&master());
        assert_eq!(s.twin(), "03f226159714a79644a7c9b63f4b1788");
        assert_eq!(reference(&s.owner_secret()[..]), Some(s.twin()));
        let distinct: Vec<u8> = (0u8..32).collect();
        assert_eq!(
            reference(&distinct).as_deref(),
            Some("630dcd2966c4336691125448bbb25b4f")
        );
        assert_eq!(reference(&distinct[..31]), None);
        assert_eq!(reference(&master()), None);
    }

    #[test]
    fn words_that_are_no_phrase_make_no_seed() {
        assert!(AgentSeed::from_words("not a phrase").is_err());
    }
}
