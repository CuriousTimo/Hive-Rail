//! The layout's LocoNet throttle slots, shared by every node over the mesh.
//!
//! Every node is the slot master for its own LocoNet, but the slot numbers belong to the
//! whole layout: slot 7 is the same loco on every node. So a FRED that is unplugged at
//! one module and plugged in at another asks for its slot again and gets the same loco.
//!
//! - Every node keeps a copy of the table. A change is published as a [`SlotEntry`] with
//!   a higher version; the newest version wins, ties go to the higher node id.
//! - Only the **allocator**, the online node with the lowest id, hands out new slot
//!   numbers. Another node asks it with a slot request and answers the throttle when the
//!   slot arrives, so two nodes never give out the same slot.
//! - A slot that was taken before any node knew it (for example by Rocrail's own slot
//!   server) has no known address until a client reads or writes it again.

use crate::loconet::LocoNetMessage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const OPC_LOCO_SPD: u8 = 0xA0;
const OPC_LOCO_DIRF: u8 = 0xA1;
const OPC_SLOT_STAT1: u8 = 0xB5;
const OPC_MOVE_SLOTS: u8 = 0xBA;
const OPC_RQ_SL_DATA: u8 = 0xBB;
const OPC_LOCO_ADR: u8 = 0xBF;
const OPC_SL_RD_DATA: u8 = 0xE7;
const OPC_WR_SL_DATA: u8 = 0xEF;
/// Slots 1 to 119 hold locos; 0 is dispatch and 120 and up are special slots.
const LAST_LOCO_SLOT: u8 = 119;
/// Slot status bits 4 and 5: `00` free, `11` in use.
const SLOT_BUSY_MASK: u8 = 0x30;
const SLOT_IN_USE: u8 = 0x30;
/// 128 speed steps, the decoder type Rocrail and JMRI expect by default.
const SLOT_128_STEPS: u8 = 0x03;
const DIRF_REVERSE: u8 = 0x20;
const DIRF_F0: u8 = 0x10;
/// `LONG_ACK` code for "no free slot".
const ACK_NO_FREE_SLOT: u8 = 0x00;
const ACK_ACCEPTED: u8 = 0x7F;

/// One loco as shared with the other nodes in the heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocoInfo {
    pub slot: u8,
    #[serde(default)]
    pub address: Option<u16>,
    /// LocoNet speed step: 0 stop, 1 emergency stop, 2 to 127 moving.
    pub speed: u8,
    pub forward: bool,
    /// F0 to F4, bit n for Fn. Missing from nodes built before functions were shared.
    #[serde(default)]
    pub functions: u8,
}

impl LocoInfo {
    pub fn label(&self) -> String {
        match self.address {
            Some(address) => format!("loco {address}"),
            None => "loco ? (address unknown)".to_owned(),
        }
    }

    pub fn direction(&self) -> &'static str {
        if self.forward {
            "forward"
        } else {
            "reverse"
        }
    }

    pub fn function_on(&self, number: u8) -> bool {
        self.functions & 1 << number != 0
    }

    pub fn speed_label(&self) -> String {
        match self.speed {
            0 => "stopped".to_owned(),
            1 => "EMERGENCY STOP".to_owned(),
            speed => format!("speed {}", speed - 1),
        }
    }
}

/// One slot as shared over the mesh on `hive/{session}/slots/update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotEntry {
    pub slot: u8,
    pub address: Option<u16>,
    pub status: u8,
    pub speed: u8,
    pub dirf: u8,
    pub version: u64,
    /// The node whose LocoNet drives the slot (where the throttle is plugged in).
    pub node: String,
}

