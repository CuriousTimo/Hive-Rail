// Hive-Rail Node-Lite: our lite node test and debug device (phase B0), a zenoh-pico client on
// an ESP32. It sends the Rust nodes' heartbeat (hive/<session>/node/<id>/hb) every 250 ms, marked
// "lite" with the node it reaches the mesh through ("via"), and follows the other nodes'
// heartbeats. It also plays its district's command station with fake hardware: district power
// on its buttons, a faked short, and the mesh-wide global emergency stop both ways.
// On an M5StickC: side button B = power on/off (hold 2 s: short circuit), front button A = next
// page (hold 2 s: GLOBAL emergency stop); the red LED shows the district power.

#include <Arduino.h>
#include <ArduinoJson.h>
#include <WiFi.h>
#include <zenoh-pico.h>

#include "secrets.h"

#ifdef HIVE_SCREEN
#include <M5Unified.h>
#endif
#include <esp_freertos_hooks.h>
#ifdef HIVE_OTA
#include <ArduinoOTA.h>
#ifndef HIVE_OTA_PASS
#error "HIVE_OTA_PASS missing in include/secrets.h (see secrets.example.h)"
#endif
#endif

// As the Rust nodes: a heartbeat every 250 ms; they call a peer stale after 750 ms.
static const uint32_t HEARTBEAT_MS = 250;
static const uint32_t STALE_MS = 750;
static const uint16_t ROCNET_ID = 199;

static z_owned_session_t session;
static z_owned_publisher_t heartbeat;
static z_owned_subscriber_t peers;
static z_owned_subscriber_t global_stop;
static z_owned_publisher_t global_stop_out;
static z_owned_publisher_t power_status;
static z_owned_publisher_t events;

// District power, as the Rust nodes' labels: OFF, ON, STOP (emergency stop), SHORT.
enum Power { POWER_OFF, POWER_ON, POWER_STOP, POWER_SHORT };
static const char *POWER_LABEL[] = {"OFF", "ON", "STOP", "SHORT"};
static volatile Power power = POWER_OFF;
static volatile bool power_changed = false;
static char last_event[40] = "";
// A global stop received from the mesh, applied by loop() (set in the receive task).
static volatile bool stop_from_mesh = false;
static char stop_origin[24] = "";
static bool session_open = false;
static uint32_t seq = 0;
static uint32_t received = 0;
static char own_zid[33] = "";

// The nodes seen in heartbeats, written by zenoh-pico's receive task, read by loop().
struct Node {
    char name[24];
    char power[8];
    char zid[33];
    uint16_t rocnet;
    bool lite;
    uint32_t last_ms;
};
static const int MAX_NODES = 12;
static Node nodes[MAX_NODES];
static int node_count = 0;
static portMUX_TYPE nodes_lock = portMUX_INITIALIZER_UNLOCKED;

// The Zenoh ID of the node this client is connected to, and that node's name.
static char via_zid[33] = "";
static char via[24] = "";

// CPU load per core, estimated: each core's idle task counts its runs; against the count of a
// fully idle second (measured at start-up), the share not idle is the load.
static volatile uint32_t idle_runs[2] = {0, 0};
static uint32_t idle_max[2] = {1, 1};
static uint8_t cpu_load[2] = {0, 0};

static bool idle_hook_0() {
    idle_runs[0]++;
    return false;
}
static bool idle_hook_1() {
    idle_runs[1]++;
    return false;
}

static void measure_cpu() {
    static uint32_t last[2] = {0, 0};
    for (int core = 0; core < 2; core++) {
        uint32_t runs = idle_runs[core];
        uint32_t idle = runs - last[core];
        last[core] = runs;
        uint32_t load = idle >= idle_max[core] ? 0 : 100 - idle * 100 / idle_max[core];
        cpu_load[core] = load;
    }
}

