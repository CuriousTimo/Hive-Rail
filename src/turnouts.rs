//! Turnout positions, kept the way a LocoNet command station keeps them, so panels that
//! ask for them (an IB-Switch at power-up, a Märklin Keyboard 6040 on Marklin-6040-LocoNet)
//! light up when the node is the command station, for example on DCC-EX.
//!
//! Modelled on an Intellibox I, measured through a GCA101 on 30 October 2026:
//! - `OPC_SW_STATE` (`BC`): answered `LONG_ACK 3C 30` closed or `3C 50` thrown, from the
//!   last switch command it saw for that turnout.
//! - `OPC_SW_REQ` (`B0`): no answer when taken. Without track power it is refused with
//!   `LONG_ACK 30 00`, but the position is remembered all the same.
//! - `OPC_SW_ACK` (`BD`): answered `LONG_ACK 3D 7F`, or `3D 00` without track power.
//!
//! The positions are learned from every switch command on either LocoNet side; only a
//! side the node is the slot master of gets answers, as only one command station may
//! answer on a bus.

use crate::loconet::LocoNetMessage;
use std::collections::BTreeMap;

const OPC_SW_REQ: u8 = 0xB0;
const OPC_SW_REP: u8 = 0xB1;
const OPC_SW_STATE: u8 = 0xBC;
const OPC_SW_ACK: u8 = 0xBD;
/// `sw2` bits of a switch message: closed (green) and output on.
const SW_DIR_CLOSED: u8 = 0x20;
const SW_OUTPUT_ON: u8 = 0x10;
/// `SW_REP` bits: input report (else an output report), closed and thrown outputs on.
const SW_REP_INPUTS: u8 = 0x40;
const SW_REP_CLOSED: u8 = 0x20;
const SW_REP_THROWN: u8 = 0x10;
/// A Digitrax command station's "interrogate" at start-up: switch commands to turnouts
/// 1017 to 1020 that make stationary decoders report their inputs. Not real turnouts
/// (JMRI skips them too).
const INTERROGATE: std::ops::RangeInclusive<u16> = 1017..=1020;
/// `LONG_ACK` answers.
const ACK_REFUSED: u8 = 0x00;
const ACK_TAKEN: u8 = 0x7F;
const STATE_CLOSED: u8 = 0x30;
const STATE_THROWN: u8 = 0x50;

