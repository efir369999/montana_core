//! The door of a node as a phone speaks to it (MTNodeWire.knockLetter, MontanaWakePush.pickup and
//! boxDel): a knock deposits one sealed letter under its conversation's tag of the day, a read asks
//! the tags of every pipe page by page, a burial gives back the rows in hand. The door sees tags and
//! sealed bytes, never a secret, a name or a word. The bodies are the phone's JSON; the road, an
//! HTTPS POST to each door, is the caller's.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use zeroize::Zeroizing;

use crate::letter::{base64_standard, base64_standard_decode, letter_fields};
use crate::wire::{
    box_days, conv_tag, day_window, minute_window, open_boxed, seal_letter, sealed_second, sub_tag,
    NONCE_LEN,
};

pub const WAKE_PATH: &str = "/wake";
pub const FETCH_PATH: &str = "/fetch";
pub const BOX_DEL_PATH: &str = "/box-del";
/// Pairs of one read page: 512 labels (about 69 KB) outgrew the node's 64 KiB body and were
/// refused with 413. A page of letters this full means the box holds more.
pub const PAGE: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoorError {
    NotJson,
    NoLetters,
}

/// The body of one knock at one door: the seal is made at the knock, so every door gets a nonce
/// of its own.
pub fn wake_body(
    secret: &[u8],
    twin: &str,
    mid: &str,
    envelope: &[u8],
    unix_seconds: u64,
    nonce: &[u8; NONCE_LEN],
    silent: bool,
) -> Vec<u8> {
    let conv = conv_tag(secret, day_window(unix_seconds));
    let from_id = sub_tag(&conv, twin);
    let sealed = seal_letter(secret, minute_window(unix_seconds), nonce, envelope);
    let mut body = json!({
        "conv": conv,
        "from_id": from_id,
        "mid": mid,
        "env": base64_standard(&sealed),
    });
    if silent {
        body["silent"] = Value::Bool(true);
    }
    body.to_string().into_bytes()
}

struct Pipe {
    name: String,
    secret: Zeroizing<Vec<u8>>,
}

/// The labels one person listens at across the box's term (MontanaWakePush.BoxEar): for every pipe
/// and day, the conversation's tag and the person's own tag beside it. The door yields no letter to
/// the tag that laid it, so the agent never reads its own.
pub struct Ear {
    labels: Vec<(String, String)>,
    pipe_of: BTreeMap<String, usize>,
    pipes: Vec<Pipe>,
}

impl Ear {
    pub fn new<'a>(
        pipes: impl IntoIterator<Item = (&'a str, &'a [u8])>,
        twin: &str,
        unix_seconds: u64,
    ) -> Self {
        let days = box_days(day_window(unix_seconds));
        let mut ear = Ear {
            labels: Vec::new(),
            pipe_of: BTreeMap::new(),
            pipes: Vec::new(),
        };
        for (name, secret) in pipes {
            let at = ear.pipes.len();
            for day in days.clone() {
                let conv = conv_tag(secret, day);
                let sid = sub_tag(&conv, twin);
                ear.pipe_of.insert(conv.clone(), at);
                ear.labels.push((conv, sid));
            }
            ear.pipes.push(Pipe {
                name: name.to_owned(),
                secret: Zeroizing::new(secret.to_vec()),
            });
        }
        ear
    }

    /// Tags read beside the pipes' -- a card's box (AgentCard.labels): a row under them opens with
    /// the card, never here.
    pub fn with_labels(mut self, more: impl IntoIterator<Item = (String, String)>) -> Self {
        self.labels.extend(more);
        self
    }

    pub fn labels(&self) -> &[(String, String)] {
        &self.labels
    }

    /// The read's pages; each page goes to every door at once.
    pub fn pages(&self) -> Vec<Vec<u8>> {
        self.labels
            .chunks(PAGE)
            .map(|page| {
                let subs: Vec<&str> = page.iter().map(|(c, _)| c.as_str()).collect();
                let sids: Vec<&str> = page.iter().map(|(_, s)| s.as_str()).collect();
                json!({ "subs": subs, "sids": sids })
                    .to_string()
                    .into_bytes()
            })
            .collect()
    }

    /// A row opens under its tag's secret, walking back from the minute the node took it. A row of a
    /// pipe this ear does not hold, one that stays sealed, or one without a name and words is no
    /// letter.
    pub fn open(&self, row: &BoxRow) -> Option<Landed> {
        let pipe = self.pipes.get(*self.pipe_of.get(&row.conv)?)?;
        let (plain, back) = open_boxed(&row.sealed, &pipe.secret, row.at)?;
        let fields = letter_fields(&plain);
        if fields[0].is_empty() || fields[1].is_empty() {
            return None;
        }
        Some(Landed {
            pipe: pipe.name.clone(),
            fields,
            sealed_at: sealed_second(row.at, back),
            node_mid: row.mid.clone(),
        })
    }
}

