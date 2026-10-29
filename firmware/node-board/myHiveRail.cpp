// Hive-Rail add-on for DCC-EX on the node board. DCC-EX's own files are not changed:
// - halSetup() adds a HAL device whose _loop() DCC-EX calls from its main loop, so the
//   OLED, the LEDs and the reads of DCC-EX's track state all run in DCC-EX's own thread;
// - myFilter() sees every command: the status line Hive-Rail sends once a second
//   (<U 1 ...>, also the link heartbeat) and DCC-EX's fast clock (<JC minutes rate>).
// The OLED is driven here, not by DCC-EX (config.h has no OLED_DRIVER), so the whole
// 128x64 screen shows the Hive-Rail layout.

#include <Arduino.h>
#include <Wire.h>
#include <WiFi.h>
#include <driver/rmt.h>
#include <esp_wifi.h>
#include "DIAG.h"
#include "IODevice.h"
#include "KeywordHasher.h"
#include "TrackManager.h"

#ifndef HIVE_RAIL_NAME
#define HIVE_RAIL_NAME "Hive-Rail" // shown in the title bar; set per module in config.h
#endif
#ifndef HIVE_RAIL_HEARTBEAT_LED
#define HIVE_RAIL_HEARTBEAT_LED 2 // blue LED on an ESP32 DevKit V1
#endif
#ifndef HIVE_RAIL_WS2812_PIN
#define HIVE_RAIL_WS2812_PIN 4 // three WS2812 on the 2021 board
#endif
#ifndef HIVE_RAIL_OLED_ADDRESS
#define HIVE_RAIL_OLED_ADDRESS 0x3C
#endif
// An SH1106 has 132 columns; its 128 visible ones start at column 2.
#ifdef HIVE_RAIL_OLED_SH1106
#define OLED_COLUMN_OFFSET 2
#else
#define OLED_COLUMN_OFFSET 0
#endif

// Hive-Rail sends its status once a second; without it for this long the link is lost.
static const uint32_t LINK_TIMEOUT_MS = 3000;
static const uint8_t BOARD_PROTOCOL = 1;

// ---------------------------------------------------------------------------------------
// State, written by myFilter() and the HAL loop, both in DCC-EX's main loop.

struct Status {
  bool seen = false;       // a status line has arrived since boot
  uint32_t lastMs = 0;     // when the last one arrived
  int rocnetId = 0;
  int nodes = 0;           // online nodes including this one
  int locos = 0;           // locos driven through this node's LocoNet
  int slots = 0;           // slots taken on the whole layout
  int flags = 0;           // FLAG_* below
  int locoAddress = 0;     // last loco driven on this module, 0 = none
  int locoSpeed = 0;       // LocoNet speed step: 0 stop, 1 emergency stop, 2..127
  bool locoForward = true;
  int locoFunctions = 0;   // F0..F4, bit n = Fn
};
static const int FLAG_ALLOCATOR = 1;  // slot allocator and fast clock master
static const int FLAG_SLOTS_SYNC = 2; // every node holds the same slot table
static const int FLAG_STOP = 4;       // district in emergency stop
static const int FLAG_COMPARED = 8;   // there was a peer to compare the slot table with

static Status status;
static int clockMinutes = -1; // fast clock from <JC minutes rate>, -1 = not set
static int clockRate = 0;

static bool linkUp() {
  return status.seen && millis() - status.lastMs < LINK_TIMEOUT_MS;
}

// ---------------------------------------------------------------------------------------
// Commands from Hive-Rail, through DCC-EX's command filter.

void myFilter(Print *stream, byte &opcode, byte &paramCount, int16_t p[]) {
  if (opcode == 'J' && paramCount == 3 && p[0] == "C"_hk) {
    // DCC-EX handles the fast clock itself too; this copy is for the screen.
    clockMinutes = p[1];
    clockRate = p[2];
    return;
  }
  if (opcode != 'U')
    return;
  if (paramCount == 1 && p[0] == 0) {
    // <U 0>: Hive-Rail asks whether this board has the add-on.
    StringFormatter::send(stream, F("<u %d>\n"), BOARD_PROTOCOL);
  } else if (paramCount >= 9 && p[0] == 1) {
    // <U 1 rocnet nodes locos slots flags address speed dirf>; DCC-EX allows 9 parameters,
    // so dirf holds F0-F4 in bits 0-4 and forward in bit 7.
    status.seen = true;
    status.lastMs = millis();
    status.rocnetId = p[1];
    status.nodes = p[2];
    status.locos = p[3];
    status.slots = p[4];
    status.flags = p[5];
    status.locoAddress = p[6];
    status.locoSpeed = p[7];
    status.locoForward = p[8] & 0x80;
    status.locoFunctions = p[8] & 0x1F;
  }
  opcode = 0; // handled here; DCC-EX would answer <X> for an unknown command
}

