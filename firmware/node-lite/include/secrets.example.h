// Copy to secrets.h (not committed) and fill in.
#pragma once

#define WIFI_SSID "your-network"
#define WIFI_PASS "your-password"

// The Hive-Rail node to connect to as a Zenoh client: a node started with
// --listen tcp/0.0.0.0:7447 on the computer at this address.
#define HIVE_LOCATOR "tcp/192.168.60.127:7447"

// Password for firmware updates over Wi-Fi (ArduinoOTA).
#define HIVE_OTA_PASS "choose-one"

#define HIVE_SESSION "bench"
#define HIVE_NODE_ID "M5StickC"
