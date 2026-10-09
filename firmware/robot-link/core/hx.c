#include "hx.h"
#include <stdbool.h>
#include <string.h>

size_t hx_packet(uint8_t *out, uint8_t id, uint8_t instruction, const uint8_t *params, size_t n)
{
    if (id == 0xFF || n > HX_MAX_PARAMS) return 0;
    out[0] = 0xFF;
    out[1] = 0xFF;
    out[2] = id;
    out[3] = (uint8_t)(n + 2);
    out[4] = instruction;
    if (n) memcpy(out + 5, params, n);
    uint8_t sum = 0;
    for (size_t i = 2; i < 5 + n; i++) sum = (uint8_t)(sum + out[i]);
    out[5 + n] = (uint8_t)~sum;
    return 6 + n;
}

int hx_pwm_bytes(int drive, uint8_t out[2])
{
    int magnitude = drive < 0 ? -drive : drive;
    if (magnitude > 1000) return 0;
    uint16_t raw = (uint16_t)magnitude | (drive < 0 ? (uint16_t)(1u << HX_PWM_DIRECTION_BIT) : 0);
    out[0] = (uint8_t)raw;
    out[1] = (uint8_t)(raw >> 8);
    return 1;
}

void hx_framer_init(hx_framer_t *f)
{
    memset(f, 0, sizeof *f);
}

/* Drop the first byte and whatever follows it that cannot begin a frame. */
static void resync(hx_framer_t *f)
{
    size_t skip = 1;
    while (skip < f->len && !(f->buf[skip] == 0xFF && (skip + 1 >= f->len || f->buf[skip + 1] == 0xFF))) skip++;
    f->dropped_bytes += (uint32_t)skip;
    f->len -= skip;
    memmove(f->buf, f->buf + skip, f->len);
}

size_t hx_framer_push(hx_framer_t *f, uint8_t byte)
{
    if (f->len == HX_MAX_FRAME) resync(f);
    f->buf[f->len++] = byte;
    for (;;) {
        if (f->len >= 1 && f->buf[0] != 0xFF) { resync(f); continue; }
        if (f->len >= 2 && f->buf[1] != 0xFF) { resync(f); continue; }
        /* FF FF FF: the first FF was idle fill before a header. */
        if (f->len >= 3 && f->buf[2] == 0xFF) { resync(f); continue; }
        if (f->len < 4) return 0;
        size_t total = (size_t)f->buf[3] + 4;
        if (total < 6 || total > HX_MAX_FRAME) { resync(f); continue; }
        if (f->len < total) return 0;
        uint8_t sum = 0;
        for (size_t i = 2; i < total; i++) sum = (uint8_t)(sum + f->buf[i]);
        if (sum != 0xFF) { f->checksum_errors++; resync(f); continue; }
        f->frames++;
        return total;
    }
}

int hx_status_decode(const uint8_t *p, size_t n, hx_status_t *s)
{
    /* Version 1: bridge_safety. 5-8: the calibration profiles (taught windows,
     * IDs 1-3), which add reason 13, an ambiguous half-turn encoder jump. */
    bool known_version = p[0] == 1 || (p[0] >= 5 && p[0] <= 8);
    if (n != HX_STATUS_LEN || !known_version || p[1] > 1 || p[2] > 13 || p[2] == 12) return 0;
    uint16_t armed = (uint16_t)(p[4] | (p[5] << 8)), fresh = (uint16_t)(p[6] | (p[7] << 8));
    if (((armed | fresh) & ~0x1FF) || (p[1] == 1 && armed) || p[9] > p[10]) return 0;
    s->version = p[0];
    s->latched = p[1];
    s->reason = p[2];
    s->fault_id = p[3];
    s->armed_mask = armed;
    s->fresh_mask = fresh;
    s->temperature_max_c = p[8];
    s->voltage_min_raw = p[9];
    s->voltage_max_raw = p[10];
    s->current_max_raw = (uint16_t)(p[11] | (p[12] << 8));
    return 1;
}

int hx_telemetry_decode(const uint8_t *p, size_t n, hx_telemetry_t *t)
{
    if (n != HX_TELEMETRY_LEN) return 0;
    uint16_t speed = (uint16_t)(p[2] | (p[3] << 8));
    t->position_raw = (uint16_t)(p[0] | (p[1] << 8));
    t->speed_counts_s = (int16_t)((speed & 0x7FFF) * ((speed & 0x8000) ? -1 : 1));
    t->load_raw = (uint16_t)(p[4] | (p[5] << 8));
    t->voltage_raw = p[6]; /* one byte; 0x3F is temperature */
    t->temperature_c = p[7];
    t->status = p[9];
    t->moving = p[10];
    t->current_raw = (uint16_t)(p[13] | (p[14] << 8));
    return 1;
}

const char *hx_reason_name(uint8_t reason)
{
    static const char *names[] = {"healthy", "boot", "over temperature", "under voltage", "over voltage", "over current", "servo error", "telemetry timeout", "command timeout", "stop button", "host stop", "bridge fault", "unknown", "ambiguous encoder jump"};
    return reason < sizeof names / sizeof names[0] ? names[reason] : "unknown";
}
