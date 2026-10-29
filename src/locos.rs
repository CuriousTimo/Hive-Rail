//! The layout's loco register and the LocoNet slots at each edge.
//!
//! **Loco register** (the mesh): every loco on the layout, by DCC address, with its speed,
//! direction and functions, and the node whose LocoNet drives it. Every node keeps a copy;
//! a change is published as a [`RegisteredLoco`] with a higher version, and the newest
//! version wins (ties go to the higher node id). Only loco addresses and state travel on
//! the mesh, never a local slot number.
//!
//! **Throttle numbers**: a loco driven from a throttle bus (FREDs and simple throttles,
//! where the node is the only slot master) also gets a throttle number, the slot number
//! that throttle sees. A FRED remembers that number and asks for it again when it is
//! plugged in elsewhere, so throttle numbers belong to the whole layout. One **allocator**,
//! the online node with the lowest id, hands them out.
//!
//! **Slots at the edge** ([`LocoNetEdge`]): each LocoNet connection either has the node
//! as its slot master ([`SlotOwner::Node`]: slots are throttle numbers) or has its own slot
//! master ([`SlotOwner::External`]: Rocrail's slot server, an Intellibox, a DR5000, a
//! Digitrax command station). There the node answers no slot requests; it learns which
//! local slot holds which loco from the slot master's replies and translates.

use crate::loconet::LocoNetMessage;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const OPC_LOCO_SPD: u8 = 0xA0;
const OPC_LOCO_DIRF: u8 = 0xA1;
const OPC_LOCO_SND: u8 = 0xA2;
const OPC_SLOT_STAT1: u8 = 0xB5;
const OPC_MOVE_SLOTS: u8 = 0xBA;
const OPC_RQ_SL_DATA: u8 = 0xBB;
const OPC_LOCO_ADR: u8 = 0xBF;
const OPC_SL_RD_DATA: u8 = 0xE7;
const OPC_WR_SL_DATA: u8 = 0xEF;
/// Slots 1 to 119 hold locos; 0 is dispatch and 120 and up are special slots.
const LAST_LOCO_SLOT: u8 = 119;
/// Slot status bits 4 and 5: `00` free, `01` common, `10` idle, `11` in use.
const SLOT_BUSY_MASK: u8 = 0x30;
const SLOT_IN_USE: u8 = 0x30;
/// Still refreshed by the slot master, but no throttle holds it.
const SLOT_COMMON: u8 = 0x10;
/// 128 speed steps, the decoder type Rocrail and JMRI expect by default.
const SLOT_128_STEPS: u8 = 0x03;
const DIRF_REVERSE: u8 = 0x20;
const DIRF_F0: u8 = 0x10;
/// `LONG_ACK` code for "no free slot".
const ACK_NO_FREE_SLOT: u8 = 0x00;
const ACK_ACCEPTED: u8 = 0x7F;
/// A slot master purges a slot nobody touched for about 200 s (LocoNet PE); a speed
/// update about every 100 s keeps a held slot ours.
const KEEP_ALIVE: Duration = Duration::from_secs(100);

/// One loco as shared with the other nodes in the heartbeat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocoInfo {
    /// The throttle number, or 0 for a loco driven without one (kept for older nodes).
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

/// One loco in the register, as shared over the mesh on `hive/{session}/locos/update`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisteredLoco {
    pub address: u16,
    /// The throttle number for throttle buses, handed out by the allocator.
    pub throttle: Option<u8>,
    /// LocoNet slot status: busy bits and decoder type.
    pub status: u8,
    pub speed: u8,
    pub dirf: u8,
    /// F5-F8 in bits 0-3, as in `OPC_LOCO_SND`.
    pub snd: u8,
    pub version: u64,
    /// The node whose LocoNet drives the loco.
    pub node: String,
    /// The node whose command station powers the loco: the district it is in.
    #[serde(default)]
    pub district: Option<String>,
}

impl RegisteredLoco {
    pub fn new(address: u16, node: &str) -> Self {
        Self {
            address,
            throttle: None,
            status: SLOT_128_STEPS,
            speed: 0,
            dirf: 0,
            snd: 0,
            version: 0,
            node: node.to_owned(),
            district: None,
        }
    }

    fn newer_than(&self, other: &RegisteredLoco) -> bool {
        (self.version, &self.node) > (other.version, &other.node)
    }

    pub fn in_use(&self) -> bool {
        self.status & SLOT_BUSY_MASK == SLOT_IN_USE
    }

