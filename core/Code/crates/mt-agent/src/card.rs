//! The card an agent hands out and the card it meets (MontanaCard, MontanaFirstContact): one
//! encapsulation key behind a link of thirty-two random bytes, its body sealed on a node under a key
//! the invitation alone derives, and the first letter that carries the encapsulation into the card's
//! own box. Accepting is proving: ML-KEM decapsulation never refuses (implicit rejection, FIPS 203),
//! so a pipe is born only from a letter whose confirmation the true secret alone makes.

use mt_crypto::{
    keypair_from_seed_mlkem, mlkem_decapsulate, mlkem_encapsulate, MlkemCiphertext, MlkemPublicKey,
    MlkemSecretKey, MLKEM_CIPHERTEXT_SIZE, MLKEM_PUBLIC_KEY_SIZE, MLKEM_SEED_SIZE,
};
use mt_messenger_e2e::media::{open_blob, seal_blob};
use mt_mnemonic::sha256_raw;
use mt_names::first_secret;
use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::door::BoxRow;
use crate::letter::{base64_standard, base64_standard_decode, letter_fields};
use crate::seed::reference;
use crate::wire::{
    box_days, day_window, hex_lower, minute_window, open_boxed, seal_letter, sealed_second,
    sub_tag, NONCE_LEN,
};

pub const RDV_KEY_DOMAIN: &[u8] = b"mt-rdv-k";
pub const RDV_BLOB_DOMAIN: &[u8] = b"mt-rdv-a";
pub const RDV_PERM_DOMAIN: &[u8] = b"mt-rdv-p";
pub const RDV_LETTER_DOMAIN: &[u8] = b"mt-rdv-letter";
pub const RDV_WAKE_DOMAIN: &[u8] = b"mt-rdv-wake";
pub const FIRST_CONF_DOMAIN: &[u8] = b"mt-first-conf";
/// The bodies of the permanent card's mark and of a spent card's tomb, sealed under the card's key.
pub const PERM_MAGIC: &[u8] = b"mt-rdv-perm";
pub const SPENT_MAGIC: &[u8] = b"mt-rdv-spent";
pub const PERM_PREFIX: &str = "https://montana.quest/perp/";
pub const BLOB_PUT_PATH: &str = "/blob-put";
pub const BLOB_GET_PATH: &str = "/blob-get";
pub const INVITE_LEN: usize = 32;
pub const NAME_LIMIT: usize = 64;
pub const CONF_LEN: usize = 16;
/// The first letter rides one size whatever it carries: the encapsulation, five fields, the padding.
pub const FIRST_ENVELOPE_SIZE: usize = 1536;
/// The phone's ear takes a first letter only longer than the encapsulation and four bytes.
const FIRST_LETTER_FLOOR: usize = MLKEM_CIPHERTEXT_SIZE + 4;

/// Every written form a phone reads an invitation from (MontanaCard.readPrefixes).
const LINK_HOSTS: [&str; 3] = ["montana.quest", "montana.xxx", "www.montana.xxx"];
const LINK_KINDS: [&str; 3] = ["r", "perp", "temp"];
const SHORT_PREFIX: &str = "montana://r/";

pub type Invite = [u8; INVITE_LEN];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardError {
    Crypto,
    /// The row lies under no tag of this card.
    NotThisCard,
    /// The row stays sealed under the invitation's letter key a day back from the node's minute.
    Sealed,
    /// The plain is no first letter: too short, or without a name and words.
    NotFirstLetter,
    /// The secret decapsulated does not make the letter's confirmation: an encapsulation to another
    /// key, a broken one, or a letter without a confirmation at all.
    Ghost,
}

fn domained(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = Vec::with_capacity(domain.len() + 1 + parts.iter().map(|p| p.len()).sum::<usize>());
    m.extend_from_slice(domain);
    m.push(0);
    for p in parts {
        m.extend_from_slice(p);
    }
    sha256_raw(&m)
}

/// The key the card's body, its permanent mark and its tomb are sealed under.
pub fn rdv_key(invite: &Invite) -> [u8; 32] {
    domained(RDV_KEY_DOMAIN, &[&invite[..]])
}

