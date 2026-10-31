# Hive-Rail board v2: schematic draft

> **Status: draft, first review done (2026-10-31, see 16). Not checked by simulation or a prototype; do not order boards from it.** Values marked *verify* come from other designs or from documents still to read. The design and its reasons are in the wiki: [Hive-Rail Node Board](https://github.com/CuriousTimo/Hive-Rail/wiki/Hive-Rail-Node-Board). The parts list is [`bom.csv`](bom.csv).

**Build style: through-hole first, modules on pin headers (decision 2026-10-31).** Every part can be soldered by hand without SMD work: the ESP32s, the track drivers, the regulators, Ethernet and the OLED are common modules that plug onto the board on 2.54 mm pin headers (12); everything else is through-hole. This keeps the board buildable by module owners at home, as the 2021 board was. A machine-assembled SMD variant can follow for batches (14).

The board is a full DCC-EX command station with the Hive-Rail extras, on two ESP32 development boards:

- **U1, ESP32-DevKitC (ESP32-WROOM-32E):** stock DCC-EX, with an **Arduino Uno motor shield** of the owner's choice as track driver (EX-MotorShield8874 or Motor Shield R3), wired as DCC-EX's ESP32 shield definitions expect, plus a third DCC-EX track for the booster buses.
- **U2, ESP32-S3-DevKitC-1 (N8 or N8R2):** the Hive-Rail lite node: LocoNet, XpressNet, the network, relays, sensors, LEDs and the OLED. The S3 is chosen because the node needs three UARTs (LocoNet, XpressNet, the DCC-EX link); the ESP32-C6 has two plus a limited low-power one.

Each development board brings its own USB port, USB-serial chip, auto-reset and 3.3 V regulator, so the board needs no USB circuit of its own.

Part choices come from three working designs: DCC-EX's [EX-CSB1](https://github.com/DCC-EX/EX-CSB1) (DCC side), positron96's [LocoNet command station](https://oshwlab.com/positron96/loconet-command-station) (CERN-OHL; LocoNet circuit) and our 2021 board (LocoNet, RailSync driver, Ethernet). We use their part choices, not their layouts.

```mermaid
flowchart LR
    VIN["VIN 15–22 V<br/>(track supply)"] --> PWR["Fuse, reverse protection, TVS<br/>12 V and 5 V switching regulators"]
    subgraph U1["U1 ESP32-DevKitC: DCC-EX"]
    end
    subgraph U2["U2 ESP32-S3-DevKitC-1: lite node"]
    end
    U1 -->|"A: main, B: prog"| SHIELD["Arduino Uno motor shield<br/>(EX-MotorShield8874 or Motor Shield R3)"] --> TRACKS["Main and programming track"]
    U1 -->|"C: booster bus DCC + enable"| GATE["U12 74HCT00"] --> DRV12["U13, U14 TC4427"]
    DRV12 --> LNB["LocoNet-B RailSync"]
    DRV12 --> CDE["CDE out (C, D)"]
    DRV12 --> BBO["B-BUS out"]
    EIN["E / overload in"] -->|"PC814"| U1
    BIN["Booster in<br/>(B-BUS, CDE, RailSync)"] -->|"U20 6N137"| U1
    BIN -->|"same signal"| U2
    U1 -->|"short out, PC817"| BIN
    U2 <-->|"UART: DCC-EX commands"| U1
    U2 -->|"reset"| U1
    U2 --- LN["LocoNet data<br/>LM393 + 2N3904"] --- LNT["LocoNet-T ×2, LocoNet-B ×2"]
    U2 --- XN["MAX3485"] --- XBUS["X-BUS (multiMAUS)"]
    U2 --- ETH["W5500 module"]
    U2 --- REL["ULN2003A → latching relays"]
    SENS["Sensors ×4"] -->|"PC817"| U2
    U2 --- UI["OLED, 5 mm addressable LEDs ×3"]
```

## 1. Power

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| J1 | 2-pole screw terminal 5.08 mm, 10 A | VIN, GND | District track supply, **15 to 22 V DC**. The track voltage is VIN |
| F1 | Fuse 5 A fast, 5×20 mm, in a PCB fuse holder | VIN in series | |
| Q1 | IRF4905 P-MOSFET (TO-220, 55 V) | Reverse-polarity protection: in the VIN line, gate to GND through 10 kΩ, 15 V zener (1N4744A) gate–source | Almost no voltage drop, unlike a diode |
| D1 | 1.5KE27A TVS (axial) | VIN to GND | Conducts above about 25.7 V |
| U3 | 12 V switching regulator, 3-pin 7805 footprint, 1 A, input up to 36 V (Mornsun K7812-1000R3 or equal) | VIN → **+12 V** | LocoNet RailSync drivers, LocoNet-T supply, X-BUS supply, relay coils, LM393. Needs VIN ≥ 15 V |
| U4 | 5 V switching regulator, same type, 2 A (K7805-2000R3 or equal) | VIN → **+5 V** | The shield's logic (5V pin), both development boards (their 5V pins; they make 3.3 V themselves), the W5500 module, the addressable LEDs, 74HCT logic, the 6N137 |
| C1–C4 | 100 µF / 35 V electrolytic and 100 nF on VIN, +12 V and +5 V | | Per the regulator data sheets |
| D2, D3 | Green 3 mm LEDs | +12 V, +5 V | Power indication |

- **Input range 15 to 22 V** covers H0 and 0 track supplies. The TVS conducts from about 25.7 V and clamps below 38 V at its full rated surge; the regulators are rated 36 V (*verify* the surge rating of the chosen type, or pick a 72 V input type if affordable). The DRV8874 is rated 37 V, the L298P 46 V.
- **N scale (12 to 14 V track):** a 12 V regulator cannot work there; the LocoNet 12 V would have to come from VIN through a diode (open point, 13).
- **USB only:** each development board runs from its own USB cable for flashing and tests, without track power. *Verify* that each development board has a diode between its USB VBUS and its 5V pin, so the board's +5 V does not feed the computer's USB port; the official DevKitC boards do, clones vary.

## 2. U1: DCC-EX (ESP32-DevKitC)

**Pins.** The track drivers are an Arduino Uno motor shield (3). Its Uno pins are wired to the same GPIOs as on the ESPduino-32 (an ESP32 in Uno form), for which DCC-EX already has shield definitions (`MotorDrivers.h`, ESP32 section), so those definitions hold unchanged.

| GPIO | Uno pin | Function | To |
| --- | --- | --- | --- |
| 25 | D3 | Track A enable (PWM) | Shield header |
| 19 | D12 | Track A DCC signal (direction) | Shield header |
| 13 | D9 | Track A brake | Shield header |
| 35 | A0 (A2 on the ESPduino-32) | Track A current (input only) | Shield header A0; solder jumper to A2 for shields that use A2 |
| 36 | A4 | Track A fault (EX-MotorShield8874; input only) | Shield header |
| 23 | D11 | Track B enable (PWM) | Shield header |
| 18 | D13 | Track B DCC signal | Shield header |
| 12 | D8 | Track B brake | Shield header. GPIO12 is a strapping pin (flash voltage): it must be low at reset; both shields have a pull-down there (*verify*), as on the ESPduino-32 |
| 34 | A1 (A3 on the ESPduino-32) | Track B current (input only) | Shield header A1; solder jumper to A3 |
| 39 | A5 | Track B fault (input only) | Shield header |
| 5 | – | Track C (booster buses) DCC signal | U12 |
| 4 | – | Track C enable | U12 |
| 32 | – | Booster input (`BOOSTER_INPUT 32`, as EX-CSB1) | U20 output |
| 16 / 17 | – | Serial2 RX / TX (`SERIAL2_COMMANDS`, DCC-EX's fixed pins) | U2 GPIO17 / 18 |
| 27 | – | Booster short in (CDE E, B-BUS overload) | U21 outputs, 10 kΩ pull-up |
| 14 | – | Short line out to the master (booster mode) | U22 LED through 1 kΩ |
| 33 | – | Stop button (to GND, 10 kΩ pull-up) | SW1, front panel |
| 26 | – | Go button | SW2, front panel |
| EN | – | Reset | DevKitC's own; also Q5 from U2 |
| 21 / 22 | – | I²C, spare | Header J30 |

GPIO0, 2 and 15 (strapping pins) stay free.

**DCC-EX configuration** (`config.h`, stock DCC-EX). The first two lines are DCC-EX's own ESP32 shield definitions; the third adds the booster buses:

```cpp
// EX-MotorShield8874 (DCC-EX's EX8874_SHIELD for ESP32) plus track C for the booster
// buses (LocoNet-B RailSync, CDE, B-BUS out): no current sense, boosters measure.
#define HIVE_RAIL_V2_EX8874 F("HIVERAIL_V2_8874"), \
  new MotorDriver(25, 19, UNUSED_PIN, 13, 35, 1.27, 5000, 36), \
  new MotorDriver(23, 18, UNUSED_PIN, 12, 34, 1.27, 5000, 39), \
  new MotorDriver( 4,  5, UNUSED_PIN, UNUSED_PIN, UNUSED_PIN, 1.0, 5000, UNUSED_PIN)

// Arduino Motor Shield R3 and clones (DCC-EX's STANDARD_MOTOR_SHIELD for ESP32): no fault pins
#define HIVE_RAIL_V2_STANDARD F("HIVERAIL_V2_STD"), \
  new MotorDriver(25, 19, UNUSED_PIN, 13, 35, 0.70, 1500, UNUSED_PIN), \
  new MotorDriver(23, 18, UNUSED_PIN, 12, 34, 0.70, 1500, UNUSED_PIN), \
  new MotorDriver( 4,  5, UNUSED_PIN, UNUSED_PIN, UNUSED_PIN, 1.0, 5000, UNUSED_PIN)

// No shield, boosters only (LocoNet-B, CDE, B-BUS): track C is the district's only DCC output.
// No programming track; shorts are handled by the boosters (CDE E and B-BUS overload still reach GPIO27).
#define HIVE_RAIL_V2_BOOSTERS F("HIVERAIL_V2_BOOST"), \
  new MotorDriver( 4,  5, UNUSED_PIN, UNUSED_PIN, UNUSED_PIN, 1.0, 5000, UNUSED_PIN)

#define MOTOR_SHIELD_TYPE HIVE_RAIL_V2_EX8874   // or HIVE_RAIL_V2_STANDARD, or HIVE_RAIL_V2_BOOSTERS
#define BOOSTER_INPUT 32
#define SERIAL2_COMMANDS
```

`myAutomation.h`: `SET_TRACK(C, MAIN)` at start (`SET_TRACK(A, MAIN)` with the boosters-only definition, where the booster buses are DCC-EX's first track), stop and go (`ONBUTTON(33) POWEROFF`, `ONBUTTON(26) POWERON`), the booster short in on GPIO27 as a power off, the short line out from `ONOVERLOAD`, and booster mode on `ONRAILSYNCON` (`SET_TRACK(A, BOOST)`). In booster mode DCC-EX routes GPIO32 to the track outputs in the ESP32's GPIO matrix (hardware, no software in the path).

## 3. Track outputs: an Arduino Uno motor shield

The board has **Arduino Uno R3 shield headers**; the owner plugs in the motor shield of their choice. A blown shield is swapped in seconds.

**The shield is optional.** A district powered entirely by boosters on LocoNet-B, CDE or B-BUS needs no shield: the board then drives only the booster buses (DCC-EX definition `HIVE_RAIL_V2_BOOSTERS` in 2), and the shield, J11, J12, D8, D9 and C10 can be left out. There is no programming track then, until a shield is added.

| Shield | Driver | Notes |
| --- | --- | --- |
| **EX-MotorShield8874** (DCC-EX) | 2× DRV8874, up to 5 A | DCC-EX's recommendation; fault outputs on A4/A5 |
| **Arduino Motor Shield R3** and clones (Deek-Robot and others) | L298P, 2 A per channel | Cheapest; must be the R3 (IOREF-aware, 3.3 V-safe current-sense outputs) |

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| J10 | Arduino Uno R3 shield headers (female: 1×10, 2× 1×8, 1×6) | Uno pins to U1 as in 2; **IOREF and 3.3V** to U1's 3.3 V; **5V** to +5 V; GND; **VIN not connected** (solder jumper, open); RESET not connected | Shields take track power on their own screw terminal, not through the VIN pin: one header pin cannot carry 5 A |
| J11 | 2-pole screw terminal 5.08 mm, 10 A, next to the shield | VIN out to the shield's power terminal (short wires) | Fused by F1 |
| J12 | 4-pole screw terminal 5.08 mm, 10 A | Main track and programming track, wired from the shield's outputs | The track wiring stays at the board's edge, behind the TVS diodes |
| D8, D9 | P6KE30CA bidirectional TVS (axial) | Across each track output at J12 | Spikes and ESD from the track |
| C10 | 470 µF / 35 V electrolytic | VIN at J11 | Bulk capacitance for the shield |
| J16 | 2×4 pin header, track A signals (enable, signal, brake, current, fault, 3.3 V, GND) | In parallel with the shield's channel A | For an **IBT-2 (BTS7960) module** on the main track for more than 5 A on a large district (cabled). DCC-EX supports the IBT-2 |

- **Keep-out:** the shield sits about 69 × 53 mm above the board; only low parts under it.
- **Shield and booster mode:** in mode 5 the shield's channel A amplifies the master's DCC (DCC-EX switches it to `BOOST`).

## 4. Booster buses out (LocoNet-B RailSync, CDE, B-BUS)

DCC-EX's track C drives all three: when DCC-EX switches the district off, the booster signal stops and the boosters switch off.

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| U12 | 74HCT00 (DIP-14, four NAND gates), +5 V | DCC_C = signal AND enable; /DCC_C = NOT signal AND enable | HCT inputs take the ESP32's 3.3 V. Both outputs low when the district is off |
| U13 | TC4427 (DIP-8, two non-inverting drivers), +12 V | DCC_C, /DCC_C → **LocoNet-B RailSync** pins 1 and 6, each through 150 Ω and a 100 mA polyfuse | A shorted RailSync pin draws about 80 mA, which the driver survives; *verify* against the LocoNet spec |
| U14 | TC4427, +12 V | → **CDE C and D** (J12) and **B-BUS out** pin 1 (J13), through 150 Ω each | CDE and B-BUS levels *verify* (wiki NB-Q16) |
| U21 | 2× PC814 (DIP-4, AC-input optocoupler), LED fed from +12 V through 2.2 kΩ | CDE E and B-BUS pin 4 → LED; transistors pull U1 GPIO36 low | AC input: works whichever way the booster's E circuit is referenced (C, D or its ground), which is still to measure. Wired-OR |
| J12 | 3-pole screw terminal (C, D, E) | | Lenz CDE |
| J13 | B-BUS out, 4-pole (pin 1 DCC, 2–3 GND, 4 overload in) | | Pinout from a forum post; connector type *verify* (NB-Q16) |

## 5. Booster input (mode 5) and the DCC sniffer

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| J14 | B-BUS in, 4-pole | Pin 1 DCC in, 2–3 GND (master side), 4 short line out | From a Z21's B-BUS |
| J15 | 3-pole screw terminal | C, D in; E out | From a CDE or RailSync output (Intellibox) |
| U20 | 6N137 (DIP-8, 10 Mbit/s optocoupler), output side on +5 V | LED through a **constant-current source** (two 2N3904, about 8 mA) with a 1N4148 reverse across the LED (accepts C/D or DCC/GND); open-collector output pulled up to **3.3 V** → U1 GPIO32 and U2 GPIO7 | The current source takes any DCC level from 5 to 24 V; a fixed resistor would give too little at 5 V and too much at 22 V. A slow optocoupler (PC817) would skew DCC's 58 µs half-bits |
| U22 | PC817 (DIP-4) | LED from U1 GPIO4 through 1 kΩ; transistor between J14 pin 4 / J15 E and the master's ground | Pulls the master's short line |

The input side of U20 and U22 is the master's ground, never the board's: the two systems stay separate.

## 6. U2: lite node (ESP32-S3-DevKitC-1)

| GPIO | Function | To |
| --- | --- | --- |
| 4 / 5 | UART1 RX / TX: LocoNet (16 666 baud, TX inverted) | U40 output / Q40 base |
| 17 / 18 | UART2 TX / RX: DCC-EX link (115 200 baud) | U1 GPIO16 / 17 |
| 47 / 21 | UART0 TX / RX, moved off GPIO43/44: XpressNet (62 500 baud, 9 bits by the parity trick) | U23 DI / RO |
| 6 | XpressNet direction | U23 DE and /RE, 10 kΩ pull-down |
| 7 | DCC sniffer in | U20 output |
| 10 / 11 / 12 / 13 | SPI CS / MOSI / SCK / MISO | J20 (W5500 module) |
| 9 / 8 | W5500 interrupt / reset | J20 |
| 1 / 2 | I²C SDA / SCL, 4.7 kΩ pull-ups | OLED J31, I²C header J32 |
| 48 | Addressable LED data | U24 (74HCT125, 3.3 → 5 V) → LED1–LED3 |
| 14 | Reset of U1 | Q5 (2N3904, collector to U1 EN), 10 kΩ base pull-down |
| 15 / 16 / 39 / 40 | Relay coils: relay 1 set / reset, relay 2 set / reset | U25 (ULN2003A) inputs, 10 kΩ pull-downs |
| 41 / 42 / 19 / 20 | Sensor inputs 1–4 | U26 (4× PC817) outputs, 10 kΩ pull-ups |
| 0 | Boot and user button (config) | The DevKitC's BOOT button |

- **Pins left free on purpose:** GPIO3, 45 and 46 (strapping), 35–37 (octal PSRAM on N8R8 boards), 43/44 (the DevKitC's USB-serial chip), 38 (the DevKitC's own RGB LED on board version 1.1; on version 1.0 it sits on GPIO48, where it then shows the same colour as LED1, which does no harm).
- GPIO19 and 20 are the S3's native USB; the board uses the DevKitC's USB-serial port for flashing and the console, so they are free as inputs.
- **Recheck this pin plan against the DevKitC bought:** clones differ.

## 7. LocoNet

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| J40, J41 | RJ12 6P6C, through-hole | **LocoNet-T**: pins 3–4 data, 2–5 GND, 1–6 +12 V through F40 | Throttle supply on the RailSync pins, *verify* against Uhlenbrock's LocoNet-T |
| J42, J43 | RJ12 6P6C | **LocoNet-B**: pins 3–4 data, 2–5 GND, 1–6 RailSync DCC from U13 | |
| F40 | Radial polyfuse 500 mA, 30 V or more | +12 V to LocoNet-T | |
| U40 | LM393 (DIP-8), **supplied from +12 V** | Data line through a divider to + input, reference to − input, hysteresis resistor; open-collector output pulled up to **3.3 V** → U2 GPIO4 | From 12 V its inputs see the whole bus swing (an LM393 input must stay 1.5 V below its supply); the open-collector output stays safe for the ESP32. Values as positron96's v1 (27k, 12k, 30k, 91k, 330k, 10k), *verify* in its schematic |
| Q40 | 2N3904 (TO-92) | Base from U2 GPIO5 through 2 kΩ; collector to the data line through 47 Ω | Open-collector transmitter |
| U41, JP40 | LM317LZ (TO-92) as a current source, with a jumper | +12 V to the data line | The bus pull-up, only on the bus master: jumper fitted when this board is the master; *verify* the current against the LocoNet spec |
| D40 | P6KE15CA TVS | Data line to GND | Protection |

## 8. XpressNet (multiMAUS)

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| U23 | MAX3485 (DIP-8, 3.3 V RS-485, half duplex) | A, B to J50 pins 4 and 3; DE and /RE from U2 GPIO6 with a 10 kΩ pull-down | The pull-down keeps the transceiver receiving while the ESP32 boots |
| R50–R52 | 680 Ω, 680 Ω bias, 120 Ω termination with jumper JP50 | | Fail-safe idle |
| D50, D51 | P6KE12CA TVS | A and B to GND | RS-485 common-mode range is −7 to +12 V |
| J50, J51 | RJ12 6P6C | 1 C (DCC, not connected by default), 2 GND, 3 B, 4 A, 5 +12 V through F50, 6 D (not connected) | Lenz pinout; Roco cable *verify* (NB-Q14) |
| F50 | Radial polyfuse 300 mA, 30 V or more | +12 V to X-BUS | Powers the multiMAUS |

## 9. Network

| Ref | Part | Notes |
| --- | --- | --- |
| J20 | 2×5 pin socket for the common blue **W5500 Ethernet module** (RJ45 with magnetics, its own 3.3 V regulator), powered from +5 V | Optional. The cheapest widely sold W5500 module; clones differ in pin order, so the silkscreen names one pinout (*verify* with the module bought). The magnetics isolate the network |

## 10. Transfer relays and sensors

The transfer relays sit **on the board**: both feeds of a transfer block (the sending district's and this district's) come to the board's screw terminals, and the relay switches the transfer block's output there.

| Ref | Part | Connections | Notes |
| --- | --- | --- | --- |
| K1, K2 | Latching relay, **double-pole changeover**, at least 5 A at 24 V DC, 12 V coils (one set coil, one reset coil); or two single-pole changeover latching relays per transfer block, switched together | Common contacts → transfer block (both rails); one side → the sending district's feed, the other → this district's own track feed | Double-pole so the two districts never share a rail; changeover breaks before it makes. Part to choose in phase B0 (price, stock, coil current) |
| J62, J63 | 6-pole screw terminals 5.08 mm, 10 A | Per transfer block: sending feed (2), own feed (2), transfer block (2) | Track current: section 15's 5 A rules apply to these nets and the relay contacts |
| U25 | ULN2003A (DIP-16, in a socket) | Inputs from U2, each with a 10 kΩ pull-down; outputs sink K1 and K2's coils from +12 V; COM to +12 V (built-in flyback diodes) | Three spare channels. The pull-downs keep a booting ESP32 from pulsing a coil, which would flip a latching relay |
| D60, D61 | Yellow 3 mm LEDs with resistors, across a set coil each | | Flash when a relay is set (a latching relay shows nothing otherwise) |
| U26 | 4× PC817 (DIP-4, in sockets) | Input: sensor through 2.2 kΩ and a 1N4148, from its own supply; output to U2 | Isolated sensor inputs (transfer block entry and clear) |
| J61 | 8-pole screw terminal 3.5 mm | Four sensor pairs | |

## 11. User interface

| Ref | Part | Notes |
| --- | --- | --- |
| SW1, SW2 | 12 mm push buttons, red and green caps | Stop and go, on U1 |
| SW3 | 6 mm push button | U1 reset (the DevKitC's own buttons are hard to reach once plugged in) |
| LED1–LED3 | 5 mm through-hole addressable RGB LEDs (WS2812D-F5 or PL9823) | Power, link, layout |
| U24 | 74HCT125 (DIP-14), +5 V | 3.3 V → 5 V level shift for the LEDs |
| J31 | 4-pin header | 128x64 OLED module (SSD1306 or SH1106, I²C) |
| J32 | 4-pin I²C header (3.3 V, GND, SDA, SCL) | I²C accessories (Qwiic and STEMMA QT through an adapter cable) |
| J33 | 2×5 pin header, **front panel** | Stop and go buttons and the three LEDs, in parallel with the ones on the board, for a module fascia |

## 12. Mechanical and plug-in modules

**Modules plug onto the board on 2.54 mm pin headers:**

| Module | Header | Where to buy |
| --- | --- | --- |
| U1 ESP32-DevKitC, U2 ESP32-S3-DevKitC-1 | Two rows each | Espressif or clones (AliExpress) |
| Motor shield (EX-MotorShield8874 or Arduino Motor Shield R3) | Arduino Uno R3 headers | DCC-EX resellers; Arduino and clones (AliExpress) |
| U3, U4 3-pin switching regulators (K7812, K7805 type) | 3 pins | AliExpress, electronics shops |
| W5500 Ethernet module | 2×5 | AliExpress |
| OLED 128x64 I²C | 4 pins | AliExpress |

**Swappable chips:** the parts facing the outside world, which are the ones that blow, sit in **DIP sockets** (or a 3-pin socket for the TO-92 transistor): U40 (LM393) and Q40 (2N3904) on LocoNet, U13 and U14 (TC4427) on the booster buses, U23 (MAX3485) on XpressNet, U20 (6N137), U21 (PC814), U22 and U26 (PC817) at the isolated inputs and outputs, U25 (ULN2003A) on the relays. A blown chip is replaced in seconds, like a module.

Not plug-in: the IBT-2 and the L298N modules have screw terminals for power and the motor. They stay a cabled option for more track current (J16 and screw terminals), not the standard. LM2596 buck boards do not fit either: their corner pads are not on a 0.1" grid and differ per seller.

**Rules:**

- **Current per pin:** a 2.54 mm header pin carries about 3 A. Nets of 2 A or more use at least two pins in parallel. Track current never runs through a header: the shield has its own screw terminals.
- **Female headers on the board, male pins on the modules.** Each module's outline and pin 1 printed on the silkscreen; headers laid out so a module cannot be plugged one row off or turned round (a dummy or missing pin, or an asymmetric row). A wrongly plugged module can destroy itself or the ESP32.
- **Hold-downs:** the board travels to meetings. Modules with mounting holes (development boards, W5500) get standoffs, not only their headers.
- **Keep-out:** no tall parts under a plugged-in module or the shield; height limit from the female header chosen (8.5 mm standard).
- **One pinout per module, named on the board:** AliExpress clones differ in pin order (W5500) and in which pins exist (development boards). The board documents the exact module it expects.

**Board:**

- Size about 160 × 100 mm with the modules (*to fit*), four mounting holes; a DIN-rail case is a later option.
- Screw terminals on one edge (power, tracks, boosters, relays, sensors), RJ12 jacks on another (LocoNet, X-BUS); the development boards' USB ports and the Ethernet jack reachable from the edge.

## 13. Open points in this draft

- Every *verify* above, especially: CDE and B-BUS levels and connectors (NB-Q16), the X-BUS cable pinout (NB-Q14), LocoNet-T supply and bus pull-up current, the LM393 values, the shields' pull-downs on D8 (GPIO12 strapping).
- N-scale track supplies (12 to 14 V): where the LocoNet 12 V comes from.
- The regulators' surge rating against the TVS clamp voltage.
- Thermal: the shield under load, and two switching regulators.

## 14. Machine-assembled variant (later)

For a batch assembled by a factory (JLCPCB), the same schematic with SMD parts is cheaper per board: ESP32-WROOM-32E and ESP32-S3-WROOM-1 modules instead of development boards (then with a USB hub, a CH340C, USB-C and ESD protection on the board), DRV8874 chips, buck converter chips, SMD optocouplers and logic. The first draft of this file (git history) lists those parts.

## 15. PCB rules: current and track widths

Track widths from IPC-2221 for **outer layers and a 10 °C rise**. The current to design for is the largest one that can flow for longer than a moment: the fuse's 5 A on the power path, the trip current elsewhere.

| Current | 1 oz copper (35 µm) | 2 oz copper (70 µm) |
| --- | --- | --- |
| 0.5 A | 0.12 mm (use 0.25 mm) | 0.25 mm |
| 1 A | 0.30 mm | 0.15 mm (use 0.25 mm) |
| 2 A | 0.78 mm | 0.39 mm |
| 3 A | 1.37 mm | 0.68 mm |
| 5 A | 2.77 mm | 1.38 mm |
| 6 A | 3.56 mm | 1.78 mm |

**Rules for the layout:**

| Net | Design current | Rule |
| --- | --- | --- |
| VIN: J1 → F1 → Q1 → J11 (shield power) and regulators | 5 A (fuse) | Copper pours, at least 3 mm wide on 1 oz, on both layers stitched with vias |
| Track outputs: J12 and D8/D9 (wired from the shield) | 5 A (DCC-EX trips at 5 A with the EX-MotorShield8874) | Pours at least 3 mm on 1 oz, both layers |
| GND return from the track outputs | Same as VIN | Solid ground pour on the bottom layer; no slots under the power path |
| +12 V from U3 | 1 A | 0.5 mm trunk and branches |
| +5 V from U4 | 2 A | 1.0 mm trunk, 0.5 mm branches |
| RailSync, CDE, B-BUS outputs | Under 0.2 A (resistors and polyfuses) | 0.4 mm |
| LocoNet data, RS-485, signals | Milliamps | 0.25 mm; RS-485 A and B as a pair |

- **Copper:** order 2 oz outer copper, or keep 1 oz and use pours; JLCPCB charges extra for 2 oz. Rev A: 2 oz, the safer choice until the boards are measured warm.
- **Through-hole pads carrying current** (terminals, fuse holder, Q1): generous annular rings, and the pour connected with wide thermal spokes or solid, so 5 A does not run through four thin spokes.
- **Vias carrying current:** count about 1 A per 0.3 mm via; at least 6 vias where 5 A changes layer.
- **Isolation gaps:** no copper under the optocouplers between their two sides, and at least 2 mm between the master's side of the booster input (J14, J15, U20 and U22 inputs) and the board's ground. This is about keeping the systems apart, not about voltage: everything is below 30 V.
- **Separation:** keep the track outputs, J11 and the shield away from the LM393, the RS-485 transceiver and the ESP32 antennas; the development boards' antennas at the board edge with no copper under them.

## 16. Review 2026-10-31: findings and changes

| # | Finding | Change |
| --- | --- | --- |
| R1 | Buck chips rated 28 V on VIN: no margin for track-supply spikes | Switching regulators rated 36 V; VIN range 15 to 22 V |
| R2 | 5 A polyfuse in 1812: such parts are mostly rated 6 to 16 V | 5 A fast fuse in a holder; all polyfuses radial, rated 30 V or more |
| R3 | TVS clamping above what the regulators survive | 1.5KE27A with a 15 to 22 V input range; surge rating to verify |
| R4 | LM393 on 3.3 V cannot see the 12 V LocoNet bus without careful dividers | LM393 on +12 V, open-collector output pulled to 3.3 V |
| R5 | A shorted RailSync pin with 47 Ω would overheat the driver | 150 Ω and a polyfuse per pin; TC4427 instead of MIC4427 (same pinout, cheaper, also in DIP) |
| R6 | The E line's reference in CDE boosters is not known yet | AC-input optocouplers (PC814) on E and the B-BUS overload line |
| R7 | The booster input's fixed resistor fails at 5 V or overdrives at 22 V | Constant-current source for the optocoupler's LED |
| R8 | Floating pins while an ESP32 boots: could pulse a latching relay, hold DCC-EX in reset, or drive the RS-485 bus | 10 kΩ pull-downs on the ULN2003A inputs, Q5's base and the RS-485 DE pin |
| R9 | No logic power without track power | Each development board runs from its own USB |
| R10 | No protection on the track outputs | Bidirectional TVS across each track output |
| R11 | WIZ850io is expensive | Socket for the common blue W5500 module |
| R12 | Only 3 A on the main track | Header for an IBT-2 module instead of M1 |
| R13 | Buttons and LEDs only on the board | Front-panel header for a module fascia |
| R14 | I²C pull-ups missing | Added |
| R15 | No current and track width rules | Section 15 |
| R16 | SMD parts need factory assembly or SMD soldering | **Through-hole first:** development boards, driver carriers, 3-pin switching regulators, DIP and TO-92 parts. The SMD variant is for later (14) |
| R17 | The S3 development board's USB-serial chip sits on GPIO43/44 (UART0) | XpressNet's UART0 moved to GPIO47/21 |
| R21 | Districts powered only by boosters paid for drivers they do not use | The shield is optional; a boosters-only DCC-EX definition |
| R20 | Track drivers fixed to one type | **Arduino Uno shield headers**: the owner picks the EX-MotorShield8874 or an Arduino Motor Shield R3; wired as DCC-EX's ESP32 shield definitions expect, so they hold unchanged; U1's pins re-planned (2, 3) |
| R19 | Relays off the board needed extra wiring at each transfer block; blown interface chips needed desoldering | Transfer relays on the board (10), with the track-current rules; interface chips in DIP sockets (12) |
| R18 | Modules are AliExpress-common and must plug in | Every module on 2.54 mm headers, with current per pin, keying, hold-down and keep-out rules (12). Track drivers that only have screw terminals (IBT-2, L298N) stay a cabled option |

Notes from the review that need no change: the 9-bit XpressNet characters only matter when sending (the master's call bytes); received bytes always have the ninth bit clear, so the receiver can ignore parity. DCC-EX's tracks must be set at start-up in `myAutomation.h` (`SET_TRACK(C, MAIN)` for the booster buses).
