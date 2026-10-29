// Hive-Rail add-on for DCC-EX on the node board.
// Runs next to DCC-EX in its own ESP32 task, so DCC-EX's code is not changed.
// Status: heartbeat LED, and WS2812 LED 1 showing the district's power. The LocoNet buffer follows.

#include <Arduino.h>
#include <driver/rmt.h>
#include "DIAG.h"
#include "TrackManager.h"

#ifndef HIVE_RAIL_HEARTBEAT_LED
#define HIVE_RAIL_HEARTBEAT_LED 2 // blue LED on an ESP32 DevKit V1
#endif
#ifndef HIVE_RAIL_WS2812_PIN
#define HIVE_RAIL_WS2812_PIN 4 // three WS2812 on the 2021 board
#endif
#define WS2812_COUNT 3
// DCC-EX uses RMT channels 0-1 (MAIN) and 2-3 (PROG); 6-7 are free.
#define WS2812_RMT_CHANNEL RMT_CHANNEL_6
// Not full brightness: about 10 %.
#define LEVEL 26

struct Colour {
  uint8_t red, green, blue;
};
static const Colour DARK = {0, 0, 0};
static const Colour POWER_ON = {0, LEVEL, 0};
static const Colour POWER_OFF = {LEVEL, 0, 0};
static const Colour POWER_SHORT = {LEVEL, LEVEL / 3, 0};

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
  // The latch needs > 50 us low; the task delay below is far longer.
}

// MAIN power as DCC-EX's track manager sees it. Only reads the state; the tracks are
// configured before this task starts.
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

static void statusTask(void *) {
  pinMode(HIVE_RAIL_HEARTBEAT_LED, OUTPUT);
  ws2812Begin();
  for (uint32_t tick = 0;; tick++) {
    // Heartbeat: 100 ms on per second.
    digitalWrite(HIVE_RAIL_HEARTBEAT_LED, tick % 20 < 2 ? HIGH : LOW);

    // LED 1: district power. A short blinks orange, 250 ms on and off.
    Colour power = POWER_OFF;
    switch (districtPower()) {
      case District::On: power = POWER_ON; break;
      case District::Short: power = tick % 10 < 5 ? POWER_SHORT : DARK; break;
      case District::Off: break;
    }
    const Colour leds[WS2812_COUNT] = {power, DARK, DARK};
    ws2812Show(leds);
    vTaskDelay(pdMS_TO_TICKS(50));
  }
}

void hiveRailBoardBegin() {
  DIAG(F("Hive-Rail board add-on started"));
  // Core 0, away from DCC-EX's loop on core 1.
  xTaskCreatePinnedToCore(statusTask, "hive-rail", 4096, nullptr, 1, nullptr, 0);
}
