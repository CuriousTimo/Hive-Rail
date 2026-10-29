// DCC-EX configuration for the Hive-Rail node board.
// Copied into a DCC-EX (EX-CommandStation) checkout by build.sh; DCC-EX ignores config.h in git.

// Prototype: the 2021 "ESP32 LocoNet Command Station" board (also fine on a bare ESP32 DevKit).
// A = MAIN: DCC to LocoNet-B RailSync via MIC4426, no current sense (the boosters measure).
// B = PROG: L6202, second input driven through a 74HC04, no usable current sense.
// MotorDriver(enable, signal, signal2, brake, current sense, sense factor, trip mA, fault).
// A negative enable pin makes it active-low; which polarity removes RailSync DCC on <0> is
// still to be tested on the board.
#define HIVE_RAIL_TEST_BOARD F("HIVERAIL_ESP32"), \
  new MotorDriver(32, 25, UNUSED_PIN, UNUSED_PIN, UNUSED_PIN, 1.0, 5000, UNUSED_PIN), \
  new MotorDriver(33, 26, UNUSED_PIN, UNUSED_PIN, UNUSED_PIN, 1.0, 250, UNUSED_PIN)
#define MOTOR_SHIELD_TYPE HIVE_RAIL_TEST_BOARD

// WiFi: without real credentials DCC-EX starts its own access point (DCCEX_xxxxxx).
#define IP_PORT 2560
#define ENABLE_WIFI true
#define WIFI_SSID "Your network name"
#define WIFI_PASSWORD "Your network passwd"
#define WIFI_HOSTNAME "dccex"
#define WIFI_CHANNEL 1
#define WIFI_FORCE_AP false

// The 128x64 OLED on I2C (IO21 SDA, IO22 SCL) is driven by the Hive-Rail add-on, not by
// DCC-EX, so there is no OLED_DRIVER here. HIVE_RAIL_NAME is the title on the screen;
// define HIVE_RAIL_OLED_SH1106 for an SH1106 display.
#define HIVE_RAIL_NAME "Hive-Rail"