/// One row of a read as the door yields it: the tag it lies under, the sealed letter, the door's
/// name of the row (the one a burial gives back) and the second the node took it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoxRow {
    pub conv: String,
    pub sealed: Vec<u8>,
    pub mid: String,
    pub at: u64,
}

/// An opened letter: the pipe's own name, its eight fields (letter_fields) and the second it was
/// sealed -- the node's second less the walk back the opening took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Landed {
    pub pipe: String,
    pub fields: Vec<String>,
    pub sealed_at: u64,
    pub node_mid: String,
}

/// A door's answer to a read page. A row without its tag or with a broken letter is passed over,
/// as the phone passes it; an answer without the letters is the door's refusal.
pub fn read_fetch(answer: &[u8]) -> Result<Vec<BoxRow>, DoorError> {
    let v: Value = serde_json::from_slice(answer).map_err(|_| DoorError::NotJson)?;
    let rows = v
        .get("letters")
        .and_then(Value::as_array)
        .ok_or(DoorError::NoLetters)?;
    Ok(rows
        .iter()
        .filter_map(|l| {
            let conv = l.get("c")?.as_str()?.to_owned();
            let sealed = base64_standard_decode(l.get("e")?.as_str()?)?;
            let mid = l
                .get("m")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let at = l
                .get("at")
                .and_then(Value::as_i64)
                .map_or(0, |s| u64::try_from(s).unwrap_or(0));
            Some(BoxRow {
                conv,
                sealed,
                mid,
                at,
            })
        })
        .collect())
}

/// The rows of every door's answer to one page: stores never talk to each other, so a letter may lie
/// on more than one, and a row the doors share is kept once by the door's name of it.
pub fn merge(answers: impl IntoIterator<Item = Vec<BoxRow>>) -> Vec<BoxRow> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for row in answers.into_iter().flatten() {
        if row.mid.is_empty() || seen.insert(row.mid.clone()) {
            out.push(row);
        }
    }
    out
}

