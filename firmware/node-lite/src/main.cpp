// Hive-Rail Node-Lite: our lite node test and debug device (phase B0), a zenoh-pico client on
// an ESP32. It sends the Rust nodes' heartbeat (hive/<session>/node/<id>/hb) every 250 ms, marked
// "lite" with the node it reaches the mesh through ("via"), and follows the other nodes'
// heartbeats. It also plays its district's command station with fake hardware: district power
// on its buttons, a faked short, and the mesh-wide global emergency stop both ways.
// On an M5StickC: side button B = power on/off (double press: short circuit), front button A =
// next page (double press: GLOBAL emergency stop, triple press: GLOBAL go); the red LED shows
// the district power.

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
static char last_event[80] = "";
// A global stop or go received from the mesh, applied by loop() (set in the receive task).
static volatile bool stop_from_mesh = false;
static volatile bool go_from_mesh = false;
static char stop_origin[24] = "";
// A firmware update arrived: loop() restarts the ESP32 (not the update code itself, which once
// left the M5StickC hanging on "restarting").
static volatile bool update_done = false;
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

// The district's part of the layout, from a full node started with --layout.
struct LayoutBlock {
    char id[40];
    char role[10];
    int address;
};
static const int MAX_LAYOUT_BLOCKS = 16;
static struct {
    bool known;
    char checksum[20];
    char modules[64];
    char borders[48];
    int sensors, signals, turnouts, crossings, block_count;
    LayoutBlock blocks[MAX_LAYOUT_BLOCKS];
} district_layout = {};
static portMUX_TYPE layout_lock = portMUX_INITIALIZER_UNLOCKED;
static volatile bool layout_arrived = false;

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
static const uint8_t RN_CS_QUERY = 1, RN_CS_TRACK_POWER = 2, RN_CS_HALT = 3;

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
    char json[200];
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
    char text[80];
    snprintf(text, sizeof(text), "district power %s (%s)", POWER_LABEL[next], why);
    event(text);
}

// An emergency stop stops a district that is ON; OFF stays OFF and SHORT stays SHORT, as in the
// Rust nodes (power.rs).
static void emergency_stop(const char *why) {
    if (power == POWER_ON) {
        set_power(POWER_STOP, why);
    }
}

// The layout-wide emergency stop: every district that is ON goes to STOP.
static void send_global_stop() {
    uint8_t frame[] = {0, 0, 0, (uint8_t)(ROCNET_ID >> 8), (uint8_t)ROCNET_ID, RN_CS_GROUP,
                       (uint8_t)(RN_REQUEST << 5 | RN_CS_HALT), 0};
    put_with_origin(z_publisher_loan(&global_stop_out), frame, sizeof(frame));
    event("GLOBAL EMERGENCY STOP sent to every district");
    emergency_stop("own global stop");
}

// The global go ends a global stop: a district in emergency stop goes back on; OFF stays OFF
// and SHORT stays SHORT (power.rs, resumes). On the layout-wide key it is a power-on request.
static void global_go(const char *why) {
    if (power == POWER_STOP) {
        set_power(POWER_ON, why);
    }
}

static void send_global_go() {
    uint8_t frame[] = {0, 0, 0, (uint8_t)(ROCNET_ID >> 8), (uint8_t)ROCNET_ID, RN_CS_GROUP,
                       (uint8_t)(RN_REQUEST << 5 | RN_CS_TRACK_POWER), 1, 1};
    put_with_origin(z_publisher_loan(&global_stop_out), frame, sizeof(frame));
    event("GLOBAL GO sent: every district in emergency stop back on");
    global_go("own global go");
}