// ---------------------------------------------------------------------------------------
// WS2812 status LEDs on RMT channel 6 (DCC-EX uses 0-1 for MAIN and 2-3 for PROG).

#define WS2812_COUNT 3
#define WS2812_RMT_CHANNEL RMT_CHANNEL_6
#define LEVEL 26 // about 10 % brightness

struct Colour {
  uint8_t red, green, blue;
};
static const Colour DARK = {0, 0, 0};
static const Colour POWER_ON = {0, LEVEL, 0};
static const Colour POWER_OFF = {LEVEL, 0, 0};
static const Colour POWER_SHORT = {LEVEL, LEVEL / 3, 0};
static const Colour POWER_STOP = {LEVEL, LEVEL, 0};

// WS2812 bit timing at 40 MHz RMT ticks (25 ns): 0 = 0.4/0.85 us, 1 = 0.8/0.45 us.
static const rmt_item32_t WS2812_BIT0 = {{{16, 1, 34, 0}}};
static const rmt_item32_t WS2812_BIT1 = {{{32, 1, 18, 0}}};

static void ws2812Begin() {
  rmt_config_t config = RMT_DEFAULT_CONFIG_TX((gpio_num_t)HIVE_RAIL_WS2812_PIN, WS2812_RMT_CHANNEL);
  config.clk_div = 2;
  config.mem_block_num = 2; // 128 items, enough for 3 LEDs x 24 bits
  ESP_ERROR_CHECK(rmt_config(&config));
  ESP_ERROR_CHECK(rmt_driver_install(config.channel, 0, 0));
}

static void ws2812Show(const Colour (&leds)[WS2812_COUNT]) {
  rmt_item32_t items[WS2812_COUNT * 24];
  size_t n = 0;
  for (const Colour &led : leds) {
    // WS2812 expects green, red, blue, most significant bit first.
    uint32_t grb = (uint32_t)led.green << 16 | (uint32_t)led.red << 8 | led.blue;
    for (int bit = 23; bit >= 0; bit--)
      items[n++] = (grb >> bit) & 1 ? WS2812_BIT1 : WS2812_BIT0;
  }
  rmt_write_items(WS2812_RMT_CHANNEL, items, n, true);
}

// ---------------------------------------------------------------------------------------
// District power as DCC-EX's track manager sees it, MAIN tracks only.

enum class District { Off, On, Short };

static District districtPower() {
  bool anyOn = false;
  for (byte t = 0; t < TrackManager::numTracks(); t++) {
    if (!TrackManager::isActive(t) || !(TrackManager::getMode(t) & TRACK_MODE_MAIN))
      continue;
    POWERMODE mode = TrackManager::getPower(t);
    if (mode == POWERMODE::OVERLOAD)
      return District::Short;
    if (mode == POWERMODE::ON || mode == POWERMODE::ALERT)
      anyOn = true;
  }
  return anyOn ? District::On : District::Off;
}

// ---------------------------------------------------------------------------------------
// SSD1306 128x64 OLED with a frame buffer; changed pages are sent one per loop call, so
// DCC-EX's loop is never held up for a whole screen.