    /// LocoNet slot status in words. A loco not in use with a throttle number is
    /// assigned but not yet taken; without one it was released.
    pub fn status_label(&self) -> &'static str {
        match (self.status & SLOT_BUSY_MASK, self.throttle) {
            (0x00, Some(_)) => "assigned",
            (0x00, None) => "released",
            (0x10, _) => "common",
            (0x20, _) => "idle",
            _ => "in use",
        }
    }

    pub fn loco(&self) -> LocoInfo {
        LocoInfo {
            slot: self.throttle.unwrap_or(0),
            address: Some(self.address),
            speed: self.speed,
            forward: self.dirf & DIRF_REVERSE == 0,
            functions: u8::from(self.dirf & DIRF_F0 != 0) | (self.dirf & 0x0F) << 1,
        }
    }

    /// Short form for the CLI log.
    pub fn describe(&self) -> String {
        let loco = self.loco();
        format!(
            "loco {}{} {}, {}, {} at {}{}",
            self.address,
            self.throttle
                .map_or(String::new(), |throttle| format!(" (throttle {throttle})")),
            self.status_label(),
            loco.speed_label(),
            loco.direction(),
            self.node,
            self.district
                .as_ref()
                .map_or(String::new(), |district| format!(", in {district}"))
        )
    }
}

/// A loco that changed: its state before (None if new) and after.
pub type Change = (Option<RegisteredLoco>, RegisteredLoco);

/// What merging an update from the mesh led to.
#[derive(Debug, Default)]
pub struct Merged {
    /// Locos whose state changed, to tell this node's LocoNet connections.
    pub changes: Vec<Change>,
    /// Addresses this node asked a throttle number for, which now have one.
    pub answered: Vec<u16>,
}

#[derive(Debug)]
pub struct LocoRegister {
    node: String,
    locos: BTreeMap<u16, RegisteredLoco>,
    /// Addresses this node asked the allocator for; their throttles wait for the number.
    pending: BTreeSet<u16>,
}

impl LocoRegister {
    pub fn new(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            locos: BTreeMap::new(),
            pending: BTreeSet::new(),
        }
    }

    pub fn get(&self, address: u16) -> Option<&RegisteredLoco> {
        self.locos.get(&address)
    }

    pub fn by_throttle(&self, throttle: u8) -> Option<&RegisteredLoco> {
        self.locos
            .values()
            .find(|loco| loco.throttle == Some(throttle))
    }

    /// Changes a loco on this node's behalf. Returns the change, or None when nothing
    /// changed (throttles repeat their messages).
    pub fn update(
        &mut self,
        address: u16,
        change: impl FnOnce(&mut RegisteredLoco),
    ) -> Option<Change> {
        let before = self.locos.get(&address).cloned();
        let node = self.node.clone();
        let loco = self
            .locos
            .entry(address)
            .or_insert_with(|| RegisteredLoco::new(address, &node));
        change(loco);
        loco.node = node;
        if before.as_ref() == Some(&*loco) {
            return None;
        }
        loco.version += 1;
        Some((before, loco.clone()))
    }

    /// Applies locos published by other nodes.
    pub fn merge(&mut self, locos: Vec<RegisteredLoco>) -> Merged {
        let mut merged = Merged::default();
        for loco in locos {
            match self.locos.get(&loco.address) {
                Some(local) if !loco.newer_than(local) => {}
                before => {
                    let before = before.cloned();
                    self.locos.insert(loco.address, loco.clone());
                    merged.changes.push((before, loco));
                }
            }
        }
        merged.answered = self
            .pending
            .iter()
            .copied()
            .filter(|address| {
                self.get(*address)
                    .is_some_and(|loco| loco.throttle.is_some())
            })
            .collect();
        for address in &merged.answered {
            self.pending.remove(address);
        }
        merged
    }

    /// On the allocator: a throttle number for this loco, driven from `node`. Returns
    /// the loco to publish, or None when every throttle number is taken.
    pub fn allocate_for(&mut self, address: u16, node: &str) -> Option<RegisteredLoco> {
        if let Some(loco) = self.get(address).filter(|loco| loco.throttle.is_some()) {
            return Some(loco.clone());
        }
        let used: BTreeSet<u8> = self
            .locos
            .values()
            .filter_map(|loco| loco.throttle)
            .collect();
        let throttle = (1..=LAST_LOCO_SLOT).find(|number| !used.contains(number))?;
        let loco = self
            .locos
            .entry(address)
            .or_insert_with(|| RegisteredLoco::new(address, node));
        loco.throttle = Some(throttle);
        loco.node = node.to_owned();
        loco.version += 1;
        Some(loco.clone())
    }

    /// Remembers that this node asked the allocator; false if it already did.
    pub fn request(&mut self, address: u16) -> bool {
        self.pending.insert(address)
    }

    pub fn is_pending(&self, address: u16) -> bool {
        self.pending.contains(&address)
    }

    /// Gives up waiting for the allocator; the throttle will ask again.
    pub fn cancel(&mut self, address: u16) {
        self.pending.remove(&address);
    }

    /// Every loco this node knows, for a node or dispatcher that just came online.
    pub fn snapshot(&self) -> Vec<RegisteredLoco> {
        self.locos.values().cloned().collect()
    }

    /// A short fingerprint of the whole register, shared in the heartbeat so nodes and the
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
        for loco in self.locos.values() {
            add(&loco.address.to_le_bytes());
            add(&[
                loco.throttle.unwrap_or(0),
                loco.status,
                loco.speed,
                loco.dirf,
                loco.snd,
            ]);
            add(&loco.version.to_le_bytes());
            add(loco.node.as_bytes());
            add(&[0]);
            add(loco.district.as_deref().unwrap_or("").as_bytes());
            add(&[0]);
        }
        hash
    }

    /// Locos in use that are driven through this node's LocoNet.
    pub fn locos(&self) -> Vec<LocoInfo> {
        self.locos
            .values()
            .filter(|loco| loco.in_use() && loco.node == self.node)
            .map(RegisteredLoco::loco)
            .collect()
    }

    /// Locos in use on the whole layout.
    pub fn taken(&self) -> usize {
        self.locos.values().filter(|loco| loco.in_use()).count()
    }

    /// The loco most recently driven through this node's LocoNet.
    pub fn last_driven(&self) -> Option<LocoInfo> {
        self.locos
            .values()
            .filter(|loco| loco.in_use() && loco.node == self.node)
            .max_by_key(|loco| loco.version)
            .map(RegisteredLoco::loco)
    }
}