// A Zenoh ID as the Rust nodes print it: the 16 bytes as a little-endian number, in hex
// without leading zeros.
static void zid_text(const z_id_t *id, char *out) {
    int start = 15;
    while (start > 0 && id->id[start] == 0) {
        start--;
    }
    char *p = out;
    p += sprintf(p, "%x", id->id[start]);
    for (int i = start - 1; i >= 0; i--) {
        p += sprintf(p, "%02x", id->id[i]);
    }
}

static void on_heartbeat(z_loaned_sample_t *sample, void *) {
    z_owned_string_t text;
    z_bytes_to_string(z_sample_payload(sample), &text);
    JsonDocument doc;
    DeserializationError error =
        deserializeJson(doc, z_string_data(z_string_loan(&text)), z_string_len(z_string_loan(&text)));
    z_string_drop(z_string_move(&text));
    if (error) {
        return;
    }
    const char *name = doc["node"] | "";
    if (strcmp(name, HIVE_NODE_ID) == 0 || name[0] == 0) {
        return;
    }
    Node seen = {};
    strlcpy(seen.name, name, sizeof(seen.name));
    strlcpy(seen.power, doc["power"] | "?", sizeof(seen.power));
    strlcpy(seen.zid, doc["zid"] | "", sizeof(seen.zid));
    seen.rocnet = doc["rocnet_id"] | 0;
    seen.lite = doc["lite"] | false;
    seen.last_ms = millis();

    portENTER_CRITICAL(&nodes_lock);
    received++;
    int i = 0;
    while (i < node_count && strcmp(nodes[i].name, seen.name) != 0) {
        i++;
    }
    if (i < MAX_NODES) {
        nodes[i] = seen;
        if (i == node_count) {
            node_count++;
        }
    }
    portEXIT_CRITICAL(&nodes_lock);
}

// zenoh-pico reports the node it is connected to as a peer or a router.
static void on_connected_zid(const z_id_t *id, void *) {
    zid_text(id, via_zid);
}

static void find_via() {
    via_zid[0] = 0;
    z_owned_closure_zid_t peers_cb;
    z_closure_zid(&peers_cb, on_connected_zid, NULL, NULL);
    z_info_peers_zid(z_session_loan(&session), z_closure_zid_move(&peers_cb));
    z_owned_closure_zid_t routers_cb;
    z_closure_zid(&routers_cb, on_connected_zid, NULL, NULL);
    z_info_routers_zid(z_session_loan(&session), z_closure_zid_move(&routers_cb));

    via[0] = 0;
    portENTER_CRITICAL(&nodes_lock);
    for (int i = 0; i < node_count; i++) {
        if (via_zid[0] && strcmp(nodes[i].zid, via_zid) == 0) {
            strlcpy(via, nodes[i].name, sizeof(via));
        }
    }
    portEXIT_CRITICAL(&nodes_lock);
}

// RocNet frames as the Rust nodes build them (rocnet.rs): network, recipient (2), sender (2),
// group, type << 5 | action, data length, data. Group 1 is the command station.
static const uint8_t RN_CS_GROUP = 1;
static const uint8_t RN_REQUEST = 0, RN_EVENT = 1;
static const uint8_t RN_CS_QUERY = 1, RN_CS_HALT = 3;

static void put_with_origin(const z_loaned_publisher_t *publisher, const uint8_t *data, size_t len) {
    z_owned_bytes_t payload;
    z_bytes_copy_from_buf(&payload, data, len);
    z_publisher_put_options_t options;
    z_publisher_put_options_default(&options);
    // The Rust nodes read the sender's name from the attachment.
    z_owned_bytes_t origin;
    z_bytes_copy_from_str(&origin, HIVE_NODE_ID);
    options.attachment = z_bytes_move(&origin);
    z_publisher_put(publisher, z_bytes_move(&payload), &options);
}

// An event for the dispatcher's list (hive/<s>/node/<id>/event), and the screen.
static void event(const char *text) {
    strlcpy(last_event, text, sizeof(last_event));
    Serial.println(text);
    char json[160];
    snprintf(json, sizeof(json), "{\"detail\":false,\"text\":\"%s\"}", text);
    z_owned_bytes_t payload;
    z_bytes_copy_from_str(&payload, json);
    z_publisher_put(z_publisher_loan(&events), z_bytes_move(&payload), NULL);
}

