/*
 * robot-link -- ESP32-P4 firmware: the FPGA bench bridge's host, over Wi-Fi.
 *
 *     browser <--Wi-Fi (C6)--> ESP32-P4 <--UART--> FPGA supervisor <--> HX-30HM servos
 *
 * The FPGA (sipeed-tang-primer-25k, bridge_safety profile) keeps every safety
 * promise: temperature, voltage and current trips, the S2 button, the
 * 200 ms telemetry and 300 ms command leases, stopping. This firmware is the
 * host it expects on its UART, with the same bytes the simulator's Rust
 * hardware layer sends over USB (core/robot_link.c), plus a page to send
 * them from. Two rules:
 *
 *   1. One owner of the UART at a time: the page that claimed it, or the
 *      raw TCP tunnel. Anyone may STOP.
 *   2. Nothing is driven unless it is being held: the page renews a hold
 *      every 100 ms and its session every 500 ms; the engine zeroes a
 *      drive 250 ms after the last renewal and stops everything 1 s after
 *      the last sign of the page. If this firmware itself stops running,
 *      the FPGA's own leases end the motion.
 *
 * Wi-Fi power save is off: modem sleep turns a 2 ms hop into 100 ms spikes.
 */
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/stream_buffer.h"
#include "esp_event.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_timer.h"
#include "esp_wifi.h"
#include "nvs_flash.h"
#include "driver/uart.h"
#include "link_shared.h"
#include "gait_store.h"

static const char *TAG = "robot-link";
#define LINK_UART UART_NUM_1

rl_t g_rl;
SemaphoreHandle_t g_rl_lock;
const char *volatile g_rl_lock_holder = "";
int g_owner_kind = OWNER_NONE, g_owner_fd = -1;
static int tunnel_sock = -1;
static StreamBufferHandle_t scan_rx; /* FPGA bytes for the bus scan while it owns the UART */

uint32_t rl_now_ms(void)
{
    return (uint32_t)(esp_timer_get_time() / 1000);
}

static void uart_write_cb(void *ctx, const uint8_t *bytes, size_t n)
{
    (void)ctx;
    uart_write_bytes(LINK_UART, bytes, n);
}

void link_uart_write(const uint8_t *bytes, size_t n)
{
    uart_write_bytes(LINK_UART, bytes, n);
}

void link_stop_now(void)
{
    rl_stop(&g_rl);
    if (g_rl.paused) {
        /* Three copies, 2 ms apart: a copy that lands between two TCP chunks of
         * one tunnel packet is swallowed into that packet and dropped with it
         * (bad checksum), so a later copy has to be the one that counts. */
        const uint8_t op = HX_OP_STOP;
        uint8_t pkt[8];
        size_t n = hx_packet(pkt, HX_BRIDGE_ID, HX_LOCAL, &op, 1);
        for (int i = 0; i < 3; i++) {
            if (i) vTaskDelay(pdMS_TO_TICKS(2));
            link_uart_write(pkt, n);
        }
    }
}

/* ---- Wi-Fi (on the C6, through esp_wifi_remote) ------------------------- */
static void on_wifi(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg; (void)data;
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        ESP_LOGW(TAG, "station link lost; reconnecting");
        esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = data;
        ESP_LOGI(TAG, "station address " IPSTR, IP2STR(&e->ip_info.ip));
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STACONNECTED) {
        ESP_LOGI(TAG, "a device joined the access point");
    }
}

static void wifi_start(void)
{
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    ESP_ERROR_CHECK(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, on_wifi, NULL));
    ESP_ERROR_CHECK(esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, on_wifi, NULL));
    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&init));
    wifi_config_t wc = {0};
#if CONFIG_RL_WIFI_STA
    esp_netif_create_default_wifi_sta();
    strncpy((char *)wc.sta.ssid, CONFIG_RL_WIFI_SSID, sizeof wc.sta.ssid - 1);
    strncpy((char *)wc.sta.password, CONFIG_RL_WIFI_PASSWORD, sizeof wc.sta.password - 1);
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_STA, &wc));
#else
    esp_netif_create_default_wifi_ap();
    strncpy((char *)wc.ap.ssid, CONFIG_RL_WIFI_SSID, sizeof wc.ap.ssid - 1);
    wc.ap.ssid_len = strlen(CONFIG_RL_WIFI_SSID);
    strncpy((char *)wc.ap.password, CONFIG_RL_WIFI_PASSWORD, sizeof wc.ap.password - 1);
    wc.ap.max_connection = 4;
    wc.ap.authmode = strlen(CONFIG_RL_WIFI_PASSWORD) >= 8 ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
    wc.ap.channel = 6;
    ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_AP));
    ESP_ERROR_CHECK(esp_wifi_set_config(WIFI_IF_AP, &wc));