/// Who owns the slot numbers on one LocoNet connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotOwner {
    /// The node is the only slot master; slots are the layout's throttle numbers. For a
    /// throttle bus with FREDs, simple throttles and the simulation.
    Node,
    /// A slot master on the connection owns its slots (Rocrail's slot server, an
    /// Intellibox, a DR5000, a Digitrax command station); the node translates.
    External,
}

impl SlotOwner {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "node" => Some(Self::Node),
            "external" => Some(Self::External),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::External => "external",
        }
    }
}

/// What one LocoNet message from a connection leads to.
#[derive(Debug, Default)]
pub struct EdgeOutcome {
    /// Replies for the same connection.
    pub replies: Vec<LocoNetMessage>,
    /// Changed locos to publish to the other nodes.
    pub publish: Vec<RegisteredLoco>,
    /// A loco address that needs a throttle number from the allocator.
    pub request: Option<u16>,
    /// Locos whose state changed, to tell this node's other LocoNet connection.
    pub changes: Vec<Change>,
    /// Locos this node took on the slot master: address and slot.
    pub acquired: Vec<(u16, u8)>,
    /// Locos the node wanted to take but found IN_USE on the slot master; the LocoNet
    /// PE says not to use such a slot.
    pub conflicts: Vec<u16>,
}

impl EdgeOutcome {
    fn changed(&mut self, change: Option<Change>) {
        if let Some((before, after)) = change {
            self.publish.push(after.clone());
            self.changes.push((before, after));
        }
    }
}

/// The slots of one LocoNet connection and their translation to the register.
#[derive(Debug)]
pub struct LocoNetEdge {
    owner: SlotOwner,
    /// External only: local slot number to loco address, learned from the slot master.
    local: BTreeMap<u8, u16>,
    /// External only: the slot master's status byte per local slot, for its speed steps.
    status: BTreeMap<u8, u8>,
    /// External only: slots driven before their address was known, already asked about.
    asked: BTreeSet<u8>,
    /// External only: locos this node asked the slot master for, to take them.
    acquiring: BTreeSet<u16>,
    /// External only: locos this node holds like a throttle, and when it last touched them.
    held: BTreeMap<u16, Instant>,
}

impl LocoNetEdge {
    pub fn new(owner: SlotOwner) -> Self {
        Self {
            owner,
            local: BTreeMap::new(),
            status: BTreeMap::new(),
            asked: BTreeSet::new(),
            acquiring: BTreeSet::new(),
            held: BTreeMap::new(),
        }
    }

    /// Takes a loco on this connection's slot master, as a throttle does (LocoNet PE,
    /// "Standard Address Selection"): ask for the address; the slot master's answer is
    /// handled in `receive`, which takes the slot with a null move unless it is IN_USE.
    /// The slot master then refreshes the loco on its track.
    pub fn acquire(&mut self, address: u16) -> Vec<LocoNetMessage> {
        if self.owner != SlotOwner::External || self.held.contains_key(&address) {
            return Vec::new();
        }
        self.acquiring.insert(address);
        vec![LocoNetMessage::loco_address(address)]
    }

    /// Releases a loco this node holds: its slot becomes COMMON, so the slot master keeps
    /// refreshing it and a train across the district border keeps running. Only the busy
    /// bits change; the slot master's speed steps for the loco stay as they are.
    pub fn release(&mut self, address: u16) -> Vec<LocoNetMessage> {
        self.acquiring.remove(&address);
        if self.held.remove(&address).is_none() {
            return Vec::new();
        }
        self.slot_of(address)
            .map(|slot| {
                let status = self
                    .status
                    .get(&slot)
                    .map_or(SLOT_128_STEPS, |status| status & !SLOT_BUSY_MASK);
                LocoNetMessage::slot_status(slot, status | SLOT_COMMON)
            })
            .into_iter()
            .collect()
    }

    #[cfg(test)]
    pub fn holds(&self, address: u16) -> bool {
        self.held.contains_key(&address)
    }