pub fn rdv_bid(invite: &Invite) -> String {
    hex_lower(&domained(RDV_BLOB_DOMAIN, &[&invite[..]]))
}

pub fn perm_bid(invite: &Invite) -> String {
    hex_lower(&domained(RDV_PERM_DOMAIN, &[&invite[..]]))
}

/// The secret of the first letter: exactly the card's owner and whoever holds the link derive it.
pub fn rdv_letter_secret(invite: &Invite) -> [u8; 32] {
    domained(RDV_LETTER_DOMAIN, &[&invite[..]])
}

/// The card's tag of a day at a door (MTNodeWire.rdvConvW).
pub fn rdv_conv(invite: &Invite, day: u64) -> String {
    hex_lower(&domained(
        RDV_WAKE_DOMAIN,
        &[&invite[..], &day.to_le_bytes()[..]],
    ))
}

pub fn first_confirm(secret: &[u8], ct: &[u8]) -> String {
    base64url_no_pad(&domained(FIRST_CONF_DOMAIN, &[secret, ct])[..CONF_LEN])
}

/// Constant time: an early exit is a timing oracle over the secret. Empty never matches.
pub fn same_confirm(a: &str, b: &str) -> bool {
    if a.len() != b.len() || a.is_empty() {
        return false;
    }
    let mut d = 0u8;
    for (x, y) in a.bytes().zip(b.bytes()) {
        d |= x ^ y;
    }
    d == 0
}

pub fn base64url_no_pad(bytes: &[u8]) -> String {
    base64_standard(bytes)
        .chars()
        .filter(|c| *c != '=')
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect()
}

/// As the phone reads it (Data(base64urlNoPad:)): the URL alphabet mapped back, the padding
/// restored, then the strict standard decoding.
pub fn base64url_no_pad_decode(s: &str) -> Option<Vec<u8>> {
    let mut b: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();
    while b.len() % 4 != 0 {
        b.push('=');
    }
    base64_standard_decode(&b)
}

/// The invitation a link names, read from every form a phone writes and reads.
pub fn invite_in_link(text: &str) -> Option<Invite> {
    let t = text.trim();
    let code = LINK_HOSTS
        .iter()
        .flat_map(|h| LINK_KINDS.iter().map(move |k| format!("https://{h}/{k}/")))
        .chain(core::iter::once(SHORT_PREFIX.to_owned()))
        .find_map(|p| t.strip_prefix(p.as_str()).map(str::to_owned))?;
    base64url_no_pad_decode(&code)?.try_into().ok()
}

/// The key and, when the name is not empty and fits the limit, the name (MontanaCard.offer): a
/// name past the limit is left out, never cut.
pub fn card_payload(root: &[u8; MLKEM_PUBLIC_KEY_SIZE], name: &str) -> Vec<u8> {
    let mut p = root.to_vec();
    let n = name.trim();
    if !n.is_empty() && n.len() <= NAME_LIMIT {
        p.extend_from_slice(n.as_bytes());
    }
    p
}

/// A card put on the node again keeps its blob name and takes a fresh term.
pub fn blob_put_body(bid: &str, sealed: &[u8]) -> Vec<u8> {
    json!({ "bid": bid, "data": base64_standard(sealed), "over": true })
        .to_string()
        .into_bytes()
}

pub fn blob_get_body(bid: &str) -> Vec<u8> {
    json!({ "bid": bid }).to_string().into_bytes()
}

/// The sealed bytes of a door's answer of 200; a 404 is the door's word that it holds none.
pub fn read_blob(answer: &[u8]) -> Option<Vec<u8>> {
    let v: Value = serde_json::from_slice(answer).ok()?;
    base64_standard_decode(v.get("data")?.as_str()?)
}

/// What a node holds under an invitation (MontanaCard.expand).
pub enum Unfolded {
    Card {
        root: Box<MlkemPublicKey>,
        name: String,
    },
    /// The card served its meeting: the link is dead for good, not unreachable.
    Spent,
}