impl SlotEntry {
    /// LocoNet slot status in words; a free slot with an address is assigned but not yet
    /// taken by a throttle.
    pub fn status_label(&self) -> &'static str {
        match (self.status & SLOT_BUSY_MASK, self.address) {
            (0x00, None) => "free",
            (0x00, Some(_)) => "assigned",
            (0x10, _) => "common",
            (0x20, _) => "idle",
            _ => "in use",
        }
    }

    pub fn loco(&self) -> LocoInfo {
        LocoInfo::from(self)
    }

    fn newer_than(&self, other: &SlotEntry) -> bool {
        (self.version, &self.node) > (other.version, &other.node)
    }

    fn in_use(&self) -> bool {
        self.status & SLOT_BUSY_MASK == SLOT_IN_USE
    }

    /// Short form for the CLI log.
    pub fn describe(&self) -> String {
        let loco = LocoInfo::from(self);
        if self.status & SLOT_BUSY_MASK == 0 && self.address.is_none() {
            return format!("slot {} free", self.slot);
        }
        format!(
            "slot {} {} {}, {} at {}",
            self.slot,
            loco.label(),
            loco.speed_label(),
            loco.direction(),
            self.node
        )
    }
}

impl From<&SlotEntry> for LocoInfo {
    fn from(entry: &SlotEntry) -> Self {
        LocoInfo {
            slot: entry.slot,
            address: entry.address,
            speed: entry.speed,
            forward: entry.dirf & DIRF_REVERSE == 0,
            functions: u8::from(entry.dirf & DIRF_F0 != 0) | (entry.dirf & 0x0F) << 1,
        }
    }
}

/// What one LocoNet message from this node's clients leads to.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Replies for this node's LocoNet clients.
    pub replies: Vec<LocoNetMessage>,
    /// Changed slots to publish to the other nodes.
    pub publish: Vec<SlotEntry>,
    /// A loco address that needs a slot from the allocator.
    pub request: Option<u16>,
}

#[derive(Debug)]
pub struct LocoTable {
    node: String,
    slots: BTreeMap<u8, SlotEntry>,
    /// Addresses this node asked the allocator for; their throttles wait for the slot.
    pending: BTreeSet<u16>,
    /// Slots taken before this node knew them, whose address it already asked for.
    asked: BTreeSet<u8>,
}

