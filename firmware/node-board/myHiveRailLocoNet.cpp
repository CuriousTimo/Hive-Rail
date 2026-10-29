// LocoNet buffer for the node board: the board's LocoNet port (UART2) bridged to an
// LbServer on TCP port 1234, next to DCC-EX and independent of it. Hive-Rail connects
// with --loconet-connect <board>:1234; JMRI or Rocrail could connect the same way.
//
// LbServer text protocol: a client sends "SEND <hex>"; the board puts the message on
// the bus and answers "RECEIVE <hex>" to every client (the bus echo) and "SENT OK" or
// "SENT ERROR" to the sender. Every other bus message goes to every client as
// "RECEIVE <hex>".
//
// Sending follows the LocoNet Personal Edition: wait until the bus has been quiet for
// the carrier detect time plus a priority delay, send, and compare the echo byte by
// byte; on a difference send a break and try again.

#include <Arduino.h>
#include <WiFi.h>
#include <driver/gpio.h>
#include <driver/uart.h>
#include <soc/uart_struct.h>
#include "DIAG.h"

#ifndef HIVE_RAIL_LOCONET_RX
#define HIVE_RAIL_LOCONET_RX 16 // LM393 receiver on the 2021 board
#endif
#ifndef HIVE_RAIL_LOCONET_TX
#define HIVE_RAIL_LOCONET_TX 17 // 2N3904 that pulls the bus low
#endif
// The transistor inverts the transmit line; the comparator leaves receive as it is.
#ifndef HIVE_RAIL_LOCONET_RX_INVERT
#define HIVE_RAIL_LOCONET_RX_INVERT 0
#endif
#ifndef HIVE_RAIL_LOCONET_TX_INVERT
#define HIVE_RAIL_LOCONET_TX_INVERT 1
#endif
#ifndef HIVE_RAIL_LOCONET_PORT
#define HIVE_RAIL_LOCONET_PORT 1234
#endif

static const uart_port_t LOCONET_UART = UART_NUM_2;
static const int LOCONET_BAUD = 16666;  // 60 us per bit
// 0x00 at this rate holds the line low for 900 us: the 15-bit LocoNet break.
static const int BREAK_BAUD = 10000;
// Carrier detect (20 bit times) plus a priority delay of 6 bit times.
static const uint32_t QUIET_BEFORE_SEND_US = 26 * 60;
static const int SEND_ATTEMPTS = 6;
static const uint32_t QUIET_WAIT_MS = 200;
// The line idles high; low for this long means the bus has no power.
static const uint32_t NO_POWER_MS = 50;
static const int MAX_CLIENTS = 4;
static const size_t MAX_LINE = 400;

// Read by the screen in myHiveRail.cpp.
volatile int hiveRailLocoNetClients = 0;
volatile bool hiveRailLocoNetPowered = false;

static uint32_t lastRxUs = 0;
static uint32_t lowSinceMs = 0;

// ---------------------------------------------------------------------------------------
// Message framing: the opcode (bit 7 set) gives the length; the checksum makes all
// bytes XOR to 0xFF.

struct Framer {
  uint8_t bytes[128];
  uint8_t length = 0;
  uint8_t expected = 0;

  // Returns true when bytes[0..length) holds a complete, valid message.
  bool push(uint8_t b) {
    if (b & 0x80) {
      bytes[0] = b;
      length = 1;
      static const uint8_t SIZES[] = {2, 4, 6, 0}; // 0: the next byte is the size
      expected = SIZES[(b >> 5) & 3];
      return false;
    }
    if (length == 0 || length >= sizeof(bytes)) {
      length = 0; // data without an opcode belongs to a message we missed
      return false;
    }
    bytes[length++] = b;
    if (expected == 0 && length == 2) {
      expected = b;
      if (expected < 2) {
        length = 0;
        return false;
      }
    }
    if (expected == 0 || length < expected)
      return false;
    uint8_t sum = 0;
    for (uint8_t i = 0; i < length; i++)
      sum ^= bytes[i];
    bool valid = sum == 0xFF;
    if (!valid)
      length = 0;
    return valid;
  }
};

static Framer framer;

