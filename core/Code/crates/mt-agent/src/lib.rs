//! The machine of an agent is a node (Montana App, «An agent»): it holds the pipes of the agent and
//! deposits and collects for them itself, as any device does. A correspondent's phone meets it only
//! if every byte the agent seals, knocks and opens is the phone's own, so every cipher here is the
//! core's (mt-messenger-e2e, mt-mnemonic) and this crate holds only the compositions of the node wire
//! the phone holds in MTNodeWire.swift, frozen by vectors derived apart from both: its own seed and
//! the names derived from it (seed), the letter (letter), the keys and tags (wire), the bodies a
//! door takes and gives (door), and the card a correspondent first meets the agent by (card).

pub mod card;
pub mod door;
pub mod letter;
pub mod seed;
pub mod wire;