impl LocoTable {
    pub fn new(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            slots: BTreeMap::new(),
            pending: BTreeSet::new(),
            asked: BTreeSet::new(),
        }
    }

    /// Updates the slots from one LocoNet message of this node's clients. `allocator`
    /// says whether this node hands out new slot numbers itself.
    pub fn apply(
        &mut self,
        message: &LocoNetMessage,
        track_status: u8,
        allocator: bool,
    ) -> Outcome {
        let b = message.as_bytes();
        let mut outcome = Outcome::default();
        match message.opcode() {
            OPC_LOCO_ADR => {
                let address = u16::from(b[1]) << 7 | u16::from(b[2]);
                if let Some(number) = self.slot_of(address) {
                    outcome.replies.push(self.slot_read(number, track_status));
                } else if allocator {
                    match self.allocate(address, &self.node.clone()) {
                        Some(entry) => {
                            outcome
                                .replies
                                .push(self.slot_read(entry.slot, track_status));
                            outcome.publish.push(entry);
                        }
                        None => outcome
                            .replies
                            .push(LocoNetMessage::long_ack(OPC_LOCO_ADR, ACK_NO_FREE_SLOT)),
                    }
                } else if self.pending.insert(address) {
                    outcome.request = Some(address);
                }
            }
            // Known slots are answered from the table, and so are free ones, so a slot
            // monitor such as JMRI's can read the whole table. A slot driven without a
            // known address is left to whoever knows more.
            OPC_RQ_SL_DATA if self.address_known(b[1]) => {
                outcome.replies.push(self.slot_read(b[1], track_status));
            }
            OPC_RQ_SL_DATA if is_loco_slot(b[1]) && self.is_free(b[1]) => {
                outcome.replies.push(LocoNetMessage::loco_slot_read(
                    b[1],
                    SLOT_128_STEPS,
                    0,
                    0,
                    0,
                    track_status,
                ));
            }
            // A null move (source and destination equal) marks the slot in use.
            OPC_MOVE_SLOTS if b[1] == b[2] && self.address_known(b[1]) => {
                outcome
                    .publish
                    .push(self.update(b[1], |slot| slot.status |= SLOT_IN_USE));
                outcome.replies.push(self.slot_read(b[1], track_status));
            }
            OPC_LOCO_SPD if is_loco_slot(b[1]) => {
                outcome
                    .publish
                    .push(self.drive(b[1], |slot| slot.speed = b[2]));
                outcome.replies.extend(self.ask_address(b[1]));
            }
            OPC_LOCO_DIRF if is_loco_slot(b[1]) => {
                outcome
                    .publish
                    .push(self.drive(b[1], |slot| slot.dirf = b[2]));
                outcome.replies.extend(self.ask_address(b[1]));
            }
            OPC_SLOT_STAT1 if is_loco_slot(b[1]) => {
                let status = b[2];
                outcome.publish.push(self.update(b[1], |slot| {
                    slot.status = status;
                    if status & SLOT_BUSY_MASK == 0 {
                        slot.address = None;
                    }
                }));
            }
            // Slot data: op, 0E, slot, stat, adr, spd, dirf, trk, ss2, adr2, snd, id1, id2, chk.
            OPC_SL_RD_DATA | OPC_WR_SL_DATA if b.len() == 14 && is_loco_slot(b[2]) => {
                let write = message.opcode() == OPC_WR_SL_DATA;
                if b[3] & SLOT_BUSY_MASK != 0 {
                    let address = u16::from(b[4]) | u16::from(b[9]) << 7;
                    outcome.publish.push(self.update(b[2], |slot| {
                        slot.status = b[3];
                        slot.address = Some(address);
                        slot.speed = b[5];
                        slot.dirf = b[6];
                    }));
                } else if write {
                    // Rocrail's slot server answers a slot it drives as free with address 0;
                    // only a client's own write releases a slot.
                    outcome.publish.push(self.update(b[2], |slot| {
                        slot.status = b[3];
                        slot.address = None;
                    }));
                }
                if write {
                    outcome
                        .replies
                        .push(LocoNetMessage::long_ack(OPC_WR_SL_DATA, ACK_ACCEPTED));
                }
            }
            _ => {}
        }
        outcome
    }

    /// Applies slots published by other nodes and returns the replies for throttles on
    /// this node that were waiting for one of them.
    pub fn merge(&mut self, entries: Vec<SlotEntry>, track_status: u8) -> Vec<LocoNetMessage> {
        for entry in entries {
            if !is_loco_slot(entry.slot) {
                continue;
            }
            match self.slots.get(&entry.slot) {
                Some(local) if !entry.newer_than(local) => {}
                _ => {
                    self.asked.remove(&entry.slot);
                    self.slots.insert(entry.slot, entry);
                }
            }
        }
        let answered: Vec<(u16, u8)> = self
            .pending
            .iter()
            .filter_map(|&address| self.slot_of(address).map(|number| (address, number)))
            .collect();
        answered
            .into_iter()
            .map(|(address, number)| {
                self.pending.remove(&address);
                self.slot_read(number, track_status)
            })
            .collect()
    }

    /// On the allocator: a slot for another node's throttle. Returns the entry to publish,
    /// or None when every slot is taken.
    pub fn allocate_for(&mut self, address: u16, node: &str) -> Option<SlotEntry> {
        match self.slot_of(address) {
            Some(number) => self.slots.get(&number).cloned(),
            None => self.allocate(address, node),
        }
    }

    /// Every slot this node knows, for a node that just came online.
    pub fn snapshot(&self) -> Vec<SlotEntry> {
        self.slots.values().cloned().collect()
    }

    /// A short fingerprint of the whole table, shared in the heartbeat so nodes and the
    /// dispatcher can see whether every copy is the same. FNV-1a, so it is the same on
    /// every platform and build.
    pub fn checksum(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut add = |bytes: &[u8]| {
            for &byte in bytes {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
        };
        for entry in self.slots.values() {
            add(&[entry.slot, entry.status, entry.speed, entry.dirf]);
            add(&entry.address.unwrap_or(u16::MAX).to_le_bytes());
            add(&entry.version.to_le_bytes());
            add(entry.node.as_bytes());
            add(&[0]);
        }
        hash
    }

    pub fn is_pending(&self, address: u16) -> bool {
        self.pending.contains(&address)
    }

    /// Gives up waiting for the allocator; the throttle will ask again.
    pub fn cancel(&mut self, address: u16) {
        self.pending.remove(&address);
    }

    /// Locos in use that are driven through this node's LocoNet, by slot.
    pub fn locos(&self) -> Vec<LocoInfo> {
        self.slots
            .values()
            .filter(|entry| entry.in_use() && entry.node == self.node)
            .map(LocoInfo::from)
            .collect()
    }

    fn slot_of(&self, address: u16) -> Option<u8> {
        self.slots
            .values()
            .find(|entry| entry.address == Some(address))
            .map(|entry| entry.slot)
    }

    /// Reserves the lowest free slot for this address, driven from `node`.
    fn allocate(&mut self, address: u16, node: &str) -> Option<SlotEntry> {
        let number = (1..=LAST_LOCO_SLOT).find(|number| {
            self.slots
                .get(number)
                .is_none_or(|entry| entry.status & SLOT_BUSY_MASK == 0 && entry.address.is_none())
        })?;
        let version = self.slots.get(&number).map_or(0, |entry| entry.version) + 1;
        let entry = SlotEntry {
            slot: number,
            address: Some(address),
            status: SLOT_128_STEPS,
            speed: 0,
            dirf: 0,
            version,
            node: node.to_owned(),
        };
        self.slots.insert(number, entry.clone());
        Some(entry)
    }

    fn is_free(&self, number: u8) -> bool {
        self.slots
            .get(&number)
            .is_none_or(|entry| entry.status & SLOT_BUSY_MASK == 0 && entry.address.is_none())
    }

    fn address_known(&self, number: u8) -> bool {
        self.slots
            .get(&number)
            .is_some_and(|entry| entry.address.is_some())
    }

    /// Changes a slot on this node's behalf and returns the new entry to publish.
    fn update(&mut self, number: u8, change: impl FnOnce(&mut SlotEntry)) -> SlotEntry {
        let node = self.node.clone();
        let entry = self.slots.entry(number).or_insert(SlotEntry {
            slot: number,
            address: None,
            status: SLOT_128_STEPS,
            speed: 0,
            dirf: 0,
            version: 0,
            node: node.clone(),
        });
        change(entry);
        entry.version += 1;
        entry.node = node;
        entry.clone()
    }

    /// A slot that a client drives is in use, even if no node assigned it.
    fn drive(&mut self, number: u8, change: impl FnOnce(&mut SlotEntry)) -> SlotEntry {
        self.update(number, |slot| {
            slot.status |= SLOT_IN_USE;
            change(slot);
        })
    }

    /// Once per slot taken before the layout knew it: asks the LocoNet for its contents.
    /// A slot master such as Rocrail's slot server answers with `SL_RD_DATA`, which
    /// gives the address; without one nothing answers.
    fn ask_address(&mut self, number: u8) -> Option<LocoNetMessage> {
        (!self.address_known(number) && self.asked.insert(number))
            .then(|| LocoNetMessage::request_slot_data(number))
    }

    fn slot_read(&self, number: u8, track_status: u8) -> LocoNetMessage {
        let slot = &self.slots[&number];
        LocoNetMessage::loco_slot_read(
            number,
            slot.status,
            slot.address.unwrap_or(0),
            slot.speed,
            slot.dirf,
            track_status,
        )
    }
}