// Classic 5x7 font, ASCII 0x20-0x7E, one byte per column, bit 0 at the top.
static const uint8_t FONT[][5] = {
  {0x00,0x00,0x00,0x00,0x00}, {0x00,0x00,0x5F,0x00,0x00}, {0x00,0x07,0x00,0x07,0x00}, // sp ! "
  {0x14,0x7F,0x14,0x7F,0x14}, {0x24,0x2A,0x7F,0x2A,0x12}, {0x23,0x13,0x08,0x64,0x62}, // # $ %
  {0x36,0x49,0x55,0x22,0x50}, {0x00,0x05,0x03,0x00,0x00}, {0x00,0x1C,0x22,0x41,0x00}, // & ' (
  {0x00,0x41,0x22,0x1C,0x00}, {0x08,0x2A,0x1C,0x2A,0x08}, {0x08,0x08,0x3E,0x08,0x08}, // ) * +
  {0x00,0x50,0x30,0x00,0x00}, {0x08,0x08,0x08,0x08,0x08}, {0x00,0x60,0x60,0x00,0x00}, // , - .
  {0x20,0x10,0x08,0x04,0x02}, {0x3E,0x51,0x49,0x45,0x3E}, {0x00,0x42,0x7F,0x40,0x00}, // / 0 1
  {0x42,0x61,0x51,0x49,0x46}, {0x21,0x41,0x45,0x4B,0x31}, {0x18,0x14,0x12,0x7F,0x10}, // 2 3 4
  {0x27,0x45,0x45,0x45,0x39}, {0x3C,0x4A,0x49,0x49,0x30}, {0x01,0x71,0x09,0x05,0x03}, // 5 6 7
  {0x36,0x49,0x49,0x49,0x36}, {0x06,0x49,0x49,0x29,0x1E}, {0x00,0x36,0x36,0x00,0x00}, // 8 9 :
  {0x00,0x56,0x36,0x00,0x00}, {0x08,0x14,0x22,0x41,0x00}, {0x14,0x14,0x14,0x14,0x14}, // ; < =
  {0x00,0x41,0x22,0x14,0x08}, {0x02,0x01,0x51,0x09,0x06}, {0x32,0x49,0x79,0x41,0x3E}, // > ? @
  {0x7E,0x11,0x11,0x11,0x7E}, {0x7F,0x49,0x49,0x49,0x36}, {0x3E,0x41,0x41,0x41,0x22}, // A B C
  {0x7F,0x41,0x41,0x22,0x1C}, {0x7F,0x49,0x49,0x49,0x41}, {0x7F,0x09,0x09,0x01,0x01}, // D E F
  {0x3E,0x41,0x41,0x51,0x32}, {0x7F,0x08,0x08,0x08,0x7F}, {0x00,0x41,0x7F,0x41,0x00}, // G H I
  {0x20,0x40,0x41,0x3F,0x01}, {0x7F,0x08,0x14,0x22,0x41}, {0x7F,0x40,0x40,0x40,0x40}, // J K L
  {0x7F,0x02,0x04,0x02,0x7F}, {0x7F,0x04,0x08,0x10,0x7F}, {0x3E,0x41,0x41,0x41,0x3E}, // M N O
  {0x7F,0x09,0x09,0x09,0x06}, {0x3E,0x41,0x51,0x21,0x5E}, {0x7F,0x09,0x19,0x29,0x46}, // P Q R
  {0x46,0x49,0x49,0x49,0x31}, {0x01,0x01,0x7F,0x01,0x01}, {0x3F,0x40,0x40,0x40,0x3F}, // S T U
  {0x1F,0x20,0x40,0x20,0x1F}, {0x7F,0x20,0x18,0x20,0x7F}, {0x63,0x14,0x08,0x14,0x63}, // V W X
  {0x03,0x04,0x78,0x04,0x03}, {0x61,0x51,0x49,0x45,0x43}, {0x00,0x7F,0x41,0x41,0x00}, // Y Z [
  {0x02,0x04,0x08,0x10,0x20}, {0x00,0x41,0x41,0x7F,0x00}, {0x04,0x02,0x01,0x02,0x04}, // \ ] ^
  {0x40,0x40,0x40,0x40,0x40}, {0x00,0x01,0x02,0x04,0x00}, {0x20,0x54,0x54,0x54,0x78}, // _ ` a
  {0x7F,0x48,0x44,0x44,0x38}, {0x38,0x44,0x44,0x44,0x20}, {0x38,0x44,0x44,0x48,0x7F}, // b c d
  {0x38,0x54,0x54,0x54,0x18}, {0x08,0x7E,0x09,0x01,0x02}, {0x08,0x14,0x54,0x54,0x3C}, // e f g
  {0x7F,0x08,0x04,0x04,0x78}, {0x00,0x44,0x7D,0x40,0x00}, {0x20,0x40,0x44,0x3D,0x00}, // h i j
  {0x00,0x7F,0x10,0x28,0x44}, {0x00,0x41,0x7F,0x40,0x00}, {0x7C,0x04,0x18,0x04,0x78}, // k l m
  {0x7C,0x08,0x04,0x04,0x78}, {0x38,0x44,0x44,0x44,0x38}, {0x7C,0x14,0x14,0x14,0x08}, // n o p
  {0x08,0x14,0x14,0x18,0x7C}, {0x7C,0x08,0x04,0x04,0x08}, {0x48,0x54,0x54,0x54,0x20}, // q r s
  {0x04,0x3F,0x44,0x40,0x20}, {0x3C,0x40,0x40,0x20,0x7C}, {0x1C,0x20,0x40,0x20,0x1C}, // t u v
  {0x3C,0x40,0x30,0x40,0x3C}, {0x44,0x28,0x10,0x28,0x44}, {0x0C,0x50,0x50,0x50,0x3C}, // w x y
  {0x44,0x64,0x54,0x4C,0x44}, {0x00,0x08,0x36,0x41,0x00}, {0x00,0x00,0x7F,0x00,0x00}, // z { |
  {0x00,0x41,0x36,0x08,0x00}, {0x08,0x04,0x08,0x10,0x08},                             // } ~
};
// Extra symbols, used in strings as \x01, \x02 and \x03.
static const uint8_t SYMBOLS[][5] = {
  {0x7F,0x3E,0x1C,0x08,0x00}, // \x01 forward
  {0x08,0x1C,0x3E,0x7F,0x00}, // \x02 reverse
  {0x22,0x14,0x7F,0x14,0x22}, // \x03 star: allocator and clock master
};

