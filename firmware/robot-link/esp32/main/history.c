/* See history.h. */
#include "history.h"
#include "link_shared.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_log.h"
#include "freertos/task.h"

static const char *TAG = "history";

typedef struct {
    uint32_t ms;
    int16_t  position, speed, load;
    uint16_t current;
    uint8_t  voltage, temperature, armed, pad;
} sample_t;

static sample_t *ring;             /* [RL_MAX_SERVOS][HISTORY_SAMPLES] */
static uint32_t count[RL_MAX_SERVOS];

static void history_task(void *arg)
{
    (void)arg;
    TickType_t wake = xTaskGetTickCount();
    for (;;) {
        vTaskDelayUntil(&wake, pdMS_TO_TICKS(HISTORY_PERIOD_MS));
        RL_LOCK("history_task");
        uint32_t now = rl_now_ms();
        for (int i = 0; i < RL_MAX_SERVOS; i++) {
            const rl_servo_t *s = &g_rl.servo[i];
            if (!(g_rl.cfg.present_mask >> i & 1) || !s->valid || now - s->at_ms > 2000) continue;
            sample_t *x = &ring[i * HISTORY_SAMPLES + count[i] % HISTORY_SAMPLES];
            *x = (sample_t){.ms = now, .position = (int16_t)s->counts, .speed = s->t.speed_counts_s, .load = s->t.load_raw,
                            .current = s->t.current_raw, .voltage = s->t.voltage_raw, .temperature = s->t.temperature_c,
                            .armed = g_rl.status_valid && (g_rl.status.armed_mask >> i & 1)};
            count[i]++;
        }
        xSemaphoreGive(g_rl_lock);
    }
}

void history_start(void)
{
    ring = calloc((size_t)RL_MAX_SERVOS * HISTORY_SAMPLES, sizeof *ring);
    if (!ring) {
        ESP_LOGE(TAG, "no memory for telemetry history");
        return;
    }
    xTaskCreate(history_task, "history", 3072, NULL, 3, NULL);
}

esp_err_t history_get(httpd_req_t *req)
{
    httpd_resp_set_type(req, "application/json");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    if (!ring) return httpd_resp_send(req, "{\"error\":\"no history\"}", HTTPD_RESP_USE_STRLEN);
    /* A copy under the lock, the JSON streamed after it. */
    static sample_t *copy; /* the heap, not .bss: internal RAM the Wi-Fi link needs at boot */
    if (!copy && !(copy = malloc(HISTORY_SAMPLES * sizeof *copy))) return httpd_resp_send(req, "{\"error\":\"no memory\"}", HTTPD_RESP_USE_STRLEN);
    char head[96];
    snprintf(head, sizeof head, "{\"period_ms\":%d,\"now_ms\":%lu,\"motors\":{", HISTORY_PERIOD_MS, (unsigned long)rl_now_ms());
    httpd_resp_send_chunk(req, head, HTTPD_RESP_USE_STRLEN);
    bool first_motor = true;
    char *buf = malloc(4096);
    if (!buf) return httpd_resp_send_chunk(req, NULL, 0);
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        RL_LOCK("history_get");
        uint32_t n = count[i] < HISTORY_SAMPLES ? count[i] : HISTORY_SAMPLES, start = count[i] - n;
        for (uint32_t k = 0; k < n; k++) copy[k] = ring[i * HISTORY_SAMPLES + (start + k) % HISTORY_SAMPLES];
        uint8_t id = (uint8_t)(g_rl.cfg.first_id + i);
        xSemaphoreGive(g_rl_lock);
        if (!n) continue;
        static const char *const fields[] = {"t", "position", "speed", "load", "current", "voltage", "temperature", "armed"};
        int len = snprintf(buf, 4096, "%s\"%u\":{", first_motor ? "" : ",", id);
        first_motor = false;
        httpd_resp_send_chunk(req, buf, len);
        for (int f = 0; f < 8; f++) {
            len = snprintf(buf, 4096, "%s\"%s\":[", f ? "," : "", fields[f]);
            for (uint32_t k = 0; k < n; k++) {
                const sample_t *x = &copy[k];
                long v = f == 0 ? (long)x->ms : f == 1 ? x->position : f == 2 ? x->speed : f == 3 ? x->load : f == 4 ? x->current
                       : f == 5 ? x->voltage : f == 6 ? x->temperature : x->armed;
                if (len > 4000) { httpd_resp_send_chunk(req, buf, len); len = 0; }
                len += snprintf(buf + len, 4096 - len, "%s%ld", k ? "," : "", v);
            }
            len += snprintf(buf + len, 4096 - len, "]");
            httpd_resp_send_chunk(req, buf, len);
        }
        httpd_resp_send_chunk(req, "}", 1);
    }
    free(buf);
    httpd_resp_send_chunk(req, "}}", 2);
    return httpd_resp_send_chunk(req, NULL, 0);
}
