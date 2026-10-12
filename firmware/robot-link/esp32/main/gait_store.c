/*
 * The gait pack (crates/sim-runtime/src/hardware/calibration/gait_pack.rs)
 * lives in its own flash partition, exactly as uploaded:
 *
 *     "RLGP" | u32 version | u32 json_len | u32 bin_len | sha256 | json | plans
 *
 * An upload is written header-last: the body after the 48-byte header goes
 * first, its digest is checked against the header's, and only then is the
 * header written. A partition without a whole, matching pack therefore never
 * reads as valid, whatever interrupted the upload.
 *
 * The plans section starts with the pack's axes table: the travel each motor
 * had when the pack was built (rl_travel_set_pack), which its plans assume.
 * The motors' taught poses themselves are the robot configuration's
 * (robot_cfg.c); a slot not yet taught is seeded from the pack.
 */
#include "gait_store.h"
#include "link_shared.h"
#include <string.h>
#include "esp_log.h"
#include "esp_partition.h"
#include "robot_cfg.h"
#include "mbedtls/sha256.h"

static const char *TAG = "gait-store";
#define HEADER 48
#define PACK_SUBTYPE 0x40
/* gait_pack.rs PACK_VERSION: 2 added the drive mode and multi-turn motors to each plan
 * axis, 3 the axes table of taught travel at the start of the plans section. */
#define PACK_VERSION 3
#define AXIS_ROW 16

static const esp_partition_t *part;
static gait_store_info_t info;

/* Pack reads and writes, 4 KB at a time: at boot and on the one HTTP server task, never at once.
 * From the heap after start-up, not .bss (internal RAM the Wi-Fi link needs at boot). */
#define CHUNK 4096
static uint8_t *chunk;

static uint32_t le32(const uint8_t *p)
{
    return (uint32_t)p[0] | (uint32_t)p[1] << 8 | (uint32_t)p[2] << 16 | (uint32_t)p[3] << 24;
}

/* Read the header and check the digest of what follows it. */
static void scan(void)
{
    memset(&info, 0, sizeof info);
    uint8_t h[HEADER];
    if (!part || esp_partition_read(part, 0, h, HEADER) != ESP_OK) return;
    if (memcmp(h, "RLGP", 4) || le32(h + 4) != PACK_VERSION) return;
    uint32_t j = le32(h + 8), b = le32(h + 12);
    if ((uint64_t)HEADER + j + b > part->size) return;
    mbedtls_sha256_context sha;
    if (!chunk) return;
    mbedtls_sha256_init(&sha);
    mbedtls_sha256_starts(&sha, 0);
    for (uint32_t at = HEADER; at < HEADER + j + b;) {
        uint32_t n = HEADER + j + b - at > CHUNK ? CHUNK : HEADER + j + b - at;
        if (esp_partition_read(part, at, chunk, n) != ESP_OK) { mbedtls_sha256_free(&sha); return; }
        mbedtls_sha256_update(&sha, chunk, n);
        at += n;
    }
    uint8_t digest[32];
    mbedtls_sha256_finish(&sha, digest);
    mbedtls_sha256_free(&sha);
    if (memcmp(digest, h + 16, 32)) {
        ESP_LOGW(TAG, "stored pack does not match its digest; ignored");
        return;
    }
    info.valid = true;
    info.json_len = j;
    info.bin_len = b;
    for (int i = 0; i < 32; i++) snprintf(info.sha256 + 2 * i, 3, "%02x", digest[i]);
}

static int32_t le32s(const uint8_t *p)
{
    return (int32_t)le32(p);
}

/* The pack's axes table into the engine (call with the lock held). Returns the motors set. */
static int load_travel(void)
{
    uint8_t head[8], row[AXIS_ROW];
    if (!info.valid || info.bin_len < sizeof head || esp_partition_read(part, HEADER + info.json_len, head, sizeof head) != ESP_OK) return 0;
    if (memcmp(head, "AXES", 4) || head[4] > RL_MAX_SERVOS || sizeof head + (uint32_t)head[4] * AXIS_ROW > info.bin_len) {
        ESP_LOGE(TAG, "the stored pack has no axes table: motors cannot be armed on the calibration profile");
        return 0;
    }
    int n = 0;
    for (int k = 0; k < head[4]; k++) {
        if (esp_partition_read(part, HEADER + info.json_len + sizeof head + (uint32_t)k * AXIS_ROW, row, sizeof row) != ESP_OK) return n;
        int32_t lo = le32s(row + 4), hi = le32s(row + 8), margin = le32s(row + 12);
        if (lo >= hi || rl_axis(&g_rl, row[0]) < 0) continue;
        rl_travel_set_pack(&g_rl, row[0], lo, hi, row[1] & 1, row[1] & 2, margin);
        n++;
    }
    return n;
}

void gait_store_init(void)
{
    chunk = malloc(CHUNK);
    if (!chunk) ESP_LOGE(TAG, "no memory for pack reads");
    part = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, PACK_SUBTYPE, "gaitpack");
    if (!part) {
        ESP_LOGE(TAG, "no gaitpack partition (partitions.csv)");
        return;
    }
    scan();
    if (info.valid) ESP_LOGI(TAG, "gait pack: %lu bytes of JSON, %lu of plans, sha256 %.16s...", (unsigned long)info.json_len, (unsigned long)info.bin_len, info.sha256);
    else ESP_LOGI(TAG, "no gait pack stored; upload one with POST /api/gaits");
    RL_LOCK("gait_store_init");
    robot_cfg_apply(); /* the configured motors present first: the pack's travel is only taken for them */
    int n = load_travel();
    bool seeded = robot_cfg_seed_travel();
    robot_cfg_apply();
    if (seeded) robot_cfg_save();
    xSemaphoreGive(g_rl_lock);
    if (n) ESP_LOGI(TAG, "the pack carries travel for %d motors%s", n, seeded ? "; untaught slots seeded from it" : "");
}