static uint8_t frame[8][128];
static uint8_t shown[8][128];
static bool oledReady = false;

static const uint8_t *glyph(char c) {
  if (c >= 1 && c <= 3)
    return SYMBOLS[c - 1];
  if (c < 0x20 || c > 0x7E)
    c = '?';
  return FONT[c - 0x20];
}

static void oledCommands(const uint8_t *commands, size_t count) {
  Wire.beginTransmission(HIVE_RAIL_OLED_ADDRESS);
  Wire.write(0x00); // command stream
  Wire.write(commands, count);
  Wire.endTransmission();
}

static void oledBegin() {
  // SSD1306 datasheet start-up for a 128x64 module with the internal charge pump.
  static const uint8_t INIT[] = {
    0xAE,       // display off
    0xD5, 0x80, // clock divide
    0xA8, 0x3F, // multiplex 64
    0xD3, 0x00, // no display offset
    0x40,       // start line 0
    0x8D, 0x14, // charge pump on
    0x20, 0x02, // page addressing
    0xA1,       // column 127 is SEG0
    0xC8,       // scan COM63 to COM0
    0xDA, 0x12, // COM pins
    0x81, 0x7F, // contrast
    0xD9, 0xF1, // pre-charge
    0xDB, 0x40, // VCOMH
    0xA4,       // show RAM
    0xA6,       // not inverted
    0xAF,       // display on
  };
  Wire.setClock(400000);
  oledCommands(INIT, sizeof(INIT));
  memset(shown, 0xFF, sizeof(shown)); // force the first full draw
  oledReady = true;
}

// Sends one changed page; returns false when the screen is up to date.
static bool oledFlushOnePage() {
  for (uint8_t page = 0; page < 8; page++) {
    if (memcmp(frame[page], shown[page], 128) == 0)
      continue;
    const uint8_t address[] = {(uint8_t)(0xB0 | page),
                               (uint8_t)(OLED_COLUMN_OFFSET & 0x0F),
                               (uint8_t)(0x10 | OLED_COLUMN_OFFSET >> 4)};
    oledCommands(address, sizeof(address));
    // The ESP32 Wire buffer is 128 bytes, so send the page in pieces.
    for (uint8_t column = 0; column < 128; column += 32) {
      Wire.beginTransmission(HIVE_RAIL_OLED_ADDRESS);
      Wire.write(0x40); // data stream
      Wire.write(&frame[page][column], 32);
      Wire.endTransmission();
    }
    memcpy(shown[page], frame[page], 128);
    return true;
  }
  return false;
}

// Small text: 6x8 cells on page `page`. Inverted text lights the whole cell.
static void text(uint8_t x, uint8_t page, const char *s, bool inverted = false) {
  for (; *s && x + 6 <= 128; s++, x += 6) {
    const uint8_t *g = glyph(*s);
    for (uint8_t col = 0; col < 6; col++) {
      uint8_t bits = col < 5 ? g[col] : 0;
      frame[page][x + col] = inverted ? (uint8_t)~bits : bits;
    }
  }
}