fn is_loco_slot(slot: u8) -> bool {
    (1..=LAST_LOCO_SLOT).contains(&slot)
}

#[cfg(test)]
mod tests {
    use super::{LocoTable, Outcome};
    use crate::loconet::LocoNetMessage;

    const TRACK_ON: u8 = 0x07;

    fn send(table: &mut LocoTable, hex: &str, allocator: bool) -> Outcome {
        table.apply(&LocoNetMessage::from_hex(hex).unwrap(), TRACK_ON, allocator)
    }

    fn apply(table: &mut LocoTable, hex: &str) -> Vec<String> {
        hexes(&send(table, hex, true).replies)
    }

    fn hexes(messages: &[LocoNetMessage]) -> Vec<String> {
        messages.iter().map(LocoNetMessage::to_hex).collect()
    }

    #[test]
    fn throttle_request_gives_the_address_a_slot() {
        let mut table = LocoTable::new("a");
        // LOCO_ADR 3: slot 1, free, 128 steps.
        assert_eq!(
            apply(&mut table, "BF 00 03 43"),
            ["E7 0E 01 03 03 00 00 07 00 00 00 00 00 10"]
        );
        assert!(table.locos().is_empty());

        // Null move: slot 1 in use.
        assert_eq!(
            apply(&mut table, "BA 01 01 45"),
            ["E7 0E 01 33 03 00 00 07 00 00 00 00 00 20"]
        );
        // Recorded from Rocrail: speed 40, then reverse with F0 on.
        apply(&mut table, "A0 01 29 77");
        apply(&mut table, "A1 01 30 6F");

        let locos = table.locos();
        assert_eq!(locos.len(), 1);
        assert_eq!(locos[0].label(), "loco 3");
        assert_eq!(locos[0].direction(), "reverse");
        assert_eq!(locos[0].speed_label(), "speed 40");
        assert!(locos[0].function_on(0));
        assert!(!locos[0].function_on(1));

        // The same address gets the same slot again.
        assert_eq!(
            apply(&mut table, "BF 00 03 43"),
            ["E7 0E 01 33 03 29 30 07 00 00 00 00 00 39"]
        );
        // A long address (1234 = 0x09 0x52) gets the next slot.
        assert_eq!(
            apply(&mut table, "BF 09 52 1B"),
            ["E7 0E 02 03 52 00 00 07 00 09 00 00 00 4B"]
        );
    }