#endif
    ESP_ERROR_CHECK(esp_wifi_start());
    ESP_ERROR_CHECK(esp_wifi_set_ps(WIFI_PS_NONE));
#if CONFIG_RL_WIFI_STA
    ESP_ERROR_CHECK(esp_wifi_connect());
    ESP_LOGI(TAG, "joining \"%s\"", CONFIG_RL_WIFI_SSID);
#else
    ESP_LOGI(TAG, "access point \"%s\": http://192.168.4.1:%d/", CONFIG_RL_WIFI_SSID, CONFIG_RL_HTTP_PORT);
#endif
}

/* ---- UART to the FPGA ---------------------------------------------------- */
static void uart_start(void)
{
    uart_config_t uc = {
        .baud_rate = CONFIG_RL_UART_BAUD,
        .data_bits = UART_DATA_8_BITS,
        .parity = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };
    ESP_ERROR_CHECK(uart_driver_install(LINK_UART, 2048, 2048, 0, NULL, 0));
    ESP_ERROR_CHECK(uart_param_config(LINK_UART, &uc));
    ESP_ERROR_CHECK(uart_set_pin(LINK_UART, CONFIG_RL_UART_TX_GPIO, CONFIG_RL_UART_RX_GPIO, UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));
    ESP_LOGI(TAG, "FPGA link on UART%d at %d baud (tx GPIO%d, rx GPIO%d)", LINK_UART, CONFIG_RL_UART_BAUD, CONFIG_RL_UART_TX_GPIO, CONFIG_RL_UART_RX_GPIO);
}

/* The engine: bytes in, one tick, every millisecond. */
/* The longest time between two passes of the link loop since the last status line, ms. */
static uint32_t link_gap_ms;
/* The longest wait for the engine's lock, and who held it when the wait began. */
static uint32_t link_wait_ms;
static const char *link_wait_for = "";

static void link_task(void *arg)
{
    (void)arg;
    uint8_t chunk[128];
    uint32_t last = rl_now_ms();
    for (;;) {
        int n = uart_read_bytes(LINK_UART, chunk, sizeof chunk, pdMS_TO_TICKS(1));
        const char *holder = g_rl_lock_holder;
        uint32_t asked = rl_now_ms();
        RL_LOCK("link_task");
        uint32_t now = rl_now_ms();
        if (now - asked > link_wait_ms) { link_wait_ms = now - asked; link_wait_for = holder; }
        if (now - last > link_gap_ms) link_gap_ms = now - last;
        /* Everything received so far is parsed before any reply timeout is judged: bytes
         * that arrived while this task waited for the lock, or was preempted, are a reply
         * in time, not a late one (a late one is dropped as unexpected after the engine
         * has moved on). */
        while (n > 0) {
            if (g_owner_kind == OWNER_TUNNEL && tunnel_sock >= 0) send(tunnel_sock, chunk, n, 0);
            if (g_owner_kind == OWNER_SCAN && scan_rx) xStreamBufferSend(scan_rx, chunk, n, 0);
            rl_rx(&g_rl, chunk, (size_t)n, rl_now_ms());
            n = uart_read_bytes(LINK_UART, chunk, sizeof chunk, 0);
        }
        rl_tick(&g_rl, rl_now_ms());
        last = rl_now_ms();
        xSemaphoreGive(g_rl_lock);
    }
}

/* ---- a link summary on the USB console ----------------------------------- */
/*
 * What the page shows, for whoever has the USB cable rather than the access
 * point: rates since the last line (FPGA status replies come every 200 ms, so
 * replies above 5/s are servos answering), the latch, and each servo's last
 * telemetry or how long it has been silent.
 */