static String toHex(const uint8_t *bytes, size_t length) {
  String hex;
  char part[4];
  for (size_t i = 0; i < length; i++) {
    snprintf(part, sizeof(part), i ? " %02X" : "%02X", bytes[i]);
    hex += part;
  }
  return hex;
}

// ---------------------------------------------------------------------------------------
// TCP clients (LbServer).

static WiFiServer server(HIVE_RAIL_LOCONET_PORT);
static WiFiClient clients[MAX_CLIENTS];
static String lines[MAX_CLIENTS];

static void toClients(const String &line) {
  for (WiFiClient &client : clients)
    if (client && client.connected())
      client.print(line);
}

static void received(const uint8_t *bytes, size_t length) {
  toClients("RECEIVE " + toHex(bytes, length) + "\r\n");
}

// ---------------------------------------------------------------------------------------
// The bus.

static bool receiverBusy() { return UART2.status.st_urx_out != 0; }

static void watchPower() {
  bool level = gpio_get_level((gpio_num_t)HIVE_RAIL_LOCONET_RX);
  if (HIVE_RAIL_LOCONET_RX_INVERT)
    level = !level;
  if (level) {
    lowSinceMs = 0;
    hiveRailLocoNetPowered = true;
  } else {
    if (lowSinceMs == 0)
      lowSinceMs = millis() | 1;
    if (millis() - lowSinceMs >= NO_POWER_MS)
      hiveRailLocoNetPowered = false;
  }
}

// Reads what the bus delivered and passes complete messages to the clients.
static void readBus() {
  uint8_t chunk[32];
  int count;
  while ((count = uart_read_bytes(LOCONET_UART, chunk, sizeof(chunk), 0)) > 0) {
    lastRxUs = micros();
    for (int i = 0; i < count; i++)
      if (framer.push(chunk[i])) {
        received(framer.bytes, framer.length);
        framer.length = 0;
      }
  }
}

static void sendBreak() {
  static const uint8_t zero = 0;
  uart_wait_tx_done(LOCONET_UART, pdMS_TO_TICKS(10));
  uart_set_baudrate(LOCONET_UART, BREAK_BAUD);
  uart_write_bytes(LOCONET_UART, (const char *)&zero, 1);
  uart_wait_tx_done(LOCONET_UART, pdMS_TO_TICKS(10));
  uart_set_baudrate(LOCONET_UART, LOCONET_BAUD);
  // Drop whatever came in during the break.
  uart_flush_input(LOCONET_UART);
  framer.length = 0;
  lastRxUs = micros();
}

// Puts one message on the bus; true once its echo came back unchanged.
static bool transmit(const uint8_t *bytes, size_t length) {
  if (!hiveRailLocoNetPowered)
    return false;
  for (int attempt = 0; attempt < SEND_ATTEMPTS; attempt++) {
    uint32_t waitStart = millis();
    for (;;) {
      readBus();
      if (!receiverBusy() && micros() - lastRxUs >= QUIET_BEFORE_SEND_US)
        break;
      if (millis() - waitStart > QUIET_WAIT_MS)
        return false;
      delayMicroseconds(100);
    }
    uart_write_bytes(LOCONET_UART, (const char *)bytes, length);
    bool echoed = true;
    for (size_t i = 0; i < length && echoed; i++) {
      uint8_t b;
      echoed = uart_read_bytes(LOCONET_UART, &b, 1, pdMS_TO_TICKS(5)) == 1 && b == bytes[i];
    }
    lastRxUs = micros();
    if (echoed)
      return true;
    sendBreak(); // collision: another device sent at the same time
    vTaskDelay(pdMS_TO_TICKS(1 + esp_random() % 4));
  }
  return false;
}

// ---------------------------------------------------------------------------------------

static bool parseHex(const String &hex, uint8_t *bytes, size_t &length) {
  length = 0;
  int i = 0;
  while (i < (int)hex.length() && length < 128) {
    while (i < (int)hex.length() && hex[i] == ' ')
      i++;
    if (i >= (int)hex.length())
      break;
    char *end;
    long value = strtol(hex.c_str() + i, &end, 16);
    int used = end - (hex.c_str() + i);
    if (used == 0 || value < 0 || value > 0xFF)
      return false;
    bytes[length++] = value;
    i += used;
  }
  uint8_t sum = 0;
  for (size_t j = 0; j < length; j++)
    sum ^= bytes[j];
  return length >= 2 && (bytes[0] & 0x80) && sum == 0xFF;
}