#[derive(Debug, Default)]
pub struct Turnouts {
    /// Turnout number (1 to 2048) and whether it is closed.
    closed: BTreeMap<u16, bool>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TurnoutOutcome {
    /// The command station's answer, for the side the message came from.
    pub replies: Vec<LocoNetMessage>,
    /// A turnout to switch on the track: number and closed. Only for a command taken with
    /// track power on, and only once per switch (the output-on message).
    pub switch: Option<(u16, bool)>,
}

impl Turnouts {
    /// Handles a message from one side. `answer`: the node is that side's command
    /// station. `power_on`: track power is on, so a switch command can be carried out.
    pub fn receive(
        &mut self,
        message: &LocoNetMessage,
        answer: bool,
        power_on: bool,
    ) -> TurnoutOutcome {
        let b = message.as_bytes();
        let mut outcome = TurnoutOutcome::default();
        if b.len() != 4 {
            return outcome;
        }
        let turnout = (u16::from(b[2] & 0x0F) << 7 | u16::from(b[1])) + 1;
        let closed = b[2] & SW_DIR_CLOSED != 0;
        let on = b[2] & SW_OUTPUT_ON != 0;
        match b[0] {
            OPC_SW_REQ | OPC_SW_ACK if INTERROGATE.contains(&turnout) => {}
            OPC_SW_REQ | OPC_SW_ACK => {
                // Remembered even when refused, as on the Intellibox.
                self.closed.insert(turnout, closed);
                if power_on && on {
                    outcome.switch = Some((turnout, closed));
                }
                if answer {
                    if b[0] == OPC_SW_ACK {
                        let code = if power_on { ACK_TAKEN } else { ACK_REFUSED };
                        outcome
                            .replies
                            .push(LocoNetMessage::long_ack(OPC_SW_ACK, code));
                    } else if !power_on {
                        outcome
                            .replies
                            .push(LocoNetMessage::long_ack(OPC_SW_REQ, ACK_REFUSED));
                    }
                }
            }
            // A decoder reporting its outputs: the position it really has.
            OPC_SW_REP if b[2] & SW_REP_INPUTS == 0 => {
                match (b[2] & SW_REP_CLOSED != 0, b[2] & SW_REP_THROWN != 0) {
                    (true, false) => {
                        self.closed.insert(turnout, true);
                    }
                    (false, true) => {
                        self.closed.insert(turnout, false);
                    }
                    _ => {}
                }
            }
            OPC_SW_STATE if answer => {
                // A turnout never switched is reported thrown, as on the Intellibox.
                let state = match self.closed.get(&turnout) {
                    Some(true) => STATE_CLOSED,
                    _ => STATE_THROWN,
                };
                outcome
                    .replies
                    .push(LocoNetMessage::long_ack(OPC_SW_STATE, state));
            }
            _ => {}
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::{TurnoutOutcome, Turnouts};
    use crate::loconet::LocoNetMessage;

    fn send(turnouts: &mut Turnouts, hex: &str, answer: bool, power_on: bool) -> TurnoutOutcome {
        turnouts.receive(&LocoNetMessage::from_hex(hex).unwrap(), answer, power_on)
    }

    fn replies(outcome: &TurnoutOutcome) -> Vec<String> {
        outcome.replies.iter().map(LocoNetMessage::to_hex).collect()
    }

    /// An IB-Switch's power-up question for turnout 111, before and after a switch command,
    /// answered as the Intellibox did.
    #[test]
    fn answers_switch_state_from_the_last_command() {
        let mut turnouts = Turnouts::default();
        assert_eq!(
            replies(&send(&mut turnouts, "BC 6E 00 2D", true, true)),
            ["B4 3C 50 27"]
        );
        let closed = send(&mut turnouts, "B0 6E 30 11", true, true);
        assert!(closed.replies.is_empty());
        assert_eq!(closed.switch, Some((111, true)));
        // The output-off message switches nothing more.
        assert_eq!(send(&mut turnouts, "B0 6E 20 01", true, true).switch, None);
        assert_eq!(
            replies(&send(&mut turnouts, "BC 6E 00 2D", true, true)),
            ["B4 3C 30 47"]
        );
    }

    /// Without track power a switch command is refused, but the position is remembered,
    /// as on the Intellibox (turnout 2000, 30 October 2026).
    #[test]
    fn refuses_without_track_power_but_remembers() {
        let mut turnouts = Turnouts::default();
        let refused = send(&mut turnouts, "B0 4F 3F 3F", true, false);
        assert_eq!(replies(&refused), ["B4 30 00 7B"]);
        assert_eq!(refused.switch, None);
        assert_eq!(
            replies(&send(&mut turnouts, "BC 4F 0F 03", true, false)),
            ["B4 3C 30 47"]
        );
        assert_eq!(
            replies(&send(&mut turnouts, "BD 4F 3F 32", true, false)),
            ["B4 3D 00 76"]
        );
        assert_eq!(
            replies(&send(&mut turnouts, "BD 4F 3F 32", true, true)),
            ["B4 3D 7F 09"]
        );
    }

    /// A Digitrax command station's interrogate (turnouts 1017 to 1020) is not remembered
    /// or switched; a decoder's output report is remembered.
    #[test]
    fn skips_interrogate_and_learns_output_reports() {
        let mut turnouts = Turnouts::default();
        // SW_REQ turnout 1017 thrown on (sw1 0x78, sw2 0x17).
        assert_eq!(send(&mut turnouts, "B0 78 17 20", true, true).switch, None);
        assert_eq!(
            replies(&send(&mut turnouts, "BC 78 07 3C", true, true)),
            ["B4 3C 50 27"]
        );
        // SW_REP turnout 5: outputs report, closed output on (sw2 0x20).
        send(&mut turnouts, "B1 04 20 6A", true, true);
        assert_eq!(
            replies(&send(&mut turnouts, "BC 04 00 47", true, true)),
            ["B4 3C 30 47"]
        );
    }

    /// On a side with its own command station the node only listens: positions are
    /// learned, nothing is answered.
    #[test]
    fn only_the_command_station_answers() {
        let mut turnouts = Turnouts::default();
        let outcome = send(&mut turnouts, "B0 6F 10 30", false, true);
        assert!(outcome.replies.is_empty());
        assert!(send(&mut turnouts, "BC 6F 00 2C", false, true)
            .replies
            .is_empty());
        assert_eq!(
            replies(&send(&mut turnouts, "BC 6F 00 2C", true, true)),
            ["B4 3C 50 27"]
        );
    }
}