// The district's power status to the mesh, as a RocNet command station status event:
// bit 0 power on, bit 1 idle (an emergency stop is on and idle).
static void publish_power() {
    uint8_t status = power == POWER_ON ? 0x01 : power == POWER_STOP ? 0x03 : 0x00;
    uint8_t frame[] = {0, 0, 0, (uint8_t)(ROCNET_ID >> 8), (uint8_t)ROCNET_ID, RN_CS_GROUP,
                       (uint8_t)(RN_EVENT << 5 | RN_CS_QUERY), 1, status};
    put_with_origin(z_publisher_loan(&power_status), frame, sizeof(frame));
}

static void set_power(Power next, const char *why) {
    if (power == next) {
        return;
    }
    power = next;
    publish_power();
    char text[40];
    snprintf(text, sizeof(text), "district power %s (%s)", POWER_LABEL[next], why);
    event(text);
}

// The layout-wide emergency stop: every district goes to STOP.
static void send_global_stop() {
    uint8_t frame[] = {0, 0, 0, (uint8_t)(ROCNET_ID >> 8), (uint8_t)ROCNET_ID, RN_CS_GROUP,
                       (uint8_t)(RN_REQUEST << 5 | RN_CS_HALT), 0};
    put_with_origin(z_publisher_loan(&global_stop_out), frame, sizeof(frame));
    set_power(POWER_STOP, "GLOBAL STOP sent");
}

static void on_global_stop(z_loaned_sample_t *sample, void *) {
    uint8_t frame[16];
    z_bytes_reader_t reader = z_bytes_get_reader(z_sample_payload(sample));
    size_t len = z_bytes_reader_read(&reader, frame, sizeof(frame));
    // A halt request: group 1, type request, action 3.
    if (len < 8 || frame[5] != RN_CS_GROUP || (frame[6] >> 5) != RN_REQUEST || (frame[6] & 0x1F) != RN_CS_HALT) {
        return;
    }
    stop_origin[0] = 0;
    const z_loaned_bytes_t *attachment = z_sample_attachment(sample);
    if (attachment) {
        z_owned_string_t origin;
        if (z_bytes_to_string(attachment, &origin) == Z_OK) {
            size_t n = z_string_len(z_string_loan(&origin));
            if (n >= sizeof(stop_origin)) {
                n = sizeof(stop_origin) - 1;
            }
            memcpy(stop_origin, z_string_data(z_string_loan(&origin)), n);
            stop_origin[n] = 0;
            z_string_drop(z_string_move(&origin));
        }
    }
    stop_from_mesh = true;
}

#ifdef HIVE_SCREEN
static const int RED_LED = 10;  // M5StickC: active low
static int page = 0;