static void handleLine(int index, const String &line) {
  if (!line.startsWith("SEND "))
    return;
  uint8_t bytes[128];
  size_t length;
  if (!parseHex(line.substring(5), bytes, length)) {
    clients[index].print("SENT ERROR invalid message\r\n");
    return;
  }
  if (transmit(bytes, length)) {
    received(bytes, length); // the bus echo, to every client
    clients[index].print("SENT OK\r\n");
  } else {
    clients[index].print(hiveRailLocoNetPowered ? "SENT ERROR bus busy\r\n"
                                                 : "SENT ERROR no LocoNet power\r\n");
  }
}

static void serveClients() {
  WiFiClient incoming = server.available();
  if (incoming) {
    int free = -1;
    for (int i = 0; i < MAX_CLIENTS; i++)
      if (!clients[i] || !clients[i].connected()) {
        free = i;
        break;
      }
    if (free < 0) {
      incoming.stop();
    } else {
      clients[free] = incoming;
      clients[free].setNoDelay(true);
      lines[free] = "";
      clients[free].print("VERSION Hive-Rail node board LocoNet buffer\r\n");
    }
  }
  int connected = 0;
  for (int i = 0; i < MAX_CLIENTS; i++) {
    if (!clients[i] || !clients[i].connected())
      continue;
    connected++;
    while (clients[i].available()) {
      char c = clients[i].read();
      if (c == '\r' || c == '\n') {
        if (lines[i].length())
          handleLine(i, lines[i]);
        lines[i] = "";
      } else if (lines[i].length() < MAX_LINE) {
        lines[i] += c;
      }
    }
  }
  hiveRailLocoNetClients = connected;
}

static void loconetTask(void *) {
  for (;;) {
    watchPower();
    readBus();
    serveClients();
    vTaskDelay(1);
  }
}

// Called once DCC-EX's setup() has started WiFi.
void hiveRailLocoNetBegin() {
  uart_config_t config = {};
  config.baud_rate = LOCONET_BAUD;
  config.data_bits = UART_DATA_8_BITS;
  config.parity = UART_PARITY_DISABLE;
  config.stop_bits = UART_STOP_BITS_1;
  config.flow_ctrl = UART_HW_FLOWCTRL_DISABLE;
  config.source_clk = UART_SCLK_APB;
  ESP_ERROR_CHECK(uart_driver_install(LOCONET_UART, 512, 0, 0, nullptr, 0));
  ESP_ERROR_CHECK(uart_param_config(LOCONET_UART, &config));
  ESP_ERROR_CHECK(uart_set_pin(LOCONET_UART, HIVE_RAIL_LOCONET_TX, HIVE_RAIL_LOCONET_RX,
                               UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));
  uint32_t inverse = UART_SIGNAL_INV_DISABLE;
  if (HIVE_RAIL_LOCONET_RX_INVERT)
    inverse |= UART_SIGNAL_RXD_INV;
  if (HIVE_RAIL_LOCONET_TX_INVERT)
    inverse |= UART_SIGNAL_TXD_INV;
  uart_set_line_inverse(LOCONET_UART, inverse);
  // Hand every byte over at once, so the bus timing is seen as it happens.
  uart_set_rx_full_threshold(LOCONET_UART, 1);
  uart_set_rx_timeout(LOCONET_UART, 1);

  server.begin();
  server.setNoDelay(true);
  // Core 0, away from DCC-EX's loop on core 1.
  xTaskCreatePinnedToCore(loconetTask, "loconet", 6144, nullptr, 2, nullptr, 0);
  DIAG(F("Hive-Rail LocoNet buffer on UART2 (RX %d, TX %d), LbServer on port %d"),
       HIVE_RAIL_LOCONET_RX, HIVE_RAIL_LOCONET_TX, HIVE_RAIL_LOCONET_PORT);
}