static void on_global_stop(z_loaned_sample_t *sample, void *) {
    uint8_t frame[16];
    z_bytes_reader_t reader = z_bytes_get_reader(z_sample_payload(sample));
    size_t len = z_bytes_reader_read(&reader, frame, sizeof(frame));
    // A halt request (group 1, type request, action 3) is the global stop; a track power on
    // request (action 2, data 1) the global go.
    if (len < 8 || frame[5] != RN_CS_GROUP || (frame[6] >> 5) != RN_REQUEST) {
        return;
    }
    uint8_t action = frame[6] & 0x1F;
    bool halt = action == RN_CS_HALT;
    bool go = action == RN_CS_TRACK_POWER && len >= 9 && frame[8] == 1;
    if (!halt && !go) {
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
    if (halt) {
        stop_from_mesh = true;
    } else {
        go_from_mesh = true;
    }
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
    } else if (page == 2) {
        // The district's part of the layout, from the mesh.
        portENTER_CRITICAL(&layout_lock);
        auto copy = district_layout;
        portEXIT_CRITICAL(&layout_lock);
        if (!copy.known) {
            d.setTextColor(TFT_ORANGE, TFT_BLACK);
            d.println("Layout: asking the mesh ...");
            d.setTextColor(TFT_WHITE, TFT_BLACK);
            d.println("(a full node with --layout");
            d.println(" answers)");
        } else {
            d.setTextColor(TFT_GREEN, TFT_BLACK);
            d.printf("Layout %s\n", copy.checksum);
            d.setTextColor(TFT_WHITE, TFT_BLACK);
            d.printf("%.26s\n", copy.modules);
            d.printf("%d blocks %d sensors %d sig\n", copy.block_count, copy.sensors, copy.signals);
            d.printf("borders: %.18s\n", copy.borders);
            for (int i = 0; i < copy.block_count && i < 2; i++) {
                const char *name = strchr(copy.blocks[i].id, '/');
                d.printf("%-10.10s %-8.8s %d\n", name ? name + 1 : copy.blocks[i].id, copy.blocks[i].role,
                         copy.blocks[i].address);
            }
        }
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

// Lite servers (full nodes started with --lite-server) run Zenoh as routers: the lite node finds
// them by scouting for routers. Only if none answers, the addresses in HIVE_LOCATORS
// ("tcp/a:7447,tcp/b:7448") or HIVE_LOCATOR are tried in turn.
#ifndef HIVE_LOCATORS
#define HIVE_LOCATORS HIVE_LOCATOR
#endif
static const char *SCOUTED = "scouting";
static bool zenoh_up = false;
static char connected_locator[48] = "";
// zenoh-pico does not always report a session as closed when its node is gone; failing
// heartbeats do: after this many in a row (2 s) the lite node moves to the next full node.
static const int MAX_FAILED_HEARTBEATS = 8;
static int failed_heartbeats = 0;


static void on_layout(z_loaned_reply_t *reply, void *) {
    if (!z_reply_is_ok(reply)) {
        return;
    }
    const z_loaned_sample_t *sample = z_reply_ok(reply);
    z_owned_string_t text;
    z_bytes_to_string(z_sample_payload(sample), &text);
    JsonDocument doc;
    DeserializationError error =
        deserializeJson(doc, z_string_data(z_string_loan(&text)), z_string_len(z_string_loan(&text)));
    z_string_drop(z_string_move(&text));
    if (error) {
        return;
    }
    // Parsed outside the lock, copied in under it.
    static LayoutBlock blocks[MAX_LAYOUT_BLOCKS];
    int count = 0;
    for (JsonObject b : doc["blocks"].as<JsonArray>()) {
        if (count == MAX_LAYOUT_BLOCKS) {
            break;
        }
        strlcpy(blocks[count].id, b["id"] | "", sizeof(blocks[count].id));
        strlcpy(blocks[count].role, b["role"] | "", sizeof(blocks[count].role));
        blocks[count].address = b["address"] | -1;
        count++;
    }
    char modules[64] = "", borders[48] = "";
    for (JsonVariant m : doc["modules"].as<JsonArray>()) {
        if (modules[0]) {
            strlcat(modules, " ", sizeof(modules));
        }
        strlcat(modules, m.as<const char *>(), sizeof(modules));
    }
    for (JsonObject b : doc["borders"].as<JsonArray>()) {
        if (borders[0]) {
            strlcat(borders, " ", sizeof(borders));
        }
        strlcat(borders, b["to"] | "?", sizeof(borders));
    }
    portENTER_CRITICAL(&layout_lock);
    district_layout.known = true;
    strlcpy(district_layout.checksum, doc["layout_checksum"] | "", sizeof(district_layout.checksum));
    strlcpy(district_layout.modules, modules, sizeof(district_layout.modules));
    strlcpy(district_layout.borders, borders, sizeof(district_layout.borders));
    district_layout.sensors = doc["sensors"].size();
    district_layout.signals = doc["signals"].size();
    district_layout.turnouts = doc["turnouts"].size();
    district_layout.crossings = doc["crossings"].size();
    district_layout.block_count = count;
    memcpy(district_layout.blocks, blocks, sizeof(LayoutBlock) * count);
    portEXIT_CRITICAL(&layout_lock);
    layout_arrived = true;
}

// Asks the mesh for this district's part of the layout; a full node with --layout answers.
static void fetch_layout() {
    char key[96];
    snprintf(key, sizeof(key), "hive/%s/layout/district/%s", HIVE_SESSION, HIVE_NODE_ID);
    z_view_keyexpr_t ke;
    z_view_keyexpr_from_str_unchecked(&ke, key);
    z_owned_closure_reply_t callback;
    z_closure_reply(&callback, on_layout, NULL, NULL);
    z_get_options_t options;
    z_get_options_default(&options);
    options.timeout_ms = 3000;
    z_get(z_session_loan(&session), z_view_keyexpr_loan(&ke), "", z_closure_reply_move(&callback), &options);
}

static bool declare(int result) {
    return result >= 0;
}

// Opens the session through a lite server found by scouting, else through the first address in
// HIVE_LOCATORS that answers, and declares everything.
static bool open_zenoh() {
    char locators[160];
    strlcpy(locators, HIVE_LOCATORS, sizeof(locators));
    const char *candidates[8] = {SCOUTED};
    int count = 1;
    for (char *locator = strtok(locators, ","); locator && count < 8; locator = strtok(NULL, ",")) {
        candidates[count++] = locator;
    }
    for (int c = 0; c < count; c++) {
        const char *locator = candidates[c];
        char text[64];
        snprintf(text, sizeof(text), locator == SCOUTED ? "Zenoh: looking for lite servers" : "Zenoh %s ...",
                 locator);
        show(text);
        z_owned_config_t config;
        z_config_default(&config);
        zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_MODE_KEY, "client");
        if (locator == SCOUTED) {
            // Routers only (1): the lite servers. Ordinary nodes are peers and do not answer.
            zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_SCOUTING_WHAT_KEY, "1");
            zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_SCOUTING_TIMEOUT_KEY, "3000");
        } else {
            zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_CONNECT_KEY, locator);
        }
        if (z_open(&session, z_config_move(&config), NULL) < 0) {
            Serial.printf("Zenoh: no answer from %s\n", locator);
            continue;
        }
        strlcpy(connected_locator, locator, sizeof(connected_locator));
        session_open = true;
        z_id_t id = z_info_zid(z_session_loan(&session));
        zid_text(&id, own_zid);
        Serial.printf("Zenoh client to %s, zid %s, free heap %u\n", locator, own_zid, ESP.getFreeHeap());

        static char key_hb[96], key_peers[96], key_all[64], key_status[96], key_events[96];
        snprintf(key_hb, sizeof(key_hb), "hive/%s/node/%s/hb", HIVE_SESSION, HIVE_NODE_ID);
        snprintf(key_peers, sizeof(key_peers), "hive/%s/node/*/hb", HIVE_SESSION);
        snprintf(key_all, sizeof(key_all), "hive/%s/rn/cs/all", HIVE_SESSION);
        snprintf(key_status, sizeof(key_status), "hive/%s/rn/cs/node/%s", HIVE_SESSION, HIVE_NODE_ID);
        snprintf(key_events, sizeof(key_events), "hive/%s/node/%s/event", HIVE_SESSION, HIVE_NODE_ID);
        z_view_keyexpr_t ke_hb, ke_peers, ke_all, ke_status, ke_events;
        z_view_keyexpr_from_str_unchecked(&ke_hb, key_hb);
        z_view_keyexpr_from_str_unchecked(&ke_peers, key_peers);
        z_view_keyexpr_from_str_unchecked(&ke_all, key_all);
        z_view_keyexpr_from_str_unchecked(&ke_status, key_status);
        z_view_keyexpr_from_str_unchecked(&ke_events, key_events);
        z_owned_closure_sample_t peers_cb, stop_cb;
        z_closure_sample(&peers_cb, on_heartbeat, NULL, NULL);
        z_closure_sample(&stop_cb, on_global_stop, NULL, NULL);
        const z_loaned_session_t *zs = z_session_loan(&session);
        if (declare(z_declare_publisher(zs, &heartbeat, z_view_keyexpr_loan(&ke_hb), NULL)) &&
            declare(z_declare_subscriber(zs, &peers, z_view_keyexpr_loan(&ke_peers), z_closure_sample_move(&peers_cb),
                                         NULL)) &&
            declare(z_declare_publisher(zs, &global_stop_out, z_view_keyexpr_loan(&ke_all), NULL)) &&
            declare(z_declare_publisher(zs, &power_status, z_view_keyexpr_loan(&ke_status), NULL)) &&
            declare(z_declare_publisher(zs, &events, z_view_keyexpr_loan(&ke_events), NULL)) &&
            declare(z_declare_subscriber(zs, &global_stop, z_view_keyexpr_loan(&ke_all), z_closure_sample_move(&stop_cb),
                                         NULL))) {
            return true;
        }
        Serial.println("Zenoh: declarations failed");
        z_session_drop(z_session_move(&session));
        session_open = false;
    }
    return false;
}

