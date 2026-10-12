/* Per-motor telemetry history for the page's graphs: every present motor
 * sampled at HISTORY_PERIOD_MS into a ring of HISTORY_SAMPLES (two minutes). */
#ifndef HISTORY_H
#define HISTORY_H
#include "esp_http_server.h"
#define HISTORY_PERIOD_MS 500
#define HISTORY_SAMPLES 240
void history_start(void);
/* GET /api/history: {"period_ms", "now_ms", "motors": {"<id>": {"t": [...ms], "position": [...], ...}}} */
esp_err_t history_get(httpd_req_t *req);
#endif