#if CONFIG_RL_STATUS_LOG_MS
static void status_log_task(void *arg)
{
    (void)arg;
    uint32_t last_sent = 0, last_replies = 0, last_timeouts = 0, last_at = rl_now_ms();
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(CONFIG_RL_STATUS_LOG_MS));
        char line[400];
        int n = 0;
        /* A snapshot under the lock; the line is formatted after (see web.c's broadcast). */
        static rl_t snap;
        RL_LOCK("status_log_task");
        memcpy(&snap, &g_rl, sizeof snap);
        uint32_t gap = link_gap_ms, wait = link_wait_ms;
        const char *wait_for = link_wait_for;
        link_gap_ms = link_wait_ms = 0;
        link_wait_for = "";
        xSemaphoreGive(g_rl_lock);
        uint32_t now = rl_now_ms();
        float dt = (now - last_at) / 1000.0f;
        n += snprintf(line + n, sizeof line - n, "per s: %.1f sent, %.1f replies, %.1f timeouts; totals refused %lu, bad checksums %lu, dropped bytes %lu | ",
            (snap.sent - last_sent) / dt, (snap.replies - last_replies) / dt, (snap.timeouts - last_timeouts) / dt,
            (unsigned long)snap.refused, (unsigned long)snap.framer.checksum_errors, (unsigned long)snap.framer.dropped_bytes);
        n += snprintf(line + n, sizeof line - n, "longest link gap %lu ms, lock wait %lu ms (%s), unexpected %lu | ", (unsigned long)gap,
                      (unsigned long)wait, wait_for, (unsigned long)snap.unexpected);
        last_sent = snap.sent; last_replies = snap.replies; last_timeouts = snap.timeouts; last_at = now;
        if (!rl_link_ok(&snap, now)) n += snprintf(line + n, sizeof line - n, "FPGA not answering | ");
        else if (snap.status.latched) n += snprintf(line + n, sizeof line - n, "FPGA latched: %s | ", hx_reason_name(snap.status.reason));
        else n += snprintf(line + n, sizeof line - n, "FPGA running, armed 0x%x fresh 0x%x | ", snap.status.armed_mask, snap.status.fresh_mask);
        for (int i = 0; i < RL_MAX_SERVOS && n < (int)sizeof line - 48; i++) {
            if (!(snap.cfg.present_mask >> i & 1)) continue;
            const rl_servo_t *s = &snap.servo[i];
            unsigned id = snap.cfg.first_id + i;
            if (!s->valid) n += snprintf(line + n, sizeof line - n, "%u: never answered  ", id);
            else if (now - s->at_ms > 2000) n += snprintf(line + n, sizeof line - n, "%u: silent %lus  ", id, (unsigned long)((now - s->at_ms) / 1000));
            else n += snprintf(line + n, sizeof line - n, "%u: %.1fV pos %u %uC  ", id, s->t.voltage_raw / 10.0, s->t.position_raw, s->t.temperature_c);
        }

        ESP_LOGI(TAG, "%s", line);
    }
}
#endif

/* ---- bus scan: which IDs answer a ping ----------------------------------- */
/*
 * "never answered" has two causes the status line cannot tell apart: a bus
 * that carries nothing, and servos with IDs other than the configured ones.
 * The FPGA forwards a ping (instruction 1) to any ID and a ping cannot move a
 * servo, so this pings 0-253 one at a time and logs who answers. It takes the
 * UART the way the tunnel does, and only while the FPGA is latched with
 * nothing armed, so the STOP it delays for ~3 s has nothing to stop.
 */
#if CONFIG_RL_BUS_SCAN_S
static bool scan_wanted(uint32_t now)
{
    if (g_owner_kind != OWNER_NONE || !rl_link_ok(&g_rl, now)) return false;
    if (!g_rl.status.latched || g_rl.status.armed_mask) return false;
    for (int i = 0; i < RL_MAX_SERVOS; i++)
        if (g_rl.cfg.present_mask >> i & 1 && g_rl.servo[i].valid && now - g_rl.servo[i].at_ms < 5000) return false;
    return true;
}

static void bus_scan_task(void *arg)
{
    (void)arg;
    scan_rx = xStreamBufferCreate(256, 1);
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(CONFIG_RL_BUS_SCAN_S * 1000));
        RL_LOCK("bus_scan_task");
        bool go = scan_wanted(rl_now_ms());
        if (go) { g_owner_kind = OWNER_SCAN; rl_pause(&g_rl, true); }
        xSemaphoreGive(g_rl_lock);
        if (!go) continue;

        vTaskDelay(pdMS_TO_TICKS(30)); /* let a reply already in flight land */
        char found[160] = "";
        int n_found = 0, len = 0;
        uint32_t t0 = rl_now_ms();
        for (int id = 0; id <= 253; id++) {
            xStreamBufferReset(scan_rx);
            uint8_t pkt[8];
            size_t n = hx_packet(pkt, (uint8_t)id, 1, NULL, 0);
            link_uart_write(pkt, n);
            hx_framer_t fr;
            hx_framer_init(&fr);
            uint32_t until = rl_now_ms() + 8;
            bool answered = false;
            uint8_t err = 0;
            while (!answered && (int32_t)(until - rl_now_ms()) > 0) {
                uint8_t b;
                if (xStreamBufferReceive(scan_rx, &b, 1, pdMS_TO_TICKS(1)) != 1) continue;
                size_t f = hx_framer_push(&fr, b);
                if (f >= 6 && fr.buf[2] == id) { answered = true; err = fr.buf[4]; }
                else if (f) hx_framer_init(&fr);
            }
            if (answered) {
                n_found++;
                if (len < (int)sizeof found - 16) len += snprintf(found + len, sizeof found - len, " %d%s", id, err ? "(err)" : "");
            }
            vTaskDelay(pdMS_TO_TICKS(3)); /* past the bridge's 2 ms reply window */
        }
        RL_LOCK("bus_scan_task");
        g_owner_kind = OWNER_NONE;
        rl_pause(&g_rl, false);
        xSemaphoreGive(g_rl_lock);
        if (n_found) ESP_LOGW(TAG, "bus scan (%lu ms): %d ID(s) answer a ping:%s; configured mask 0x%x from ID %d",
                              (unsigned long)(rl_now_ms() - t0), n_found, found, CONFIG_RL_PRESENT_MASK, CONFIG_RL_FIRST_ID);
        else ESP_LOGW(TAG, "bus scan (%lu ms): no ID 0-253 answers a ping", (unsigned long)(rl_now_ms() - t0));
    }
}
#endif