    /// Speed updates for held slots nobody touched for 100 s, so the slot master does
    /// not purge them.
    pub fn keep_alive(&mut self, register: &LocoRegister, now: Instant) -> Vec<LocoNetMessage> {
        let mut messages = Vec::new();
        let due: Vec<u16> = self
            .held
            .iter()
            .filter(|(_, last)| now.duration_since(**last) >= KEEP_ALIVE)
            .map(|(&address, _)| address)
            .collect();
        for address in due {
            self.held.insert(address, now);
            if let (Some(slot), Some(loco)) = (self.slot_of(address), register.get(address)) {
                messages.push(LocoNetMessage::loco_speed(slot, loco.speed));
            }
        }
        messages
    }

    fn slot_of(&self, address: u16) -> Option<u8> {
        self.local
            .iter()
            .find(|(_, &local)| local == address)
            .map(|(&slot, _)| slot)
    }

    pub fn owner(&self) -> SlotOwner {
        self.owner
    }

    /// Handles one message from this connection. `allocator` says whether this node
    /// hands out throttle numbers itself.
    pub fn receive(
        &mut self,
        register: &mut LocoRegister,
        message: &LocoNetMessage,
        track_status: u8,
        allocator: bool,
    ) -> EdgeOutcome {
        match self.owner {
            SlotOwner::Node => receive_as_master(register, message, track_status, allocator),
            SlotOwner::External => self.receive_from_master(register, message),
        }
    }

    /// Follows the slot master of this connection: what its slots hold, and what its
    /// throttles do with them.
    fn receive_from_master(
        &mut self,
        register: &mut LocoRegister,
        message: &LocoNetMessage,
    ) -> EdgeOutcome {
        let b = message.as_bytes();
        let mut outcome = EdgeOutcome::default();
        match message.opcode() {
            OPC_LOCO_SPD | OPC_LOCO_DIRF | OPC_LOCO_SND if is_loco_slot(b[1]) => {
                match self.local.get(&b[1]) {
                    Some(&address) => {
                        let (opcode, value) = (message.opcode(), b[2]);
                        outcome.changed(register.update(address, |loco| {
                            loco.status |= SLOT_IN_USE;
                            set_drive(loco, opcode, value);
                        }));
                    }
                    // A slot taken before the node started: ask the slot master once.
                    None => {
                        if self.asked.insert(b[1]) {
                            outcome
                                .replies
                                .push(LocoNetMessage::request_slot_data(b[1]));
                        }
                    }
                }
            }
            // Slot data: op, 0E, slot, stat, adr, spd, dirf, trk, ss2, adr2, snd, id1, id2, chk.
            OPC_SL_RD_DATA | OPC_WR_SL_DATA if b.len() == 14 && is_loco_slot(b[2]) => {
                let address = u16::from(b[4]) | u16::from(b[9]) << 7;
                if address != 0 {
                    // Also the slot master's answer to LOCO_ADR, before the slot is taken.
                    let known = self.local.insert(b[2], address) == Some(address);
                    self.status.insert(b[2], b[3]);
                    self.asked.remove(&b[2]);
                    if self.acquiring.remove(&address) {
                        // Only a slot a throttle here already drives is in use: Rocrail's
                        // slot server answers a new LOCO_ADR with IN_USE, unlike the PE.
                        if known && b[3] & SLOT_BUSY_MASK == SLOT_IN_USE {
                            outcome.conflicts.push(address);
                        } else {
                            // Take the slot, then give it the loco's state from the register.
                            self.held.insert(address, Instant::now());
                            outcome.replies.push(LocoNetMessage::null_move(b[2]));
                            if let Some(loco) = register.get(address) {
                                outcome.replies.extend([
                                    LocoNetMessage::loco_speed(b[2], loco.speed),
                                    LocoNetMessage::loco_dirf(b[2], loco.dirf),
                                    LocoNetMessage::loco_sound(b[2], loco.snd),
                                ]);
                            }
                            outcome.acquired.push((address, b[2]));
                        }
                    } else if !self.held.contains_key(&address) {
                        outcome.changed(register.update(address, |loco| {
                            loco.status = b[3];
                            loco.speed = b[5];
                            loco.dirf = b[6];
                            loco.snd = b[10] & 0x0F;
                        }));
                    }
                    // A held slot's data from the slot master (speed 0 after the null move)
                    // does not overwrite the register: the register leads for a held loco.
                } else if message.opcode() == OPC_WR_SL_DATA && b[3] & SLOT_BUSY_MASK == 0 {
                    // Rocrail's slot server reads a slot it drives as free with address 0;
                    // only a client's own write releases a slot.
                    self.forget_slot(register, b[2], b[3], &mut outcome);
                }
            }
            OPC_SLOT_STAT1 if is_loco_slot(b[1]) && b[2] & SLOT_BUSY_MASK == 0 => {
                self.forget_slot(register, b[1], b[2], &mut outcome);
            }
            _ => {}
        }
        outcome
    }

    /// The slot master freed a slot: the loco is no longer in use here.
    fn forget_slot(
        &mut self,
        register: &mut LocoRegister,
        slot: u8,
        status: u8,
        outcome: &mut EdgeOutcome,
    ) {
        self.status.remove(&slot);
        if let Some(address) = self.local.remove(&slot) {
            self.held.remove(&address);
            outcome.changed(register.update(address, |loco| loco.status = status));
        }
    }