pub fn unfold(invite: &Invite, sealed: &[u8]) -> Option<Unfolded> {
    let payload = open_blob(&rdv_key(invite), sealed)?;
    if payload == SPENT_MAGIC {
        return Some(Unfolded::Spent);
    }
    if !(MLKEM_PUBLIC_KEY_SIZE..=MLKEM_PUBLIC_KEY_SIZE + NAME_LIMIT).contains(&payload.len()) {
        return None;
    }
    let root = MlkemPublicKey::from_slice(&payload[..MLKEM_PUBLIC_KEY_SIZE])?;
    let name = core::str::from_utf8(&payload[MLKEM_PUBLIC_KEY_SIZE..])
        .map(|n| n.trim().to_owned())
        .unwrap_or_default();
    Some(Unfolded::Card {
        root: Box::new(root),
        name,
    })
}

/// Was the link a permanent one: its mark stands under the invitation's key.
pub fn is_permanent(invite: &Invite, sealed_mark: &[u8]) -> bool {
    open_blob(&rdv_key(invite), sealed_mark).is_some_and(|d| d == PERM_MAGIC)
}

/// The card the agent hands out. The seed and the invitation are the caller's draw, as a nonce is.
pub struct AgentCard {
    invite: Invite,
    root: MlkemPublicKey,
    sk: MlkemSecretKey,
}

/// A first letter the card proved: the pipe's secret and filing name, the five fields (mid, words,
/// name, glyph, confirmation), the second it was sealed and the door's name of the row.
pub struct Met {
    pub secret: Zeroizing<[u8; 32]>,
    pub reference: String,
    pub fields: Vec<String>,
    pub sealed_at: u64,
    pub node_mid: String,
}

impl AgentCard {
    pub fn new(seed: &[u8; MLKEM_SEED_SIZE], invite: Invite) -> Result<Self, CardError> {
        let (root, sk) = keypair_from_seed_mlkem(seed).map_err(|_| CardError::Crypto)?;
        Ok(Self { invite, root, sk })
    }

    pub fn invite(&self) -> &Invite {
        &self.invite
    }

    pub fn root(&self) -> &[u8; MLKEM_PUBLIC_KEY_SIZE] {
        self.root.as_bytes()
    }

    pub fn link(&self) -> String {
        format!("{PERM_PREFIX}{}", base64url_no_pad(&self.invite))
    }

    /// The card's body on the node under the invitation's blob name, sealed with a fresh nonce at
    /// every put and wearing the name of the moment it is put.
    pub fn blob_put_body(&self, name: &str, nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
        let sealed = seal_blob(
            &rdv_key(&self.invite),
            nonce,
            &card_payload(self.root(), name),
        );
        blob_put_body(&rdv_bid(&self.invite), &sealed)
    }

    pub fn perm_mark_body(&self, nonce: &[u8; NONCE_LEN]) -> Vec<u8> {
        let sealed = seal_blob(&rdv_key(&self.invite), nonce, PERM_MAGIC);
        blob_put_body(&perm_bid(&self.invite), &sealed)
    }

    /// The tags the card's box is read at (MontanaWakePush.BoxEar): the invitation's tag of each day
    /// of the box's term, and the agent's own tag beside it.
    pub fn labels(&self, twin: &str, unix_seconds: u64) -> Vec<(String, String)> {
        box_days(day_window(unix_seconds))
            .map(|day| {
                let conv = rdv_conv(&self.invite, day);
                let sid = sub_tag(&conv, twin);
                (conv, sid)
            })
            .collect()
    }

    pub fn accept(&self, row: &BoxRow) -> Result<Met, CardError> {
        if !box_days(day_window(row.at)).any(|d| rdv_conv(&self.invite, d) == row.conv) {
            return Err(CardError::NotThisCard);
        }
        let letter = Zeroizing::new(rdv_letter_secret(&self.invite));
        let (plain, back) =
            open_boxed(&row.sealed, &letter[..], row.at).ok_or(CardError::Sealed)?;
        let plain = Zeroizing::new(plain);
        if plain.len() <= FIRST_LETTER_FLOOR {
            return Err(CardError::NotFirstLetter);
        }
        let (ct, rest) = plain.split_at(MLKEM_CIPHERTEXT_SIZE);
        let mut fields = letter_fields(rest);
        fields.truncate(5);
        if fields[0].is_empty() || fields[1].is_empty() {
            return Err(CardError::NotFirstLetter);
        }
        let ct = MlkemCiphertext::from_slice(ct).ok_or(CardError::Crypto)?;
        let ss = mlkem_decapsulate(&self.sk, &ct).map_err(|_| CardError::Crypto)?;
        let secret = Zeroizing::new(first_secret(ss.as_bytes(), self.root(), ct.as_bytes()));
        if !same_confirm(&fields[4], &first_confirm(&secret[..], ct.as_bytes())) {
            return Err(CardError::Ghost);
        }
        let reference = reference(&secret[..]).ok_or(CardError::Crypto)?;
        Ok(Met {
            secret,
            reference,
            fields,
            sealed_at: sealed_second(row.at, back),
            node_mid: row.mid.clone(),
        })
    }
}