static void show(const char *status) {
    auto &d = M5.Display;
    d.startWrite();
    d.fillScreen(TFT_BLACK);
    d.setCursor(0, 0);
    d.setTextColor(TFT_BLACK, TFT_YELLOW);
    d.printf(" Node-Lite %-10s %3u ", HIVE_NODE_ID, ROCNET_ID);
    d.setTextColor(TFT_WHITE, TFT_BLACK);
    d.setCursor(0, 11);
    if (status) {
        d.setTextColor(TFT_ORANGE, TFT_BLACK);
        d.println(status);
    } else if (page == 0) {
        d.printf("WiFi %ddBm %s\n", WiFi.RSSI(), WiFi.localIP().toString().c_str());
        bool open = session_open && !z_session_is_closed(z_session_loan(&session));
        d.setTextColor(open ? TFT_GREEN : TFT_RED, TFT_BLACK);
        d.printf("Zenoh %s", open ? "open" : "CLOSED");
        d.setTextColor(TFT_WHITE, TFT_BLACK);
        d.printf("  via %s\n", via[0] ? via : "?");
        int online = 0;
        uint32_t now = millis();
        portENTER_CRITICAL(&nodes_lock);
        for (int i = 0; i < node_count; i++) {
            if (now - nodes[i].last_ms < STALE_MS) {
                online++;
            }
        }
        uint32_t rx = received;
        portEXIT_CRITICAL(&nodes_lock);
        d.printf("Mesh %d node(s) online  ", online);
        uint16_t colour = power == POWER_ON ? TFT_GREEN : power == POWER_OFF ? TFT_LIGHTGREY : TFT_RED;
        d.setTextColor(colour, TFT_BLACK);
        d.printf("%s\n", POWER_LABEL[power]);
        d.setTextColor(TFT_WHITE, TFT_BLACK);
        d.printf("hb tx %lu rx %lu\n", (unsigned long)seq, (unsigned long)rx);
        d.printf("heap %uk low %uk cpu %u/%u%%\n", ESP.getFreeHeap() / 1024, ESP.getMinFreeHeap() / 1024,
                 cpu_load[0], cpu_load[1]);
        uint32_t up = now / 1000;
        d.printf("up %luh%02lum%02lus\n", (unsigned long)(up / 3600), (unsigned long)(up / 60 % 60),
                 (unsigned long)(up % 60));
        d.setTextColor(TFT_CYAN, TFT_BLACK);
        d.printf("%.26s", last_event);
    } else {
        uint32_t now = millis();
        portENTER_CRITICAL(&nodes_lock);
        Node copy[MAX_NODES];
        int count = node_count;
        memcpy(copy, nodes, sizeof(Node) * count);
        portEXIT_CRITICAL(&nodes_lock);
        if (count == 0) {
            d.println("no other nodes seen");
        }
        for (int i = 0; i < count && i < 6; i++) {
            uint32_t age = (now - copy[i].last_ms) / 1000;
            bool online = now - copy[i].last_ms < STALE_MS;
            d.setTextColor(online ? TFT_GREEN : TFT_RED, TFT_BLACK);
            d.printf("%-10.10s %3u %-5s", copy[i].name, copy[i].rocnet, copy[i].power);
            d.setTextColor(TFT_WHITE, TFT_BLACK);
            d.printf(online ? (strcmp(copy[i].name, via) == 0 ? " via\n" : "\n") : " %lus\n", (unsigned long)age);
        }
    }
    d.endWrite();
}
#else
static void show(const char *status) {
    if (status) {
        Serial.println(status);
    }
}
#endif

static void halt(const char *why) {
    Serial.println(why);
    show(why);
    while (true) {
        delay(1000);
    }
}