    /// This connection's number for a loco: its throttle number, or the local slot the
    /// slot master gave it.
    fn number_for(&self, loco: &RegisteredLoco) -> Option<u8> {
        match self.owner {
            SlotOwner::Node => loco.throttle,
            SlotOwner::External => self.slot_of(loco.address),
        }
    }

    /// A loco changed elsewhere (the other connection or another node): the messages
    /// that tell this connection, in its own slot numbers. Nothing if the connection
    /// does not know the loco.
    pub fn notify(
        &self,
        before: Option<&RegisteredLoco>,
        after: &RegisteredLoco,
    ) -> Vec<LocoNetMessage> {
        let Some(slot) = self.number_for(after) else {
            return Vec::new();
        };
        let mut messages = Vec::new();
        if before.is_none_or(|before| before.speed != after.speed) {
            messages.push(LocoNetMessage::loco_speed(slot, after.speed));
        }
        if before.is_none_or(|before| before.dirf != after.dirf) {
            messages.push(LocoNetMessage::loco_dirf(slot, after.dirf));
        }
        if before.is_none_or(|before| before.snd != after.snd) {
            messages.push(LocoNetMessage::loco_sound(slot, after.snd));
        }
        messages
    }

    /// For a throttle that waited for a throttle number: its slot data, now it has one.
    pub fn answer(
        &self,
        register: &LocoRegister,
        address: u16,
        track_status: u8,
    ) -> Option<LocoNetMessage> {
        let throttle = register.get(address)?.throttle?;
        (self.owner == SlotOwner::Node).then(|| slot_read(register, throttle, track_status))
    }
}

/// The node as slot master: slots are the layout's throttle numbers.
fn receive_as_master(
    register: &mut LocoRegister,
    message: &LocoNetMessage,
    track_status: u8,
    allocator: bool,
) -> EdgeOutcome {
    let b = message.as_bytes();
    let mut outcome = EdgeOutcome::default();
    let by_throttle =
        |register: &LocoRegister, number: u8| register.by_throttle(number).map(|loco| loco.address);
    match message.opcode() {
        OPC_LOCO_ADR => {
            let address = u16::from(b[1]) << 7 | u16::from(b[2]);
            if let Some(throttle) = register.get(address).and_then(|loco| loco.throttle) {
                outcome
                    .replies
                    .push(slot_read(register, throttle, track_status));
            } else if allocator {
                let node = register.node.clone();
                match register.allocate_for(address, &node) {
                    Some(loco) => {
                        let throttle = loco.throttle.unwrap_or(0);
                        outcome
                            .replies
                            .push(slot_read(register, throttle, track_status));
                        outcome.publish.push(loco);
                    }
                    None => outcome
                        .replies
                        .push(LocoNetMessage::long_ack(OPC_LOCO_ADR, ACK_NO_FREE_SLOT)),
                }
            } else if register.request(address) {
                outcome.request = Some(address);
            }
        }
        // Throttle numbers in use are answered from the register, and free ones too, so a
        // slot monitor such as JMRI's can read every slot.
        OPC_RQ_SL_DATA if is_loco_slot(b[1]) => {
            outcome.replies.push(match by_throttle(register, b[1]) {
                Some(_) => slot_read(register, b[1], track_status),
                None => {
                    LocoNetMessage::loco_slot_read(b[1], SLOT_128_STEPS, 0, 0, 0, track_status, 0)
                }
            });
        }
        // A null move (source and destination equal) marks the slot in use.
        OPC_MOVE_SLOTS if b[1] == b[2] => {
            if let Some(address) = by_throttle(register, b[1]) {
                outcome.changed(register.update(address, |loco| loco.status |= SLOT_IN_USE));
                outcome
                    .replies
                    .push(slot_read(register, b[1], track_status));
            }
        }
        // Speed, direction and functions; every change is answered with the new slot data,
        // as throttles build their next message from the slot data they last got.
        OPC_LOCO_SPD | OPC_LOCO_DIRF | OPC_LOCO_SND if is_loco_slot(b[1]) => {
            if let Some(address) = by_throttle(register, b[1]) {
                let (opcode, value) = (message.opcode(), b[2]);
                let change = register.update(address, |loco| {
                    loco.status |= SLOT_IN_USE;
                    set_drive(loco, opcode, value);
                });
                if change.is_some() {
                    outcome
                        .replies
                        .push(slot_read(register, b[1], track_status));
                }
                outcome.changed(change);
            }
        }
        OPC_SLOT_STAT1 if is_loco_slot(b[1]) => {
            if let Some(address) = by_throttle(register, b[1]) {
                let status = b[2];
                outcome.changed(register.update(address, |loco| {
                    loco.status = status;
                    if status & SLOT_BUSY_MASK == 0 {
                        loco.throttle = None;
                    }
                }));
            }
        }
        // A client writing a slot, such as JMRI's slot monitor.
        OPC_WR_SL_DATA if b.len() == 14 && is_loco_slot(b[2]) => {
            if let Some(address) = by_throttle(register, b[2]) {
                outcome.changed(register.update(address, |loco| {
                    loco.status = b[3];
                    loco.speed = b[5];
                    loco.dirf = b[6];
                    loco.snd = b[10] & 0x0F;
                    if b[3] & SLOT_BUSY_MASK == 0 {
                        loco.throttle = None;
                    }
                }));
            }
            outcome
                .replies
                .push(LocoNetMessage::long_ack(OPC_WR_SL_DATA, ACK_ACCEPTED));
        }
        _ => {}
    }
    outcome
}

