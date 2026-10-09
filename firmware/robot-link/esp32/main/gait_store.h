/* The gait pack in flash: upload, the page's JSON, one plan at a time into RAM. */
#ifndef GAIT_STORE_H
#define GAIT_STORE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "esp_http_server.h"

typedef struct {
    bool     valid;
    uint32_t json_len, bin_len;
    char     sha256[65];
} gait_store_info_t;

void gait_store_init(void);
gait_store_info_t gait_store_info(void);
/* POST /api/gaits: the whole pack file as the body. Refused while a gait is active. */
esp_err_t gait_store_upload(httpd_req_t *req);
/* GET /api/gaits: the pack's JSON (or {"valid": false}). */
esp_err_t gait_store_send_json(httpd_req_t *req);
/* Copy the plan at [offset, offset + length) of the plans section into buf. */
bool gait_store_read_plan(uint32_t offset, uint32_t length, uint8_t *buf, size_t cap);
/* Keep the poses taught on the page (call with the engine's lock held). */
esp_err_t gait_store_save_travel(void);
#endif