/// The scanner's side of a card: one encapsulation, and both sides hold the pipe's secret once the
/// first letter lands.
pub struct Meeting {
    pub invite: Invite,
    pub secret: Zeroizing<[u8; 32]>,
    pub reference: String,
    pub ciphertext: [u8; MLKEM_CIPHERTEXT_SIZE],
    pub conf: String,
}

pub fn meet(invite: &Invite, root: &MlkemPublicKey) -> Result<Meeting, CardError> {
    let (ct, ss) = mlkem_encapsulate(root).map_err(|_| CardError::Crypto)?;
    let secret = Zeroizing::new(first_secret(ss.as_bytes(), root.as_bytes(), ct.as_bytes()));
    let conf = first_confirm(&secret[..], ct.as_bytes());
    let reference = reference(&secret[..]).ok_or(CardError::Crypto)?;
    Ok(Meeting {
        invite: *invite,
        secret,
        reference,
        ciphertext: *ct.as_bytes(),
        conf,
    })
}

impl Meeting {
    /// The knock of the first letter (MontanaWakePush.wakeRdv): the encapsulation and mid, words,
    /// name, glyph and confirmation, padded to one size, sealed under the invitation's letter key in
    /// the minute of the knock and laid under its tag of the day. Words that outgrow the envelope are
    /// no first letter here: the phone sends a long one as a blob, which this crate does not write.
    pub fn first_wake_body(
        &self,
        twin: &str,
        [mid, text, name, glyph]: [&str; 4],
        unix_seconds: u64,
        nonce: &[u8; NONCE_LEN],
    ) -> Option<Vec<u8>> {
        let mut body = Zeroizing::new(self.ciphertext.to_vec());
        for field in [mid, text, name, glyph, self.conf.as_str()] {
            body.extend_from_slice(field.as_bytes());
            body.push(0);
        }
        if body.len() > FIRST_ENVELOPE_SIZE {
            return None;
        }
        body.resize(FIRST_ENVELOPE_SIZE, 0);
        let conv = rdv_conv(&self.invite, day_window(unix_seconds));
        let letter = Zeroizing::new(rdv_letter_secret(&self.invite));
        let sealed = seal_letter(&letter[..], minute_window(unix_seconds), nonce, &body);
        Some(
            json!({
                "conv": conv,
                "from_id": sub_tag(&conv, twin),
                "mid": mid,
                "env": base64_standard(&sealed),
            })
            .to_string()
            .into_bytes(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::door::Ear;

    const T: u64 = 1_761_000_017;
    const TWIN_A: &str = "03f226159714a79644a7c9b63f4b1788";
    const TWIN_B: &str = "f4f72dafb742470e49defbd3c2326c27";

    fn distinct() -> Invite {
        core::array::from_fn(|i| i as u8)
    }

    fn card(mask: u8) -> AgentCard {
        let seed: [u8; MLKEM_SEED_SIZE] = core::array::from_fn(|i| i as u8 ^ mask);
        let invite: Invite = core::array::from_fn(|i| (i as u8).wrapping_mul(3) ^ mask);
        AgentCard::new(&seed, invite).expect("keypair")
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("json")
    }

    fn row_of(knock: &[u8], mid: &str, at: u64) -> BoxRow {
        let k = json_of(knock);
        BoxRow {
            conv: k["conv"].as_str().expect("conv").to_owned(),
            sealed: base64_standard_decode(k["env"].as_str().expect("env")).expect("base64"),
            mid: mid.to_owned(),
            at,
        }
    }

    // Derived by Python hashlib apart from the phone over thirty-two distinct bytes: rejects an
    // implementation without the separator (c4d7b93f...) and one that swaps the key and blob domains.
    #[test]
    fn invitation_domains_are_the_phones() {
        let inv = distinct();
        assert_eq!(
            hex::encode(rdv_key(&inv)),
            "fb3ca037ed7a476276094d7f1e3a6c15b08be424bc5beacd31ef7427ee8605cf"
        );
        assert_eq!(
            rdv_bid(&inv),
            "3a33fc1f729bc87bcf5b8190f662324d51b29bf983ae454117e90f04d7e53109"
        );
        assert_eq!(
            perm_bid(&inv),
            "a0d734b895136f677e4e469745be3fc7c9cb9da1af10440579aeec938df39223"
        );
        assert_eq!(
            hex::encode(rdv_letter_secret(&inv)),
            "61c4fb9588a2b5959131c211b4786b0aff24bb63c47d8c25aa76ab63cdc689c1"
        );
        assert_ne!(
            hex::encode(rdv_key(&inv)),
            "c4d7b93f1153937997ce4644a97fa918faf0564780d3e428d9c053064b534c64"
        );
    }

    // A day with bits at both ends rejects an implementation laying it most-significant first
    // (b3d01701...).
    #[test]
    fn the_card_tag_of_a_day() {
        let inv = distinct();
        assert_eq!(
            rdv_conv(&inv, 0x0102_0304_0506_0708),
            "30ecfa457b80e9349e06e9d9234386facfb321e5b6767e36b122d7efab04fdbe"
        );
        assert_ne!(
            rdv_conv(&inv, 0x0102_0304_0506_0708),
            "b3d01701354d7f69f45efe345edb12858817ce5e24dc9e953ef5c0d7ada2e376"
        );
        assert_eq!(
            rdv_conv(&inv, 20_382),
            "3ef2617fa5fecdebf8608f6724c38b9a5ee28d027e334722e7427881fb503549"
        );
    }

    // Rejects an implementation that hashes the ciphertext before the secret (DQ8zNJ-hc67SCsF1dgFdBQ)
    // and one that keeps the standard alphabet with its padding (4caQNHSMPKm2jqA3jfcfOw==).
    #[test]
    fn the_confirmation_is_the_phones() {
        let secret: Vec<u8> = (0u8..32).collect();
        let ct: Vec<u8> = (0..1088u32).map(|i| ((i * 7 + 3) % 256) as u8).collect();
        let conf = first_confirm(&secret, &ct);
        assert_eq!(conf, "4caQNHSMPKm2jqA3jfcfOw");
        assert!(same_confirm(&conf, "4caQNHSMPKm2jqA3jfcfOw"));
        assert!(!same_confirm(&conf, "DQ8zNJ-hc67SCsF1dgFdBQ"));
        assert!(!same_confirm(&conf, "4caQNHSMPKm2jqA3jfcfOw=="));
        assert!(!same_confirm("", ""));
    }

    // The URL alphabet in both directions: rejects an encoder that keeps '+' and '/' or the padding.
    #[test]
    fn links_are_read_in_every_form_the_phone_writes() {
        let inv: Invite = core::array::from_fn(|i| 255 - i as u8);
        assert_eq!(
            base64url_no_pad(&inv),
            "__79_Pv6-fj39vX08_Lx8O_u7ezr6uno5-bl5OPi4eA"
        );
        assert_eq!(
            base64url_no_pad_decode(&base64url_no_pad(&inv)),
            Some(inv.to_vec())
        );
        let c = card(0);
        assert_eq!(
            c.link(),
            format!(
                "https://montana.quest/perp/{}",
                base64url_no_pad(c.invite())
            )
        );
        assert_eq!(invite_in_link(&c.link()), Some(*c.invite()));
        let code = base64url_no_pad(&inv);
        for host in LINK_HOSTS {
            for kind in LINK_KINDS {
                let link = format!(" https://{host}/{kind}/{code}\n");
                assert_eq!(invite_in_link(&link), Some(inv), "{link}");
            }
        }
        assert_eq!(invite_in_link(&format!("montana://r/{code}")), Some(inv));
        assert_eq!(
            invite_in_link(&format!("https://montana.quest/c/{code}")),
            None
        );
        assert_eq!(
            invite_in_link(&format!("https://example.com/r/{code}")),
            None
        );
        assert_eq!(
            invite_in_link(&format!("montana://r/{}", &code[..40])),
            None
        );
        assert_eq!(invite_in_link("montana://r/"), None);
    }

    #[test]
    fn the_payload_carries_the_name_only_whole() {
        let c = card(0);
        assert_eq!(card_payload(c.root(), ""), c.root().to_vec());
        assert_eq!(card_payload(c.root(), " \n"), c.root().to_vec());
        let named = card_payload(c.root(), "  Agent ");
        assert_eq!(&named[MLKEM_PUBLIC_KEY_SIZE..], b"Agent");
        let full = "a".repeat(NAME_LIMIT);
        assert_eq!(
            card_payload(c.root(), &full).len(),
            MLKEM_PUBLIC_KEY_SIZE + NAME_LIMIT
        );
        let over = "a".repeat(NAME_LIMIT + 1);
        assert_eq!(card_payload(c.root(), &over), c.root().to_vec());
    }

    #[test]
    fn the_card_on_the_node_unfolds_for_the_scanner() {
        let c = card(0);
        let put = json_of(&c.blob_put_body("Agent", &[5; NONCE_LEN]));
        let o = put.as_object().expect("object");
        assert_eq!(
            o.keys().map(String::as_str).collect::<Vec<_>>(),
            ["bid", "data", "over"]
        );
        assert_eq!(o["bid"], rdv_bid(c.invite()).as_str());
        assert_eq!(o["over"], Value::Bool(true));
        let answer = json!({ "data": o["data"].clone() }).to_string();
        let sealed = read_blob(answer.as_bytes()).expect("data");
        assert_eq!(&sealed[..NONCE_LEN], &[5; NONCE_LEN]);
        match unfold(c.invite(), &sealed) {
            Some(Unfolded::Card { root, name }) => {
                assert_eq!(root.as_bytes(), c.root());
                assert_eq!(name, "Agent");
            },
            _ => panic!("a card"),
        }
        assert!(unfold(&distinct(), &sealed).is_none());
        let tomb = seal_blob(&rdv_key(c.invite()), &[1; NONCE_LEN], SPENT_MAGIC);
        assert!(matches!(unfold(c.invite(), &tomb), Some(Unfolded::Spent)));
        let mark = json_of(&c.perm_mark_body(&[6; NONCE_LEN]));
        assert_eq!(mark["bid"], perm_bid(c.invite()).as_str());
        let mark = base64_standard_decode(mark["data"].as_str().expect("data")).expect("base64");
        assert!(is_permanent(c.invite(), &mark));
        assert!(!is_permanent(c.invite(), &sealed));
        assert!(!is_permanent(&distinct(), &mark));
        assert_eq!(json_of(&blob_get_body("ab")), json!({ "bid": "ab" }));
        assert!(read_blob(b"{}").is_none());
    }

    #[test]
    fn a_first_letter_makes_one_pipe_on_both_sides() {
        let c = card(0);
        let met = meet(c.invite(), &c.root).expect("encapsulates");
        let knock = met
            .first_wake_body(
                TWIN_B,
                ["m-01", "Hello agent", "Petros", "P"],
                T,
                &[3; NONCE_LEN],
            )
            .expect("fits");
        let k = json_of(&knock);
        let conv = rdv_conv(c.invite(), day_window(T));
        assert_eq!(k["conv"], conv.as_str());
        assert_eq!(k["from_id"], sub_tag(&conv, TWIN_B).as_str());
        assert_eq!(k["mid"], "m-01");
        let taken = T + 120;
        let row = row_of(&knock, "row-1", taken);
        assert_eq!(row.sealed.len(), NONCE_LEN + FIRST_ENVELOPE_SIZE + 16);
        let landed = c.accept(&row).expect("proved");
        assert_eq!(landed.secret[..], met.secret[..]);
        assert_eq!(landed.reference, met.reference);
        assert_eq!(
            reference(&landed.secret[..]),
            Some(landed.reference.clone())
        );
        assert_eq!(
            landed.fields,
            ["m-01", "Hello agent", "Petros", "P", met.conf.as_str()]
        );
        assert_eq!(landed.node_mid, "row-1");
        assert_eq!(landed.sealed_at, taken - 120);
        assert!(c.labels(TWIN_A, taken).iter().any(|(l, _)| *l == row.conv));
    }

    #[test]
    fn a_letter_the_secret_does_not_confirm_is_a_ghost() {
        let (mine, other) = (card(0), card(0x5a));
        let mut stray = meet(other.invite(), &other.root).expect("encapsulates");
        stray.invite = *mine.invite();
        let knock = stray
            .first_wake_body(TWIN_B, ["m-02", "Hi", "", ""], T, &[4; NONCE_LEN])
            .expect("fits");
        assert_eq!(
            mine.accept(&row_of(&knock, "r", T)).err(),
            Some(CardError::Ghost)
        );
        let mut silent = meet(mine.invite(), &mine.root).expect("encapsulates");
        silent.conf.clear();
        let knock = silent
            .first_wake_body(TWIN_B, ["m-03", "Hi", "", ""], T, &[4; NONCE_LEN])
            .expect("fits");
        assert_eq!(
            mine.accept(&row_of(&knock, "r", T)).err(),
            Some(CardError::Ghost)
        );
    }

    #[test]
    fn rows_that_are_no_first_letter_are_refused_by_name() {
        let c = card(0);
        let met = meet(c.invite(), &c.root).expect("encapsulates");
        let knock = met
            .first_wake_body(TWIN_B, ["m-04", "Hi", "", ""], T, &[2; NONCE_LEN])
            .expect("fits");
        let mut row = row_of(&knock, "r", T);
        assert_eq!(card(0x11).accept(&row).err(), Some(CardError::NotThisCard));
        row.at = T + 5 * 86_400;
        assert_eq!(c.accept(&row).err(), Some(CardError::NotThisCard));
        row.at = T;
        row.sealed[NONCE_LEN] ^= 1;
        assert_eq!(c.accept(&row).err(), Some(CardError::Sealed));
        let letter = rdv_letter_secret(c.invite());
        let w = minute_window(T);
        let mut short = met.ciphertext.to_vec();
        short.extend_from_slice(b"m\0t\0");
        row.sealed = seal_letter(&letter, w, &[2; NONCE_LEN], &short);
        assert_eq!(c.accept(&row).err(), Some(CardError::NotFirstLetter));
        let mut nameless = met.ciphertext.to_vec();
        nameless.extend_from_slice(b"m-05\0\0\0\0");
        nameless.resize(FIRST_ENVELOPE_SIZE, 0);
        row.sealed = seal_letter(&letter, w, &[2; NONCE_LEN], &nameless);
        assert_eq!(c.accept(&row).err(), Some(CardError::NotFirstLetter));
        let long = "x".repeat(FIRST_ENVELOPE_SIZE);
        assert!(met
            .first_wake_body(TWIN_B, ["m-06", &long, "", ""], T, &[2; NONCE_LEN])
            .is_none());
    }

    #[test]
    fn the_ear_reads_the_card_box_beside_the_pipes() {
        let c = card(0);
        let pipe: Vec<u8> = (0u8..32).collect();
        let ear = Ear::new([("chat", pipe.as_slice())], TWIN_A, T).with_labels(c.labels(TWIN_A, T));
        assert_eq!(ear.labels().len(), 8);
        assert_eq!(&ear.labels()[4..], c.labels(TWIN_A, T).as_slice());
        let met = meet(c.invite(), &c.root).expect("encapsulates");
        let knock = met
            .first_wake_body(TWIN_B, ["m-07", "Hi", "", ""], T, &[8; NONCE_LEN])
            .expect("fits");
        let row = row_of(&knock, "r", T);
        assert!(ear.open(&row).is_none());
        assert!(c.accept(&row).is_ok());
    }
}