fn set_drive(loco: &mut RegisteredLoco, opcode: u8, value: u8) {
    match opcode {
        OPC_LOCO_SPD => loco.speed = value,
        OPC_LOCO_DIRF => loco.dirf = value,
        _ => loco.snd = value & 0x0F,
    }
}

fn slot_read(register: &LocoRegister, throttle: u8, track_status: u8) -> LocoNetMessage {
    let loco = register.by_throttle(throttle);
    LocoNetMessage::loco_slot_read(
        throttle,
        loco.map_or(SLOT_128_STEPS, |loco| loco.status),
        loco.map_or(0, |loco| loco.address),
        loco.map_or(0, |loco| loco.speed),
        loco.map_or(0, |loco| loco.dirf),
        track_status,
        loco.map_or(0, |loco| loco.snd),
    )
}

fn is_loco_slot(slot: u8) -> bool {
    (1..=LAST_LOCO_SLOT).contains(&slot)
}

#[cfg(test)]
mod tests {
    use super::{EdgeOutcome, LocoNetEdge, LocoRegister, SlotOwner};
    use crate::loconet::LocoNetMessage;
    use std::time::{Duration, Instant};

    const TRACK_ON: u8 = 0x07;

    fn send(
        edge: &mut LocoNetEdge,
        register: &mut LocoRegister,
        hex: &str,
        allocator: bool,
    ) -> EdgeOutcome {
        edge.receive(
            register,
            &LocoNetMessage::from_hex(hex).unwrap(),
            TRACK_ON,
            allocator,
        )
    }

    fn hexes(messages: &[LocoNetMessage]) -> Vec<String> {
        messages.iter().map(LocoNetMessage::to_hex).collect()
    }

    /// A throttle bus, where the node is the slot master.
    fn bus(node: &str) -> (LocoNetEdge, LocoRegister) {
        (LocoNetEdge::new(SlotOwner::Node), LocoRegister::new(node))
    }

    #[test]
    fn as_slot_master_a_throttle_gets_a_throttle_number() {
        let (mut edge, mut register) = bus("a");
        let mut replies = |hex: &str| hexes(&send(&mut edge, &mut register, hex, true).replies);
        // LOCO_ADR 3: throttle 1, free, 128 steps.
        assert_eq!(
            replies("BF 00 03 43"),
            ["E7 0E 01 03 03 00 00 07 00 00 00 00 00 10"]
        );
        // Null move: in use.
        assert_eq!(
            replies("BA 01 01 45"),
            ["E7 0E 01 33 03 00 00 07 00 00 00 00 00 20"]
        );
        // Recorded from Rocrail: speed 40, then reverse with F0 on; each change answered.
        replies("A0 01 29 77");
        assert_eq!(
            replies("A1 01 30 6F"),
            ["E7 0E 01 33 03 29 30 07 00 00 00 00 00 39"]
        );
        // A repeat changes nothing and is not answered.
        assert!(replies("A1 01 30 6F").is_empty());
        // The same address gets the same throttle number again; 1234 gets the next one.
        assert_eq!(
            replies("BF 00 03 43"),
            ["E7 0E 01 33 03 29 30 07 00 00 00 00 00 39"]
        );
        assert_eq!(
            replies("BF 09 52 1B"),
            ["E7 0E 02 03 52 00 00 07 00 09 00 00 00 4B"]
        );
        // An empty slot reads as free, so JMRI's slot monitor can read every slot.
        assert_eq!(
            replies("BB 05 00 41"),
            ["E7 0E 05 03 00 00 00 07 00 00 00 00 00 17"]
        );

        let locos = register.locos();
        assert_eq!(locos.len(), 1);
        assert_eq!(locos[0].label(), "loco 3");
        assert_eq!(locos[0].speed_label(), "speed 40");
        assert!(locos[0].function_on(0) && locos[0].direction() == "reverse");
    }

    #[test]
    fn as_slot_master_a_freed_slot_releases_the_throttle_number() {
        let (mut edge, mut register) = bus("a");
        send(&mut edge, &mut register, "BF 00 03 43", true);
        send(&mut edge, &mut register, "BA 01 01 45", true);
        // SLOT_STAT1 slot 1 free: the loco stays registered, without a throttle number.
        send(&mut edge, &mut register, "B5 01 03 48", true);
        assert!(register.locos().is_empty());
        assert_eq!(register.get(3).unwrap().status_label(), "released");
        // Throttle number 1 goes to the next loco.
        let replies = send(&mut edge, &mut register, "BF 00 0A 4A", true).replies;
        assert_eq!(
            hexes(&replies),
            ["E7 0E 01 03 0A 00 00 07 00 00 00 00 00 19"]
        );
    }