gait_store_info_t gait_store_info(void)
{
    return info;
}

static esp_err_t reply(httpd_req_t *req, const char *status, const char *json)
{
    httpd_resp_set_status(req, status);
    httpd_resp_set_type(req, "application/json");
    return httpd_resp_send(req, json, HTTPD_RESP_USE_STRLEN);
}

esp_err_t gait_store_upload(httpd_req_t *req)
{
    if (!part) return reply(req, "500 Internal Server Error", "{\"error\":\"no gaitpack partition\"}");
    RL_LOCK("gait_store_upload");
    bool busy = rl_gait_active(&g_rl);
    xSemaphoreGive(g_rl_lock);
    if (busy) return reply(req, "409 Conflict", "{\"error\":\"a gait is playing; stop it first\"}");
    size_t total = req->content_len;
    if (total < HEADER || total > part->size) return reply(req, "413 Payload Too Large", "{\"error\":\"the pack must fit the gaitpack partition\"}");
    uint8_t header[HEADER];
    size_t got = 0;
    while (got < HEADER) {
        int r = httpd_req_recv(req, (char *)header + got, HEADER - got);
        if (r <= 0) return ESP_FAIL;
        got += (size_t)r;
    }
    if (memcmp(header, "RLGP", 4) || le32(header + 4) != PACK_VERSION || HEADER + le32(header + 8) + le32(header + 12) != total)
        return reply(req, "400 Bad Request", "{\"error\":\"not a version-2 robot-link gait pack (rebuild it with leg_gait_pack), or its length disagrees with its header\"}");
    /* Invalidate first: erase the whole range, header included. */
    size_t erase = (total + 4095) & ~(size_t)4095;
    if (esp_partition_erase_range(part, 0, erase) != ESP_OK) return reply(req, "500 Internal Server Error", "{\"error\":\"erase failed\"}");
    memset(&info, 0, sizeof info);
    mbedtls_sha256_context sha;
    mbedtls_sha256_init(&sha);
    mbedtls_sha256_starts(&sha, 0);
    size_t at = HEADER;
    while (at < total) {
        size_t want = total - at > CHUNK ? CHUNK : total - at;
        int r = httpd_req_recv(req, (char *)chunk, want);
        if (r == HTTPD_SOCK_ERR_TIMEOUT) continue;
        if (r <= 0) { mbedtls_sha256_free(&sha); return ESP_FAIL; }
        if (esp_partition_write(part, at, chunk, (size_t)r) != ESP_OK) { mbedtls_sha256_free(&sha); return reply(req, "500 Internal Server Error", "{\"error\":\"write failed\"}"); }
        mbedtls_sha256_update(&sha, chunk, (size_t)r);
        at += (size_t)r;
    }
    uint8_t digest[32];
    mbedtls_sha256_finish(&sha, digest);
    mbedtls_sha256_free(&sha);
    if (memcmp(digest, header + 16, 32)) return reply(req, "400 Bad Request", "{\"error\":\"the pack's digest does not match its contents\"}");
    if (esp_partition_write(part, 0, header, HEADER) != ESP_OK) return reply(req, "500 Internal Server Error", "{\"error\":\"header write failed\"}");
    scan();
    if (!info.valid) return reply(req, "500 Internal Server Error", "{\"error\":\"the stored pack did not read back\"}");
    /* The new pack's multi-turn poses may count from another turn, and its travel replaces any taught here. */
    RL_LOCK("gait_store_upload");
    rl_turn_forget_all(&g_rl, "a new gait pack was stored", rl_now_ms());
    /* The pack's travel is what its gaits were built from; the robot configuration's
     * taught poses stay (a motor whose poses differ refuses gaits until promoted). */
    robot_cfg_apply();
    load_travel();
    bool seeded = robot_cfg_seed_travel();
    robot_cfg_apply();
    if (seeded) robot_cfg_save();
    xSemaphoreGive(g_rl_lock);
    ESP_LOGI(TAG, "gait pack stored: %u bytes, sha256 %.16s...", (unsigned)total, info.sha256);
    char text[160];
    snprintf(text, sizeof text, "{\"stored\":true,\"bytes\":%u,\"sha256\":\"%s\"}", (unsigned)total, info.sha256);
    return reply(req, "200 OK", text);
}

esp_err_t gait_store_send_json(httpd_req_t *req)
{
    httpd_resp_set_type(req, "application/json");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    if (!info.valid || !chunk) return httpd_resp_send(req, "{\"valid\":false}", HTTPD_RESP_USE_STRLEN);
    for (uint32_t at = 0; at < info.json_len;) {
        uint32_t n = info.json_len - at > CHUNK ? CHUNK : info.json_len - at;
        if (esp_partition_read(part, HEADER + at, chunk, n) != ESP_OK) return httpd_resp_send_chunk(req, NULL, 0);
        if (httpd_resp_send_chunk(req, (const char *)chunk, n) != ESP_OK) return ESP_FAIL;
        at += n;
    }
    return httpd_resp_send_chunk(req, NULL, 0);
}

bool gait_store_read_plan(uint32_t offset, uint32_t length, uint8_t *buf, size_t cap)
{
    if (!info.valid || length > cap || (uint64_t)offset + length > info.bin_len) return false;
    return esp_partition_read(part, HEADER + info.json_len + offset, buf, length) == ESP_OK;
}