void setup() {
    Serial.begin(115200);
#ifdef HIVE_SCREEN
    M5.begin();
    M5.Display.setRotation(3);
    M5.Display.setTextSize(1);
    M5.Display.setBrightness(96);
    // Holding a button 2 s counts as "hold" (global stop, faked short).
    M5.BtnA.setHoldThresh(2000);
    M5.BtnB.setHoldThresh(2000);
#endif
    // CPU load baseline: one second with nothing running yet.
    esp_register_freertos_idle_hook_for_cpu(idle_hook_0, 0);
    esp_register_freertos_idle_hook_for_cpu(idle_hook_1, 1);
    uint32_t before[2] = {idle_runs[0], idle_runs[1]};
    delay(1000);
    for (int core = 0; core < 2; core++) {
        idle_max[core] = max<uint32_t>(1, idle_runs[core] - before[core]);
    }
    Serial.printf("\nHive-Rail Node-Lite %s (RocNet %u), session %s, free heap %u\n", HIVE_NODE_ID, ROCNET_ID,
                  HIVE_SESSION, ESP.getFreeHeap());

    show("Wi-Fi ...");
    WiFi.mode(WIFI_STA);
    WiFi.begin(WIFI_SSID, WIFI_PASS);
    while (WiFi.status() != WL_CONNECTED) {
        delay(500);
    }
    Serial.printf("Wi-Fi %s\n", WiFi.localIP().toString().c_str());

#ifdef HIVE_OTA
    // Firmware updates over Wi-Fi: pio run -e m5stick-c-ota -t upload
    ArduinoOTA.setHostname("m5stickc");
    ArduinoOTA.setPassword(HIVE_OTA_PASS);
    ArduinoOTA.onStart([]() { show("Firmware update ..."); });
    ArduinoOTA.onProgress([](unsigned int done, unsigned int total) {
        static unsigned int last = 101;
        unsigned int percent = done * 100 / total;
        if (percent / 10 != last / 10) {
            last = percent;
            char text[32];
            snprintf(text, sizeof(text), "Firmware update %u%%", percent);
            show(text);
        }
    });
    ArduinoOTA.onEnd([]() { show("Update done, restarting"); });
    ArduinoOTA.onError([](ota_error_t error) {
        char text[32];
        snprintf(text, sizeof(text), "Update failed (%u)", (unsigned)error);
        show(text);
    });
    ArduinoOTA.begin();
#endif

    show("Zenoh ...");
    z_owned_config_t config;
    z_config_default(&config);
    zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_MODE_KEY, "client");
    zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_CONNECT_KEY, HIVE_LOCATOR);
    if (z_open(&session, z_config_move(&config), NULL) < 0) {
        halt("Zenoh: cannot open the session");
    }
    session_open = true;
    z_id_t id = z_info_zid(z_session_loan(&session));
    zid_text(&id, own_zid);
    Serial.printf("Zenoh client to %s, zid %s, free heap %u\n", HIVE_LOCATOR, own_zid, ESP.getFreeHeap());

    char key[96];
    snprintf(key, sizeof(key), "hive/%s/node/%s/hb", HIVE_SESSION, HIVE_NODE_ID);
    z_view_keyexpr_t ke;
    z_view_keyexpr_from_str_unchecked(&ke, key);
    if (z_declare_publisher(z_session_loan(&session), &heartbeat, z_view_keyexpr_loan(&ke), NULL) < 0) {
        halt("Zenoh: no heartbeat publisher");
    }

    static char peers_key[96];
    snprintf(peers_key, sizeof(peers_key), "hive/%s/node/*/hb", HIVE_SESSION);
    z_owned_closure_sample_t callback;
    z_closure_sample(&callback, on_heartbeat, NULL, NULL);
    z_view_keyexpr_t peers_ke;
    z_view_keyexpr_from_str_unchecked(&peers_ke, peers_key);
    if (z_declare_subscriber(z_session_loan(&session), &peers, z_view_keyexpr_loan(&peers_ke),
                             z_closure_sample_move(&callback), NULL) < 0) {
        halt("Zenoh: no heartbeat subscriber");
    }

    // District power: the global stop both ways, this district's status, events.
    static char key_all[64], key_status[96], key_events[96];
    snprintf(key_all, sizeof(key_all), "hive/%s/rn/cs/all", HIVE_SESSION);
    snprintf(key_status, sizeof(key_status), "hive/%s/rn/cs/node/%s", HIVE_SESSION, HIVE_NODE_ID);
    snprintf(key_events, sizeof(key_events), "hive/%s/node/%s/event", HIVE_SESSION, HIVE_NODE_ID);
    z_view_keyexpr_t ke_all, ke_status, ke_events;
    z_view_keyexpr_from_str_unchecked(&ke_all, key_all);
    z_view_keyexpr_from_str_unchecked(&ke_status, key_status);
    z_view_keyexpr_from_str_unchecked(&ke_events, key_events);
    z_owned_closure_sample_t stop_cb;
    z_closure_sample(&stop_cb, on_global_stop, NULL, NULL);
    if (z_declare_publisher(z_session_loan(&session), &global_stop_out, z_view_keyexpr_loan(&ke_all), NULL) < 0 ||
        z_declare_publisher(z_session_loan(&session), &power_status, z_view_keyexpr_loan(&ke_status), NULL) < 0 ||
        z_declare_publisher(z_session_loan(&session), &events, z_view_keyexpr_loan(&ke_events), NULL) < 0 ||
        z_declare_subscriber(z_session_loan(&session), &global_stop, z_view_keyexpr_loan(&ke_all),
                             z_closure_sample_move(&stop_cb), NULL) < 0) {
        halt("Zenoh: no power keys");
    }