    /// Recorded from Rocrail with its own slot server on: Rocrail owns the slots on its
    /// connection; the node answers no slot request and follows them.
    #[test]
    fn follows_an_external_slot_master_and_answers_nothing() {
        let mut edge = LocoNetEdge::new(SlotOwner::External);
        let mut register = LocoRegister::new("mac");
        // Rocrail's LOCO_ADR 103 is not answered by the node.
        let request = send(&mut edge, &mut register, "BF 00 67 27", true);
        assert!(request.replies.is_empty() && request.request.is_none());
        // Rocrail's slot server answers: slot 1 holds loco 103, in use.
        let read = send(
            &mut edge,
            &mut register,
            "E7 0E 01 30 67 00 00 04 00 00 00 00 00 44",
            true,
        );
        assert!(read.replies.is_empty());
        assert_eq!(read.publish[0].address, 103);
        assert_eq!(read.publish[0].throttle, None);
        // F0 and F1 on slot 1 are loco 103's.
        send(&mut edge, &mut register, "A1 01 11 4E", true);
        let loco = register.get(103).unwrap().loco();
        assert!(loco.function_on(0) && loco.function_on(1));
        // A slot driven before the node knew it: asked once, the slot master answers.
        let unknown = send(&mut edge, &mut register, "A0 02 29 74", true);
        assert_eq!(hexes(&unknown.replies), ["BB 02 00 46"]);
        assert!(send(&mut edge, &mut register, "A0 02 29 74", true)
            .replies
            .is_empty());
        // A slot request from Rocrail is left to its slot master.
        assert!(send(&mut edge, &mut register, "BB 05 00 41", true)
            .replies
            .is_empty());
    }

    /// A central that keeps the loco at 28 steps (status bits 0 to 2 `000`), as an
    /// Intellibox may: releasing the slot makes it COMMON and keeps the 28 steps.
    #[test]
    fn releases_as_common_with_the_central_s_speed_steps() {
        let mut register = LocoRegister::new("MacBook");
        let mut edge = LocoNetEdge::new(SlotOwner::External);
        register.update(3, |loco| loco.district = Some("MacBook".into()));
        edge.acquire(3);
        // Slot 2, free, 28 steps; then its answer to the null move, in use.
        send(
            &mut edge,
            &mut register,
            "E7 0E 02 00 03 00 00 07 00 00 00 00 00 10",
            true,
        );
        send(
            &mut edge,
            &mut register,
            "E7 0E 02 30 03 00 00 07 00 00 00 00 00 20",
            true,
        );
        assert_eq!(hexes(&edge.release(3)), ["B5 02 10 58"]);
        // A loco no longer held is not released twice.
        assert!(edge.release(3).is_empty());
    }

    /// A loco transferred into this district is taken on the slot master (here Rocrail's
    /// slot server) like a throttle would, following the LocoNet PE.
    #[test]
    fn takes_a_loco_on_the_slot_master_and_keeps_it_alive() {
        let mut register = LocoRegister::new("mac");
        let mut edge = LocoNetEdge::new(SlotOwner::External);
        // Loco 3333 at speed 40, forward, lights on, now in this district.
        register.update(3333, |loco| {
            loco.speed = 0x29;
            loco.dirf = 0x10;
            loco.district = Some("mac".into());
        });
        assert_eq!(hexes(&edge.acquire(3333)), ["BF 1A 05 5F"]);
        // The slot master loads it into a new slot 4: speed 0, free.
        let taken = send(
            &mut edge,
            &mut register,
            "E7 0E 04 03 05 00 00 07 00 1A 00 00 00 09",
            true,
        );
        assert_eq!(taken.acquired, [(3333, 4)]);
        assert_eq!(
            hexes(&taken.replies),
            ["BA 04 04 45", "A0 04 29 72", "A1 04 10 4A", "A2 04 00 59"]
        );
        // Its answer to the null move (in use, speed 0) does not overwrite the register.
        send(
            &mut edge,
            &mut register,
            "E7 0E 04 33 05 00 00 07 00 1A 00 00 00 39",
            true,
        );
        assert_eq!(register.get(3333).unwrap().speed, 0x29);
        assert!(edge.holds(3333));

        // After 100 s without activity a speed update keeps the slot ours.
        let later = Instant::now() + Duration::from_secs(101);
        assert_eq!(hexes(&edge.keep_alive(&register, later)), ["A0 04 29 72"]);
        // Leaving the district makes the slot COMMON, keeping its 128 steps.
        assert_eq!(hexes(&edge.release(3333)), ["B5 04 13 5D"]);

        // Rocrail's slot server answers a new LOCO_ADR with IN_USE: still taken.
        let mut rocrail = LocoNetEdge::new(SlotOwner::External);
        rocrail.acquire(1234);
        let fresh = send(
            &mut rocrail,
            &mut register,
            "E7 0E 07 30 52 00 00 04 00 09 00 00 00 7E",
            true,
        );
        assert_eq!(fresh.acquired, [(1234, 7)]);

        // A loco a throttle here already drives in slot 7 is not taken.
        let mut busy = LocoNetEdge::new(SlotOwner::External);
        send(
            &mut busy,
            &mut register,
            "E7 0E 07 30 52 00 00 04 00 09 00 00 00 7E",
            true,
        );
        busy.acquire(1234);
        let refused = send(
            &mut busy,
            &mut register,
            "E7 0E 07 30 52 00 00 04 00 09 00 00 00 7E",
            true,
        );
        assert_eq!(refused.conflicts, [1234]);
        assert!(refused.replies.is_empty() && !busy.holds(1234));
    }