    #[test]
    fn a_slot_taken_before_start_is_driven_without_an_address() {
        let mut table = LocoTable::new("a");
        // Recorded from Rocrail: speed 0, then forward with F1 and F2 on.
        assert_eq!(apply(&mut table, "A0 01 00 5E"), ["BB 01 00 45"]);
        assert!(apply(&mut table, "A1 01 03 5C").is_empty());

        let locos = table.locos();
        assert_eq!(locos[0].label(), "loco ? (address unknown)");
        assert_eq!(locos[0].functions, 0b0_0110);
        // Asked once, and not answered by this node.
        assert!(apply(&mut table, "A0 01 00 5E").is_empty());
        assert!(apply(&mut table, "BB 01 00 45").is_empty());

        // Recorded from Rocrail's slot server: slot 1 free, address 0. The loco stays,
        // and the node does not ask again.
        apply(&mut table, "E7 0E 01 00 00 00 01 07 00 00 00 00 00 11");
        assert_eq!(table.locos()[0].label(), "loco ? (address unknown)");
        assert!(apply(&mut table, "A0 01 29 77").is_empty());

        // A slot server answers: slot 1 in use, address 3, speed 40, forward, F1 F2.
        apply(&mut table, "E7 0E 01 33 03 29 03 07 00 00 00 00 00 0A");
        assert_eq!(table.locos()[0].label(), "loco 3");
        assert_eq!(table.locos()[0].speed_label(), "speed 40");
    }

    #[test]
    fn freeing_a_slot_removes_the_loco() {
        let mut table = LocoTable::new("a");
        apply(&mut table, "BF 00 03 43");
        apply(&mut table, "BA 01 01 45");
        assert_eq!(table.locos().len(), 1);

        // SLOT_STAT1 slot 1, status free.
        apply(&mut table, "B5 01 03 48");
        assert!(table.locos().is_empty());
        // The freed slot is handed out again.
        assert_eq!(
            apply(&mut table, "BF 00 0A 4A"),
            ["E7 0E 01 03 0A 00 00 07 00 00 00 00 00 19"]
        );
    }

    #[test]
    fn ignores_the_fast_clock_and_dispatch_slots() {
        let mut table = LocoTable::new("a");
        assert!(apply(&mut table, "A0 00 10 4F").is_empty());
        assert!(apply(&mut table, "A0 7B 10 34").is_empty());
        assert!(table.locos().is_empty());
    }