void setup() {
    Serial.begin(115200);
#ifdef HIVE_SCREEN
    M5.begin();
    M5.Display.setRotation(3);
    M5.Display.setTextSize(1);
    M5.Display.setBrightness(96);
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
    ArduinoOTA.setRebootOnSuccess(false);
    ArduinoOTA.onStart([]() { show("Firmware update ..."); });
    ArduinoOTA.onProgress([](unsigned int done, unsigned int total) {
        // The upload runs inside loop(): keep its watchdog fed.
        feedLoopWDT();
        static unsigned int last = 101;
        unsigned int percent = done * 100 / total;
        if (percent / 10 != last / 10) {
            last = percent;
            char text[32];
            snprintf(text, sizeof(text), "Firmware update %u%%", percent);
            show(text);
        }
    });
    ArduinoOTA.onEnd([]() {
        show("Update done, restarting");
        update_done = true;
    });
    ArduinoOTA.onError([](ota_error_t error) {
        char text[32];
        snprintf(text, sizeof(text), "Update failed (%u)", (unsigned)error);
        show(text);
    });
    ArduinoOTA.begin();
#endif

    zenoh_up = open_zenoh();
#ifdef HIVE_SCREEN
    pinMode(RED_LED, OUTPUT);
    digitalWrite(RED_LED, HIGH);
#endif
    if (zenoh_up) {
        publish_power();
        char text[80];
        snprintf(text, sizeof(text), "lite node started (found its server by %s), power OFF",
                 strcmp(connected_locator, SCOUTED) == 0 ? "scouting" : connected_locator);
        event(text);
        fetch_layout();
    }
    // If loop() ever hangs for more than a few seconds, the ESP32 restarts.
    enableLoopWDT();
}

