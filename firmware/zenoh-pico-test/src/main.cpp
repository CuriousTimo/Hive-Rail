// Phase B0 test: a zenoh-pico client on an ESP32 sends a Hive-Rail heartbeat
// (hive/<session>/node/<id>/hb, the JSON the Rust nodes send), so the dispatcher
// shows it as a district, and listens to the other nodes' heartbeats. Logs the
// free heap, to see what a lite node has left.

#include <Arduino.h>
#include <WiFi.h>
#include <zenoh-pico.h>

#include "secrets.h"

static z_owned_session_t session;
static z_owned_publisher_t heartbeat;
static z_owned_subscriber_t peers;
static uint32_t seq = 0;
static uint32_t received = 0;

// A heartbeat from another node: print who it is from.
static void on_heartbeat(z_loaned_sample_t *sample, void *) {
    z_view_string_t key;
    z_keyexpr_as_view_string(z_sample_keyexpr(sample), &key);
    received++;
    Serial.print("<- ");
    Serial.write(z_string_data(z_view_string_loan(&key)), z_string_len(z_view_string_loan(&key)));
    Serial.println();
}

static void halt(const char *why) {
    Serial.println(why);
    while (true) {
        delay(1000);
    }
}

void setup() {
    Serial.begin(115200);
    delay(500);
    Serial.printf("\nHive-Rail zenoh-pico test, node %s, session %s, free heap %u\n", HIVE_NODE_ID,
                  HIVE_SESSION, ESP.getFreeHeap());

    Serial.print("Wi-Fi ");
    WiFi.mode(WIFI_STA);
    WiFi.begin(WIFI_SSID, WIFI_PASS);
    while (WiFi.status() != WL_CONNECTED) {
        delay(500);
        Serial.print(".");
    }
    Serial.printf(" %s\n", WiFi.localIP().toString().c_str());

    z_owned_config_t config;
    z_config_default(&config);
    zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_MODE_KEY, "client");
    zp_config_insert(z_config_loan_mut(&config), Z_CONFIG_CONNECT_KEY, HIVE_LOCATOR);
    Serial.printf("Zenoh client to %s ... ", HIVE_LOCATOR);
    if (z_open(&session, z_config_move(&config), NULL) < 0) {
        halt("cannot open the session");
    }
    Serial.printf("open, free heap %u\n", ESP.getFreeHeap());

    char key[96];
    snprintf(key, sizeof(key), "hive/%s/node/%s/hb", HIVE_SESSION, HIVE_NODE_ID);
    z_view_keyexpr_t ke;
    z_view_keyexpr_from_str_unchecked(&ke, key);
    if (z_declare_publisher(z_session_loan(&session), &heartbeat, z_view_keyexpr_loan(&ke), NULL) < 0) {
        halt("cannot declare the heartbeat publisher");
    }

    static char peers_key[96];
    snprintf(peers_key, sizeof(peers_key), "hive/%s/node/*/hb", HIVE_SESSION);
    z_owned_closure_sample_t callback;
    z_closure_sample(&callback, on_heartbeat, NULL, NULL);
    z_view_keyexpr_t peers_ke;
    z_view_keyexpr_from_str_unchecked(&peers_ke, peers_key);
    if (z_declare_subscriber(z_session_loan(&session), &peers, z_view_keyexpr_loan(&peers_ke),
                             z_closure_sample_move(&callback), NULL) < 0) {
        halt("cannot subscribe to the heartbeats");
    }
    Serial.printf("heartbeat on %s, listening on %s, free heap %u\n", key, peers_key, ESP.getFreeHeap());
}

// As the Rust nodes: a heartbeat every 250 ms; they call a peer stale after 750 ms.
static const uint32_t HEARTBEAT_MS = 250;

void loop() {
    delay(HEARTBEAT_MS);
    seq++;
    // The Rust nodes' Heartbeat: node, seq, power, rocnet_id, locos, register.
    char json[160];
    snprintf(json, sizeof(json),
             "{\"node\":\"%s\",\"seq\":%u,\"power\":\"OFF\",\"rocnet_id\":199,\"locos\":[],\"register\":null}",
             HIVE_NODE_ID, (unsigned)seq);
    z_owned_bytes_t payload;
    z_bytes_copy_from_str(&payload, json);
    if (z_publisher_put(z_publisher_loan(&heartbeat), z_bytes_move(&payload), NULL) < 0) {
        Serial.println("heartbeat not sent");
    }
    if (seq % 40 == 0) {
        Serial.printf("-> heartbeat %u; %u heartbeats received; free heap %u, lowest %u\n", (unsigned)seq,
                      (unsigned)received, ESP.getFreeHeap(), ESP.getMinFreeHeap());
    }
}