    /// A FREMO session over three modules: the FRED takes loco 1234 at module B, drives
    /// it, is unplugged and plugged in again at module C, where it asks for its slot.
    #[test]
    fn a_fred_keeps_its_slot_across_modules() {
        // Module A has the lowest id, so it is the allocator.
        let mut a = LocoTable::new("a");
        let mut b = LocoTable::new("b");
        let mut c = LocoTable::new("c");

        // At B: LOCO_ADR 1234. B is not the allocator, so it asks A and waits.
        let asked = send(&mut b, "BF 09 52 1B", false);
        assert!(asked.replies.is_empty());
        assert_eq!(asked.request, Some(1234));
        assert!(b.is_pending(1234));
        // The throttle retries before the slot arrives: no second request.
        assert_eq!(send(&mut b, "BF 09 52 1B", false).request, None);

        // A assigns slot 1, driven from B, and publishes it; every node merges it.
        let assigned = a.allocate_for(1234, "b").unwrap();
        assert_eq!((assigned.slot, assigned.node.as_str()), (1, "b"));
        assert_eq!(
            hexes(&b.merge(vec![assigned.clone()], TRACK_ON)),
            ["E7 0E 01 03 52 00 00 07 00 09 00 00 00 48"]
        );
        assert!(!b.is_pending(1234));
        assert!(c.merge(vec![assigned], TRACK_ON).is_empty());

        // At B: null move and speed 40 forward; the changes go to A and C.
        let mut changes = send(&mut b, "BA 01 01 45", false).publish;
        changes.extend(send(&mut b, "A0 01 29 77", false).publish);
        a.merge(changes.clone(), TRACK_ON);
        c.merge(changes, TRACK_ON);
        assert_eq!(b.locos()[0].label(), "loco 1234");
        assert!(c.locos().is_empty());

        // Plugged in at C, the FRED asks for slot 1 and gets loco 1234 at speed 40.
        assert_eq!(
            hexes(&send(&mut c, "BB 01 00 45", false).replies),
            ["E7 0E 01 33 52 29 00 07 00 09 00 00 00 51"]
        );
        // Driving from C moves the loco to C on every node.
        let moved = send(&mut c, "A0 01 33 6D", false).publish;
        b.merge(moved.clone(), TRACK_ON);
        assert_eq!(c.locos()[0].speed_label(), "speed 50");
        assert!(b.locos().is_empty());
        // A second LOCO_ADR 1234 anywhere gets the same slot, without asking A.
        let again = send(&mut a, "BF 09 52 1B", true);
        assert!(again.request.is_none());
        assert!(again.replies[0].to_hex().starts_with("E7 0E 01"));
    }

    #[test]
    fn free_slots_are_answered_and_checksums_follow_the_table() {
        let mut a = LocoTable::new("a");
        // An empty slot reads as free, so JMRI's slot monitor can read every slot.
        assert_eq!(
            apply(&mut a, "BB 05 00 41"),
            ["E7 0E 05 03 00 00 00 07 00 00 00 00 00 17"]
        );

        let mut b = LocoTable::new("b");
        assert_eq!(a.checksum(), b.checksum());
        let assigned = send(&mut a, "BF 00 03 43", true).publish;
        assert_ne!(a.checksum(), b.checksum());
        b.merge(assigned, TRACK_ON);
        assert_eq!(a.checksum(), b.checksum());
        assert_eq!(a.snapshot()[0].status_label(), "assigned");
    }

    #[test]
    fn newer_versions_win_and_late_nodes_get_a_snapshot() {
        let mut a = LocoTable::new("a");
        apply(&mut a, "BF 00 03 43");
        apply(&mut a, "BA 01 01 45");

        // A node that comes online later gets the whole table.
        let mut d = LocoTable::new("d");
        d.merge(a.snapshot(), TRACK_ON);
        assert_eq!(
            hexes(&send(&mut d, "BB 01 00 45", false).replies),
            hexes(&send(&mut a, "BB 01 00 45", true).replies)
        );

        // An older copy does not overwrite a newer one.
        let old = a.snapshot();
        apply(&mut a, "A0 01 29 77");
        a.merge(old, TRACK_ON);
        assert_eq!(a.locos()[0].speed_label(), "speed 40");
    }
}