/// Only a row in hand is given back: the node removes a letter on this word and on no other.
pub fn box_del_body(mids: &[String]) -> Vec<u8> {
    json!({ "mids": mids }).to_string().into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::letter::{letter_head, pad_envelope};

    const T: u64 = 1_761_000_017;
    const TWIN_A: &str = "03f226159714a79644a7c9b63f4b1788";
    const TWIN_B: &str = "f4f72dafb742470e49defbd3c2326c27";

    fn secret(mask: u8) -> Vec<u8> {
        (0u8..32).map(|i| i ^ mask).collect()
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("json")
    }

    #[test]
    fn a_knock_is_the_phones_form() {
        let s = secret(0);
        let env = pad_envelope(&letter_head("m-01", "Hello", "Agent", "")).expect("fits");
        let body = json_of(&wake_body(
            &s,
            TWIN_A,
            "m-01",
            &env,
            T,
            &[9; NONCE_LEN],
            false,
        ));
        let o = body.as_object().expect("object");
        assert_eq!(
            o.keys().map(String::as_str).collect::<Vec<_>>(),
            ["conv", "env", "from_id", "mid"]
        );
        let conv = conv_tag(&s, day_window(T));
        assert_eq!(o["conv"], conv.as_str());
        assert_eq!(o["from_id"], sub_tag(&conv, TWIN_A).as_str());
        assert_eq!(o["mid"], "m-01");
        let sealed = base64_standard_decode(o["env"].as_str().expect("env")).expect("base64");
        assert_eq!(sealed.len(), NONCE_LEN + env.len() + 16);
        assert_eq!(&sealed[..NONCE_LEN], &[9; NONCE_LEN]);
        let quiet = json_of(&wake_body(
            &s,
            TWIN_A,
            "m-01",
            &env,
            T,
            &[9; NONCE_LEN],
            true,
        ));
        assert_eq!(quiet["silent"], Value::Bool(true));
        assert!(body.get("silent").is_none());
        assert_eq!(open_boxed(&sealed, &s, T), Some((env, 0)));
    }

    #[test]
    fn the_ear_listens_at_four_days_of_every_pipe_under_its_own_tag() {
        let (a, b) = (secret(0), secret(0x5a));
        let ear = Ear::new(
            [("pipe-a", a.as_slice()), ("pipe-b", b.as_slice())],
            TWIN_A,
            T,
        );
        let days: Vec<u64> = box_days(day_window(T)).collect();
        assert_eq!(days.len(), 4);
        assert_eq!(ear.labels().len(), 8);
        for (i, (conv, sid)) in ear.labels().iter().enumerate() {
            let s = if i < 4 { &a } else { &b };
            assert_eq!(conv, &conv_tag(s, days[i % 4]));
            assert_eq!(sid, &sub_tag(conv, TWIN_A));
        }
        let pages = ear.pages();
        assert_eq!(pages.len(), 1);
        let p = json_of(&pages[0]);
        assert_eq!(p["subs"].as_array().map(Vec::len), Some(8));
        assert_eq!(p["sids"][7], ear.labels()[7].1.as_str());
    }

    #[test]
    fn a_read_is_cut_into_pages_of_a_hundred_and_twenty_eight() {
        let secrets: Vec<Vec<u8>> = (0..40u8).map(secret).collect();
        let names: Vec<String> = (0..40).map(|i| format!("p{i}")).collect();
        let ear = Ear::new(
            names
                .iter()
                .map(String::as_str)
                .zip(secrets.iter().map(Vec::as_slice)),
            TWIN_A,
            T,
        );
        let sizes: Vec<usize> = ear
            .pages()
            .iter()
            .map(|p| json_of(p)["subs"].as_array().map_or(0, Vec::len))
            .collect();
        assert_eq!(sizes, [PAGE, 160 - PAGE]);
    }

    #[test]
    fn a_letter_knocked_by_one_side_lands_with_the_other() {
        let s = secret(0x33);
        let env = pad_envelope(&letter_head("m-07", "Hi agent", "Petros", "")).expect("fits");
        let knock = json_of(&wake_body(
            &s,
            TWIN_B,
            "m-07",
            &env,
            T,
            &[1; NONCE_LEN],
            false,
        ));
        let taken = T + 180;
        let answer = json!({ "letters": [
            { "c": knock["conv"].clone(), "e": knock["env"].clone(), "m": "row-1", "at": taken },
            { "c": knock["conv"].clone(), "e": "not base64!", "m": "row-2", "at": taken },
            { "e": knock["env"].clone(), "m": "row-3" },
            { "c": "ff", "e": knock["env"].clone(), "m": "row-4", "at": taken },
        ]})
        .to_string();
        let rows = read_fetch(answer.as_bytes()).expect("letters");
        assert_eq!(
            rows.iter().map(|r| r.mid.as_str()).collect::<Vec<_>>(),
            ["row-1", "row-4"]
        );
        let ear = Ear::new([("chat", s.as_slice())], TWIN_A, taken);
        let landed = ear.open(&rows[0]).expect("opens");
        assert_eq!(landed.pipe, "chat");
        assert_eq!(&landed.fields[..3], &["m-07", "Hi agent", "Petros"]);
        assert_eq!(landed.node_mid, "row-1");
        let back = minute_window(taken) - minute_window(T);
        assert_eq!(back, 3);
        assert_eq!(landed.sealed_at, taken - back * 60);
        assert!(ear.open(&rows[1]).is_none());
        let other = secret(0x34);
        let stranger = Ear::new([("chat", other.as_slice())], TWIN_A, taken);
        assert!(stranger.open(&rows[0]).is_none());
    }

    #[test]
    fn an_answer_without_letters_is_a_refusal() {
        assert_eq!(read_fetch(b"<html>"), Err(DoorError::NotJson));
        assert_eq!(
            read_fetch(br#"{"error":"too often"}"#),
            Err(DoorError::NoLetters)
        );
        assert_eq!(read_fetch(br#"{"letters":[]}"#), Ok(vec![]));
        let rows = read_fetch(br#"{"letters":[{"c":"aa","e":"Zg==","at":-5}]}"#).expect("letters");
        assert_eq!(
            rows,
            vec![BoxRow {
                conv: "aa".into(),
                sealed: b"f".to_vec(),
                mid: String::new(),
                at: 0,
            }]
        );
    }

    #[test]
    fn a_row_two_doors_yield_is_kept_once() {
        let row = |m: &str| BoxRow {
            conv: "aa".into(),
            sealed: vec![1],
            mid: m.into(),
            at: 1,
        };
        let merged = merge([vec![row("x"), row("y")], vec![row("y"), row(""), row("")]]);
        assert_eq!(
            merged.iter().map(|r| r.mid.as_str()).collect::<Vec<_>>(),
            ["x", "y", "", ""]
        );
    }

    #[test]
    fn a_burial_names_the_rows_in_hand() {
        let body = json_of(&box_del_body(&["row-1".to_owned(), "row-4".to_owned()]));
        assert_eq!(body, json!({ "mids": ["row-1", "row-4"] }));
    }
}