#ifdef HIVE_SCREEN
    pinMode(RED_LED, OUTPUT);
    digitalWrite(RED_LED, HIGH);
#endif
    publish_power();
    event("lite node started, district power OFF");
}

void loop() {
    static uint32_t next_beat = 0;
    static uint32_t next_screen = 0;
    static uint32_t next_via = 0;
    static uint32_t next_cpu = 0;
    uint32_t now = millis();
    if (now >= next_cpu) {
        next_cpu = now + 1000;
        measure_cpu();
    }

#ifdef HIVE_OTA
    ArduinoOTA.handle();
#endif
    if (stop_from_mesh) {
        stop_from_mesh = false;
        char why[32];
        snprintf(why, sizeof(why), "GLOBAL STOP from %s", stop_origin[0] ? stop_origin : "?");
        set_power(POWER_STOP, why);
        next_screen = 0;
    }
#ifdef HIVE_SCREEN
    M5.update();
    // Front button: a press is the next page, holding it 2 s the global emergency stop.
    if (M5.BtnA.wasReleased() && !M5.BtnA.wasReleasedAfterHold()) {
        page = (page + 1) % 2;
        next_screen = 0;
    }
    if (M5.BtnA.wasHold()) {
        send_global_stop();
        next_screen = 0;
    }
    // Side button: district power on and off (the board's go and stop), holding it a short.
    if (M5.BtnB.wasReleased() && !M5.BtnB.wasReleasedAfterHold()) {
        set_power(power == POWER_ON ? POWER_OFF : POWER_ON, "button");
        next_screen = 0;
    }
    if (M5.BtnB.wasHold()) {
        set_power(POWER_SHORT, "faked short");
        next_screen = 0;
    }
    // Red LED: on with power, blinking for STOP and SHORT.
    bool lit = power == POWER_ON || ((power == POWER_STOP || power == POWER_SHORT) && (now / 250) % 2);
    digitalWrite(RED_LED, lit ? LOW : HIGH);
#endif
    if (now >= next_via) {
        next_via = now + 2000;
        find_via();
    }
    if (now >= next_beat) {
        next_beat = now + HEARTBEAT_MS;
        seq++;
        // The Rust nodes' Heartbeat, as a lite node with the node that carries it.
        char json[256];
        snprintf(json, sizeof(json),
                 "{\"node\":\"%s\",\"seq\":%lu,\"power\":\"%s\",\"rocnet_id\":%u,\"locos\":[],\"register\":null,"
                 "\"zid\":\"%s\",\"lite\":true%s%s%s}",
                 HIVE_NODE_ID, (unsigned long)seq, POWER_LABEL[power], ROCNET_ID, own_zid, via[0] ? ",\"via\":\"" : "", via,
                 via[0] ? "\"" : "");
        z_owned_bytes_t payload;
        z_bytes_copy_from_str(&payload, json);
        if (z_publisher_put(z_publisher_loan(&heartbeat), z_bytes_move(&payload), NULL) < 0) {
            Serial.println("heartbeat not sent");
        }
        if (seq % 40 == 0) {
            Serial.printf("-> heartbeat %lu via %s; %lu received; free heap %u, lowest %u; cpu %u%% / %u%%\n",
                          (unsigned long)seq, via[0] ? via : "?", (unsigned long)received, ESP.getFreeHeap(),
                          ESP.getMinFreeHeap(), cpu_load[0], cpu_load[1]);
        }
    }
    if (now >= next_screen) {
        next_screen = now + 500;
        show(nullptr);
    }
    delay(10);
}
