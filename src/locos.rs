//! Locomotives driven through this node's LocoNet.
//!
//! The node is the command station for its district, so it answers the slot requests
//! of LocoNet throttles and Rocrail: `LOCO_ADR` gives a loco address its slot, and the
//! later speed and function messages for that slot belong to that address. A slot that
//! was taken before this node started has no known address until a client reads or
//! writes it again.

use crate::loconet::LocoNetMessage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

#[derive(Debug, Clone)]
struct Slot {
    status: u8,
    address: Option<u16>,
    speed: u8,
    dirf: u8,
    /// Whether this node already asked the LocoNet for the slot's unknown address.
    asked: bool,
}

#[derive(Debug, Default)]
pub struct LocoTable {
    slots: BTreeMap<u8, Slot>,
}

impl LocoTable {
    /// Updates the slots from one LocoNet message and returns the command station's
    /// replies, which go to every LocoNet client.
    pub fn apply(&mut self, message: &LocoNetMessage, track_status: u8) -> Vec<LocoNetMessage> {
        let b = message.as_bytes();
        match message.opcode() {
            OPC_LOCO_ADR => {
                let address = u16::from(b[1]) << 7 | u16::from(b[2]);
                match self.slot_for(address) {
                    Some(slot) => vec![self.slot_read(slot, track_status)],
                    None => vec![LocoNetMessage::long_ack(OPC_LOCO_ADR, ACK_NO_FREE_SLOT)],
                }
            }
            // Only slots this node can describe are answered; another slot master may know more.
            OPC_RQ_SL_DATA if self.address_known(b[1]) => {
                vec![self.slot_read(b[1], track_status)]
            }
            // A null move (source and destination equal) marks the slot in use.
            OPC_MOVE_SLOTS if b[1] == b[2] && self.address_known(b[1]) => {
                self.in_use(b[1]);
                vec![self.slot_read(b[1], track_status)]
            }
            OPC_LOCO_SPD if is_loco_slot(b[1]) => {
                self.in_use(b[1]).speed = b[2];
                self.ask_address(b[1])
            }
            OPC_LOCO_DIRF if is_loco_slot(b[1]) => {
                self.in_use(b[1]).dirf = b[2];
                self.ask_address(b[1])
            }
            OPC_SLOT_STAT1 if is_loco_slot(b[1]) => {
                if b[2] & SLOT_BUSY_MASK == 0 {
                    self.slots.remove(&b[1]);
                } else {
                    self.in_use(b[1]).status = b[2];
                }
                Vec::new()
            }
            // Slot data: op, 0E, slot, stat, adr, spd, dirf, trk, ss2, adr2, snd, id1, id2, chk.
            OPC_SL_RD_DATA | OPC_WR_SL_DATA if b.len() == 14 && is_loco_slot(b[2]) => {
                if b[3] & SLOT_BUSY_MASK == 0 {
                    // Rocrail's slot server answers a slot it drives as free with address 0;
                    // only a client's own write releases a slot.
                    if message.opcode() == OPC_WR_SL_DATA {
                        self.slots.remove(&b[2]);
                    }
                } else {
                    self.slots.insert(
                        b[2],
                        Slot {
                            status: b[3],
                            address: Some(u16::from(b[4]) | u16::from(b[9]) << 7),
                            speed: b[5],
                            dirf: b[6],
                            asked: false,
                        },
                    );
                }
                if message.opcode() == OPC_WR_SL_DATA {
                    vec![LocoNetMessage::long_ack(OPC_WR_SL_DATA, ACK_ACCEPTED)]
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    /// Locos in use, by slot.
    pub fn locos(&self) -> Vec<LocoInfo> {
        self.slots
            .iter()
            .filter(|(_, slot)| slot.status & SLOT_BUSY_MASK == SLOT_IN_USE)
            .map(|(&number, slot)| LocoInfo {
                slot: number,
                address: slot.address,
                speed: slot.speed,
                forward: slot.dirf & DIRF_REVERSE == 0,
                functions: u8::from(slot.dirf & DIRF_F0 != 0) | (slot.dirf & 0x0F) << 1,
            })
            .collect()
    }

    /// The slot that holds this address, or a newly reserved free one.
    fn slot_for(&mut self, address: u16) -> Option<u8> {
        if let Some((&number, _)) = self
            .slots
            .iter()
            .find(|(_, slot)| slot.address == Some(address))
        {
            return Some(number);
        }
        let number = (1..=LAST_LOCO_SLOT).find(|number| !self.slots.contains_key(number))?;
        self.slots.insert(
            number,
            Slot {
                status: SLOT_128_STEPS,
                address: Some(address),
                speed: 0,
                dirf: 0,
                asked: false,
            },
        );
        Some(number)
    }

    fn address_known(&self, number: u8) -> bool {
        self.slots
            .get(&number)
            .is_some_and(|slot| slot.address.is_some())
    }

    /// Once per slot taken before this node started: asks the LocoNet for its contents.
    /// A slot master such as Rocrail's slot server answers with `SL_RD_DATA`, which
    /// gives the address; without one nothing answers.
    fn ask_address(&mut self, number: u8) -> Vec<LocoNetMessage> {
        match self.slots.get_mut(&number) {
            Some(slot) if slot.address.is_none() && !slot.asked => {
                slot.asked = true;
                vec![LocoNetMessage::request_slot_data(number)]
            }
            _ => Vec::new(),
        }
    }

    /// A slot that a client drives is in use, even if this node never assigned it.
    fn in_use(&mut self, number: u8) -> &mut Slot {
        let slot = self.slots.entry(number).or_insert(Slot {
            status: SLOT_128_STEPS,
            address: None,
            speed: 0,
            dirf: 0,
            asked: false,
        });
        slot.status |= SLOT_IN_USE;
        slot
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
    use super::LocoTable;
    use crate::loconet::LocoNetMessage;

    const TRACK_ON: u8 = 0x07;

    fn apply(table: &mut LocoTable, hex: &str) -> Vec<String> {
        table
            .apply(&LocoNetMessage::from_hex(hex).unwrap(), TRACK_ON)
            .iter()
            .map(LocoNetMessage::to_hex)
            .collect()
    }

    #[test]
    fn throttle_request_gives_the_address_a_slot() {
        let mut table = LocoTable::default();
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
        let mut table = LocoTable::default();
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
        let mut table = LocoTable::default();
        apply(&mut table, "BF 00 03 43");
        apply(&mut table, "BA 01 01 45");
        assert_eq!(table.locos().len(), 1);

        // SLOT_STAT1 slot 1, status free.
        apply(&mut table, "B5 01 03 48");
        assert!(table.locos().is_empty());
    }

    #[test]
    fn ignores_the_fast_clock_and_dispatch_slots() {
        let mut table = LocoTable::default();
        assert!(apply(&mut table, "A0 00 10 4F").is_empty());
        assert!(apply(&mut table, "A0 7B 10 34").is_empty());
        assert!(table.locos().is_empty());
    }
}