    /// Rocrail's slot 7 and the FRED's throttle 1 are the same loco 1234; the node
    /// translates between them in both directions.
    #[test]
    fn translates_between_an_external_slot_and_a_throttle_number() {
        let mut register = LocoRegister::new("mac");
        let mut rocrail = LocoNetEdge::new(SlotOwner::External);
        let (mut fred, _) = bus("mac");
        // Rocrail's slot server put loco 1234 in its slot 7.
        send(
            &mut rocrail,
            &mut register,
            "E7 0E 07 30 52 00 00 04 00 09 00 00 00 7E",
            true,
        );
        // A FRED on the throttle bus takes loco 1234: throttle number 1.
        send(&mut fred, &mut register, "BF 09 52 1B", true);
        send(&mut fred, &mut register, "BA 01 01 45", true);

        // Rocrail sets speed 40 on its slot 7: the FRED's bus hears it as throttle 1.
        let from_rocrail = send(&mut rocrail, &mut register, "A0 07 29 71", true);
        let (before, after) = &from_rocrail.changes[0];
        assert_eq!(hexes(&fred.notify(before.as_ref(), after)), ["A0 01 29 77"]);
        // The FRED switches F0 on throttle 1: Rocrail hears it on its slot 7.
        let from_fred = send(&mut fred, &mut register, "A1 01 10 4F", true);
        let (before, after) = &from_fred.changes[0];
        assert_eq!(
            hexes(&rocrail.notify(before.as_ref(), after)),
            ["A1 07 10 49"]
        );
    }

    /// A FREMO session over three modules with throttle buses: the FRED takes loco 1234
    /// at module B, drives it, and is plugged in again at module C.
    #[test]
    fn a_fred_keeps_its_throttle_number_across_modules() {
        // Module A has the lowest id, so it is the allocator.
        let mut a = LocoRegister::new("a");
        let (mut b_bus, mut b) = bus("b");
        let (mut c_bus, mut c) = bus("c");

        // At B: LOCO_ADR 1234. B is not the allocator, so it asks A and waits.
        let asked = send(&mut b_bus, &mut b, "BF 09 52 1B", false);
        assert!(asked.replies.is_empty());
        assert_eq!(asked.request, Some(1234));
        // The throttle retries before the number arrives: no second request.
        assert_eq!(send(&mut b_bus, &mut b, "BF 09 52 1B", false).request, None);

        // A assigns throttle 1, driven from B; every node merges it.
        let assigned = a.allocate_for(1234, "b").unwrap();
        assert_eq!(assigned.throttle, Some(1));
        let merged = b.merge(vec![assigned.clone()]);
        assert_eq!(merged.answered, [1234]);
        assert_eq!(
            hexes(&[b_bus.answer(&b, 1234, TRACK_ON).unwrap()]),
            ["E7 0E 01 03 52 00 00 07 00 09 00 00 00 48"]
        );
        c.merge(vec![assigned]);

        // At B: null move and speed 40; the changes go to A and C.
        let mut changes = send(&mut b_bus, &mut b, "BA 01 01 45", false).publish;
        changes.extend(send(&mut b_bus, &mut b, "A0 01 29 77", false).publish);
        a.merge(changes.clone());
        c.merge(changes);
        assert!(c.locos().is_empty());

        // Plugged in at C, the FRED asks for throttle 1 and gets loco 1234 at speed 40.
        assert_eq!(
            hexes(&send(&mut c_bus, &mut c, "BB 01 00 45", false).replies),
            ["E7 0E 01 33 52 29 00 07 00 09 00 00 00 51"]
        );
        // Driving from C moves the loco to C on every node.
        let moved = send(&mut c_bus, &mut c, "A0 01 33 6D", false).publish;
        b.merge(moved);
        assert_eq!(c.locos()[0].speed_label(), "speed 50");
        assert!(b.locos().is_empty());
    }

    #[test]
    fn newer_versions_win_and_checksums_follow_the_register() {
        let (mut edge, mut a) = bus("a");
        send(&mut edge, &mut a, "BF 00 03 43", true);
        send(&mut edge, &mut a, "BA 01 01 45", true);

        // A node that comes online later gets the whole register.
        let mut d = LocoRegister::new("d");
        assert_ne!(a.checksum(), d.checksum());
        d.merge(a.snapshot());
        assert_eq!(a.checksum(), d.checksum());

        // An older copy does not overwrite a newer one.
        let old = a.snapshot();
        send(&mut edge, &mut a, "A0 01 29 77", true);
        a.merge(old);
        assert_eq!(a.locos()[0].speed_label(), "speed 40");
    }
}