// Large text, twice the size: 12x16 cells over pages `page` and `page + 1`.
static void bigText(uint8_t x, uint8_t page, const char *s, bool inverted = false) {
  for (; *s && x + 12 <= 128; s++, x += 12) {
    const uint8_t *g = glyph(*s);
    for (uint8_t col = 0; col < 6; col++) {
      uint8_t bits = col < 5 ? g[col] : 0;
      // Double every bit vertically: the top 4 bits fill the upper page.
      uint16_t tall = 0;
      for (uint8_t bit = 0; bit < 8; bit++)
        if (bits & (1 << bit))
          tall |= 3 << (bit * 2);
      if (inverted)
        tall = ~tall;
      for (uint8_t dx = 0; dx < 2; dx++) {
        frame[page][x + col * 2 + dx] = tall & 0xFF;
        frame[page + 1][x + col * 2 + dx] = tall >> 8;
      }
    }
  }
}

static void fillPage(uint8_t page, uint8_t x0, uint8_t x1, uint8_t bits) {
  for (uint8_t x = x0; x < x1; x++)
    frame[page][x] = bits;
}

// ---------------------------------------------------------------------------------------
// The screen.

static char wifiName[33] = "";
static char wifiPassword[65] = "";
static char ipAddress[16] = "";

static void readWifi() {
  if (WiFi.getMode() & WIFI_MODE_AP && WiFi.status() != WL_CONNECTED) {
    strncpy(wifiName, WiFi.softAPSSID().c_str(), sizeof(wifiName) - 1);
    wifi_config_t config;
    if (esp_wifi_get_config(WIFI_IF_AP, &config) == ESP_OK)
      strncpy(wifiPassword, (const char *)config.ap.password, sizeof(wifiPassword) - 1);
    strncpy(ipAddress, WiFi.softAPIP().toString().c_str(), sizeof(ipAddress) - 1);
  } else {
    strncpy(wifiName, WiFi.SSID().c_str(), sizeof(wifiName) - 1);
    wifiPassword[0] = '\0'; // the home network's password is not shown
    strncpy(ipAddress, WiFi.localIP().toString().c_str(), sizeof(ipAddress) - 1);
  }
}

static void render(District power, bool blink) {
  memset(frame, 0, sizeof(frame));
  char line[72]; // longer than the screen; text() stops at the edge
  bool link = linkUp();

  // Page 0: title bar, inverted: name, RocNet ID, star for allocator and clock master.
  fillPage(0, 0, 128, 0xFF);
  text(2, 0, HIVE_RAIL_NAME, true);
  if (status.seen) {
    snprintf(line, sizeof(line), "RN%d%s", status.rocnetId,
             link && status.flags & FLAG_ALLOCATOR ? " \x03" : "");
    text(126 - 6 * strlen(line), 0, line, true);
  }

  // Pages 1-2: fast clock and district power, twice the size.
  if (clockMinutes >= 0) {
    snprintf(line, sizeof(line), "%02d:%02d", clockMinutes / 60 % 24, clockMinutes % 60);
    bigText(0, 1, line);
  } else {
    bigText(0, 1, "--:--");
  }
  const char *powerLabel = "OFF";
  bool powerInverted = false;
  if (power == District::Short) {
    powerLabel = "SHORT";
    powerInverted = blink;
  } else if (power == District::On) {
    powerLabel = link && status.flags & FLAG_STOP ? "STOP" : "ON";
  }
  bigText(128 - 12 * strlen(powerLabel), 1, powerLabel, powerInverted);

  // Page 3: small labels under the large fields.
  if (clockMinutes < 0)
    text(0, 3, "fast clock");
  else if (clockRate == 0)
    text(0, 3, "paused");
  else {
    snprintf(line, sizeof(line), "fast x%d", clockRate);
    text(0, 3, line);
  }
  text(128 - 6 * 8, 3, "district");

  // Page 4: a thin line between the large fields and the status lines.
  fillPage(4, 0, 128, 0x08);

  // Pages 5-7: Hive-Rail status, or what is needed to connect.
  if (link) {
    snprintf(line, sizeof(line), "Mesh %d node%s", status.nodes, status.nodes == 1 ? "" : "s");
    text(0, 5, line);
    if (status.flags & FLAG_COMPARED) {
      bool sync = status.flags & FLAG_SLOTS_SYNC;
      text(128 - 6 * 8, 5, sync ? "slots ok" : "SLOTS !!", !sync && blink);
    }
    if (status.locoAddress > 0) {
      int speed = status.locoSpeed;
      if (speed == 1)
        snprintf(line, sizeof(line), "Loco %d %c ESTOP", status.locoAddress,
                 status.locoForward ? '\x01' : '\x02');
      else
        snprintf(line, sizeof(line), "Loco %d %c %d", status.locoAddress,
                 status.locoForward ? '\x01' : '\x02', speed > 1 ? speed - 1 : 0);
      text(0, 6, line);
      for (uint8_t f = 0; f < 5; f++)
        if (status.locoFunctions & 1 << f) {
          char fn[3] = {'F', (char)('0' + f), 0};
          text(128 - 6 * 2, 6, fn); // the lowest function that is on
          break;
        }
    } else {
      snprintf(line, sizeof(line), "Locos %d  slots %d", status.locos, status.slots);
      text(0, 6, line);
    }
    snprintf(line, sizeof(line), "IP %s", ipAddress);
    text(0, 7, line);
  } else {
    if (status.seen) {
      fillPage(5, 0, 128, blink ? 0xFF : 0x00);
      text(1, 5, "HIVE-RAIL LINK LOST", blink);
    } else {
      text(0, 5, "Waiting for Hive-Rail");
    }
    snprintf(line, sizeof(line), "WiFi %s", wifiName);
    text(0, 6, line);
    // Password and address take turns every 3 s.
    if (wifiPassword[0] && millis() / 3000 % 2)
      snprintf(line, sizeof(line), "Pass %s", wifiPassword);
    else
      snprintf(line, sizeof(line), "IP %s", ipAddress);
    text(0, 7, line);
  }
}

