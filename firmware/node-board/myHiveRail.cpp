// Hive-Rail add-on for DCC-EX on the node board.
// Runs next to DCC-EX in its own ESP32 task, so DCC-EX's code is not changed.
// Step 1: prove the hook and the build with a heartbeat LED. The LocoNet buffer follows.

#include <Arduino.h>
#include "DIAG.h"

#ifndef HIVE_RAIL_STATUS_LED
#define HIVE_RAIL_STATUS_LED 2 // blue LED on an ESP32 DevKit V1
#endif

static void heartbeatTask(void *) {
  pinMode(HIVE_RAIL_STATUS_LED, OUTPUT);
  for (;;) {
    digitalWrite(HIVE_RAIL_STATUS_LED, HIGH);
    vTaskDelay(pdMS_TO_TICKS(100));
    digitalWrite(HIVE_RAIL_STATUS_LED, LOW);
    vTaskDelay(pdMS_TO_TICKS(900));
  }
}

void hiveRailBoardBegin() {
  DIAG(F("Hive-Rail board add-on started"));
  // Core 0, away from DCC-EX's loop on core 1.
  xTaskCreatePinnedToCore(heartbeatTask, "hive-rail", 2048, nullptr, 1, nullptr, 0);
}