/* ---- the raw tunnel ------------------------------------------------------ */
/*
 * The FPGA's host UART as one TCP connection: the simulator's Rust hardware
 * layer talks to it through a pty (socat), unchanged. Available only while
 * no page owns the link; the engine is paused meanwhile and a disconnect
 * sends STOP, because the Rust side may have left a servo armed.
 */
#if CONFIG_RL_TUNNEL_PORT
static void tunnel_task(void *arg)
{
    (void)arg;
    int listener = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in addr = {.sin_family = AF_INET, .sin_port = htons(CONFIG_RL_TUNNEL_PORT), .sin_addr.s_addr = htonl(INADDR_ANY)};
    int one = 1;
    setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    if (bind(listener, (struct sockaddr *)&addr, sizeof addr) < 0 || listen(listener, 1) < 0) {
        ESP_LOGE(TAG, "tunnel: cannot listen on %d", CONFIG_RL_TUNNEL_PORT);
        vTaskDelete(NULL);
        return;
    }
    ESP_LOGI(TAG, "UART tunnel on TCP port %d", CONFIG_RL_TUNNEL_PORT);
    for (;;) {
        int s = accept(listener, NULL, NULL);
        if (s < 0) continue;
        RL_LOCK("tunnel_task");
        bool free = g_owner_kind == OWNER_NONE;
        if (free) {
            g_owner_kind = OWNER_TUNNEL;
            tunnel_sock = s;
            rl_pause(&g_rl, true);
        }
        xSemaphoreGive(g_rl_lock);
        if (!free) {
            const char refusal[] = "BUSY: a page owns the robot link\n";
            send(s, refusal, sizeof refusal - 1, 0);
            close(s);
            continue;
        }
        setsockopt(s, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
        ESP_LOGI(TAG, "tunnel: a host took the link");
        uint8_t buf[256];
        for (;;) {
            int n = recv(s, buf, sizeof buf, 0);
            if (n <= 0) break;
            link_uart_write(buf, (size_t)n);
        }
        RL_LOCK("tunnel_task");
        tunnel_sock = -1;
        g_owner_kind = OWNER_NONE;
        rl_pause(&g_rl, false);
        rl_stop(&g_rl);
        xSemaphoreGive(g_rl_lock);
        close(s);
        ESP_LOGI(TAG, "tunnel: host gone; STOP sent");
    }
}
#endif

void app_main(void)
{
    esp_err_t nvs = nvs_flash_init();
    if (nvs == ESP_ERR_NVS_NO_FREE_PAGES || nvs == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        nvs = nvs_flash_init();
    }
    ESP_ERROR_CHECK(nvs);

    g_rl_lock = xSemaphoreCreateMutex();
    rl_config_t cfg = rl_default_config();
    cfg.first_id = CONFIG_RL_FIRST_ID;
    cfg.present_mask = CONFIG_RL_PRESENT_MASK;
    cfg.duty_cap = CONFIG_RL_DUTY_CAP;
    rl_init(&g_rl, cfg, uart_write_cb, NULL);
    /* The gait log from the heap, not static: see web.c's plan buffer. */
    rl_gait_sample_t *gait_log = malloc(RL_GAIT_LOG * sizeof *gait_log);
    if (gait_log) rl_gait_set_log(&g_rl, gait_log, RL_GAIT_LOG);
    else ESP_LOGE(TAG, "no memory for the gait log: runs will not be recorded");

    gait_store_init();
    uart_start();
    xTaskCreatePinnedToCore(link_task, "link", 4096, NULL, 10, NULL, 1);
#if CONFIG_RL_STATUS_LOG_MS
    xTaskCreate(status_log_task, "status_log", 4096, NULL, 3, NULL);
#endif
#if CONFIG_RL_BUS_SCAN_S
    xTaskCreate(bus_scan_task, "bus_scan", 4096, NULL, 4, NULL);
#endif
    wifi_start();
    web_start();
#if CONFIG_RL_TUNNEL_PORT
    xTaskCreate(tunnel_task, "tunnel", 4096, NULL, 5, NULL);
#endif
}