// ---------------------------------------------------------------------------------------
// The HAL device that runs it all from DCC-EX's main loop.

static bool setupDone = false;

class HiveRailBoard : public IODevice {
public:
  HiveRailBoard() { addDevice(this); }

protected:
  void _loop(unsigned long currentMicros) override {
    if (!setupDone) {
      delayUntil(currentMicros + 10000);
      return;
    }
    uint32_t now = millis();
    if (!oledReady) {
      pinMode(HIVE_RAIL_HEARTBEAT_LED, OUTPUT);
      ws2812Begin();
      readWifi();
      oledBegin();
    }
    bool blink = now / 250 % 2;
    // Heartbeat: 100 ms on per second.
    digitalWrite(HIVE_RAIL_HEARTBEAT_LED, now % 1000 < 100 ? HIGH : LOW);

    if (now - lastFrameMs >= 100) {
      lastFrameMs = now;
      District power = districtPower();
      // LED 1: district power. LED 2: Hive-Rail link (green up, red blinking lost).
      Colour powerColour = POWER_OFF;
      if (power == District::Short)
        powerColour = blink ? POWER_SHORT : DARK;
      else if (power == District::On)
        powerColour = linkUp() && status.flags & FLAG_STOP ? POWER_STOP : POWER_ON;
      Colour linkColour = linkUp() ? POWER_ON : (status.seen && blink ? POWER_OFF : DARK);
      const Colour leds[WS2812_COUNT] = {powerColour, linkColour, DARK};
      ws2812Show(leds);
      if (now - lastWifiMs >= 5000) {
        lastWifiMs = now;
        readWifi();
      }
      render(power, blink);
    }
    // One changed page per call keeps every call short (about 3 ms at 400 kHz).
    delayUntil(currentMicros + (oledFlushOnePage() ? 2000 : 20000));
  }

  void _display() override { DIAG(F("Hive-Rail board: OLED, status LEDs")); }

private:
  uint32_t lastFrameMs = 0;
  uint32_t lastWifiMs = 0;
};

// Called by DCC-EX's HAL start-up (the myHal.cpp hook).
void halSetup() { new HiveRailBoard(); }

// Called from mySetup.h at the end of DCC-EX's setup(), once WiFi and DCC are running.
void hiveRailBoardBegin() {
  DIAG(F("Hive-Rail board add-on started (protocol %d)"), BOARD_PROTOCOL);
  setupDone = true;
}
