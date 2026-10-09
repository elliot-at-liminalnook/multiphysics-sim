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
 * The plans section starts with the pack's axes table: each motor's taught
 * travel, which becomes the engine's (rl_travel_set_pack). Poses the operator
 * teaches on the page are kept in NVS ("rlcal"/"travel") with the digest of
 * the pack they were taught against and restored at boot only onto that pack.
 * Storing another pack keeps a motor's taught poses when the new pack has
 * that motor's travel unchanged (new gaits, say), and replaces them when it
 * changes it (the Mac promotes taught poses first: leg_gait_pack
 * --pull-calibration).
 */
#include "gait_store.h"
#include "link_shared.h"
#include <string.h>
#include "esp_log.h"
#include "esp_partition.h"
#include "nvs.h"
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
    mbedtls_sha256_init(&sha);
    mbedtls_sha256_starts(&sha, 0);
    static uint8_t chunk[4096];
    for (uint32_t at = HEADER; at < HEADER + j + b;) {
        uint32_t n = HEADER + j + b - at > sizeof chunk ? sizeof chunk : HEADER + j + b - at;
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

typedef struct {
    char    pack[17];            /* the first 16 hex digits of the pack's digest */
    uint8_t count;
    struct { uint8_t id, open_lo, open_hi, pad; int32_t lo, hi; } m[RL_MAX_SERVOS];
} saved_travel_t;

/* Poses taught on the page, back onto the pack they were taught against (lock held). */
static void restore_travel(void)
{
    nvs_handle_t h;
    saved_travel_t saved;
    size_t len = sizeof saved;
    if (nvs_open("rlcal", NVS_READONLY, &h) != ESP_OK) return;
    esp_err_t r = nvs_get_blob(h, "travel", &saved, &len);
    nvs_close(h);
    if (r != ESP_OK || len != sizeof saved || saved.count > RL_MAX_SERVOS) return;
    saved.pack[16] = 0;
    if (strncmp(saved.pack, info.sha256, 16)) {
        ESP_LOGW(TAG, "poses taught on the page belong to another pack: not restored");
        return;
    }
    for (int k = 0; k < saved.count; k++)
        if (rl_travel_restore(&g_rl, saved.m[k].id, saved.m[k].lo, saved.m[k].hi, saved.m[k].open_lo, saved.m[k].open_hi))
            ESP_LOGW(TAG, "motor %u: travel taught on the page restored (%ld..%ld%s%s), not yet promoted", saved.m[k].id, (long)saved.m[k].lo,
                     (long)saved.m[k].hi, saved.m[k].open_lo ? ", low end open" : "", saved.m[k].open_hi ? ", high end open" : "");
}

esp_err_t gait_store_save_travel(void)
{
    saved_travel_t saved = {0};
    snprintf(saved.pack, sizeof saved.pack, "%.16s", info.sha256);
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        const rl_travel_t *t = &g_rl.servo[i].travel;
        if (!rl_travel_changed(t)) continue;
        saved.m[saved.count].id = (uint8_t)(g_rl.cfg.first_id + i);
        saved.m[saved.count].open_lo = t->open[0];
        saved.m[saved.count].open_hi = t->open[1];
        saved.m[saved.count].lo = t->lo;
        saved.m[saved.count].hi = t->hi;
        saved.count++;
    }
    nvs_handle_t h;
    esp_err_t r = nvs_open("rlcal", NVS_READWRITE, &h);
    if (r != ESP_OK) return r;
    r = saved.count ? nvs_set_blob(h, "travel", &saved, sizeof saved) : nvs_erase_key(h, "travel");
    if (r == ESP_ERR_NVS_NOT_FOUND) r = ESP_OK;
    if (r == ESP_OK) r = nvs_commit(h);
    nvs_close(h);
    g_rl.travel_dirty = false;
    return r;
}

void gait_store_init(void)
{
    part = esp_partition_find_first(ESP_PARTITION_TYPE_DATA, PACK_SUBTYPE, "gaitpack");
    if (!part) {
        ESP_LOGE(TAG, "no gaitpack partition (partitions.csv)");
        return;
    }
    scan();
    if (info.valid) ESP_LOGI(TAG, "gait pack: %lu bytes of JSON, %lu of plans, sha256 %.16s...", (unsigned long)info.json_len, (unsigned long)info.bin_len, info.sha256);
    else ESP_LOGI(TAG, "no gait pack stored; upload one with POST /api/gaits");
    RL_LOCK("gait_store_init");
    int n = load_travel();
    if (n) restore_travel();
    xSemaphoreGive(g_rl_lock);
    if (n) ESP_LOGI(TAG, "taught travel for %d motors from the pack", n);
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
    static uint8_t chunk[4096];
    size_t at = HEADER;
    while (at < total) {
        size_t want = total - at > sizeof chunk ? sizeof chunk : total - at;
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
    rl_travel_t before[RL_MAX_SERVOS];
    for (int i = 0; i < RL_MAX_SERVOS; i++) before[i] = g_rl.servo[i].travel;
    load_travel();
    int dropped = 0;
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        const rl_travel_t *was = &before[i], *now = &g_rl.servo[i].travel;
        if (!rl_travel_changed(was)) continue;
        if (now->known && now->pack_lo == was->pack_lo && now->pack_hi == was->pack_hi)
            rl_travel_restore(&g_rl, (uint8_t)(g_rl.cfg.first_id + i), was->lo, was->hi, was->open[0], was->open[1]);
        else dropped++;
    }
    gait_store_save_travel();
    xSemaphoreGive(g_rl_lock);
    if (dropped) ESP_LOGW(TAG, "the new pack's travel replaced poses taught on the page for %d motors", dropped);
    ESP_LOGI(TAG, "gait pack stored: %u bytes, sha256 %.16s...", (unsigned)total, info.sha256);
    char text[160];
    snprintf(text, sizeof text, "{\"stored\":true,\"bytes\":%u,\"sha256\":\"%s\"}", (unsigned)total, info.sha256);
    return reply(req, "200 OK", text);
}

esp_err_t gait_store_send_json(httpd_req_t *req)
{
    httpd_resp_set_type(req, "application/json");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    if (!info.valid) return httpd_resp_send(req, "{\"valid\":false}", HTTPD_RESP_USE_STRLEN);
    static uint8_t chunk[4096];
    for (uint32_t at = 0; at < info.json_len;) {
        uint32_t n = info.json_len - at > sizeof chunk ? sizeof chunk : info.json_len - at;
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