void loop() {
    static uint32_t next_beat = 0;
    static uint32_t next_screen = 0;
    static uint32_t next_via = 0;
    static uint32_t next_cpu = 0;
    static uint32_t next_zenoh_check = 0;
    static uint32_t next_layout = 0;
    uint32_t now = millis();
    // The node carrying this one gone: reconnect, through whichever full node answers.
    if (now >= next_zenoh_check) {
        next_zenoh_check = now + 2000;
        if (zenoh_up &&
            (z_session_is_closed(z_session_loan(&session)) || failed_heartbeats >= MAX_FAILED_HEARTBEATS)) {
            // The lite server is gone. Closing the dead session can hang in zenoh-pico, so the
            // lite node restarts and scouts for another lite server on the way up (a few
            // seconds). The district's power state starts OFF again, as after any restart.
            Serial.printf("Zenoh: lite server at %s gone, restarting to find another\n", connected_locator);
            show("Lite server gone: restarting");
            delay(300);
            ESP.restart();
        }
        if (!zenoh_up) {
            // Opening may wait on a node that is gone: longer than the loop watchdog allows.
            disableLoopWDT();
            zenoh_up = open_zenoh();
            enableLoopWDT();
            next_zenoh_check = now + 5000;
            if (zenoh_up) {
                publish_power();
                char text[80];
                snprintf(text, sizeof(text), "reconnected through %s", connected_locator);
                event(text);
                next_layout = 0;
            }
        }
    }
    if (zenoh_up && !district_layout.known && now >= next_layout) {
        next_layout = now + 10000;
        fetch_layout();
    }
    if (layout_arrived) {
        layout_arrived = false;
        char text[80];
        snprintf(text, sizeof(text), "layout %s: %d blocks, borders to %s", district_layout.checksum,
                 district_layout.block_count, district_layout.borders);
        event(text);
    }
    if (!zenoh_up) {
#ifdef HIVE_SCREEN
        if (now >= next_screen) {
            next_screen = now + 500;
            show(nullptr);
        }
#endif
        delay(10);
        return;
    }
    if (now >= next_cpu) {
        next_cpu = now + 1000;
        measure_cpu();
    }

#ifdef HIVE_OTA
    ArduinoOTA.handle();
    if (update_done) {
        delay(500);
        ESP.restart();
    }
#endif
    if (stop_from_mesh) {
        stop_from_mesh = false;
        char why[48];
        snprintf(why, sizeof(why), "global stop from %s", stop_origin[0] ? stop_origin : "?");
        char text[80];
        snprintf(text, sizeof(text), "District emergency stop requested by %s", stop_origin[0] ? stop_origin : "?");
        event(text);
        emergency_stop(why);
        next_screen = 0;
    }
    if (go_from_mesh) {
        go_from_mesh = false;
        char why[48];
        snprintf(why, sizeof(why), "global go from %s", stop_origin[0] ? stop_origin : "?");
        char text[80];
        snprintf(text, sizeof(text), "Global go from %s", stop_origin[0] ? stop_origin : "?");
        event(text);
        global_go(why);
        next_screen = 0;
    }
#ifdef HIVE_SCREEN
    M5.update();
    // Front button: a single press is the next page, a double press the global emergency stop.
    // A single press counts once the time for a second press has passed.
    if (M5.BtnA.wasSingleClicked()) {
        page = (page + 1) % 3;
        next_screen = 0;
    }
    if (M5.BtnA.wasDoubleClicked()) {
        send_global_stop();
        next_screen = 0;
    }
    // Three presses: the global go, ending a global stop.
    if (M5.BtnA.wasDecideClickCount() && M5.BtnA.getClickCount() >= 3) {
        send_global_go();
        next_screen = 0;
    }
    // Side button: district power on and off (the board's go and stop); a double press a short.
    if (M5.BtnB.wasSingleClicked()) {
        set_power(power == POWER_ON ? POWER_OFF : POWER_ON, "button");
        next_screen = 0;
    }
    if (M5.BtnB.wasDoubleClicked()) {
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
            failed_heartbeats++;
            Serial.printf("heartbeat not sent (%d)\n", failed_heartbeats);
        } else {
            failed_heartbeats = 0;
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
