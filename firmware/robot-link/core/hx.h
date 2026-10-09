/*
 * hx -- the FPGA bench bridge's host wire format, free of any platform.
 *
 * The same bytes the Rust hardware layer sends over the dock's USB UART
 * (crates/sim-runtime/src/acquisition/{servo_bus,servo_safety}.rs): HX bus
 * packets
 *
 *     FF FF | ID | LEN | INSTR-or-ERROR | params[LEN-2] | ~sum(ID..params)
 *
 * with the supervisor's own commands addressed to ID FE, instruction A0
 * (sipeed-tang-primer-25k/servo/safety.md, "Host protocol, version 1").
 * Change one side, change all of them.
 */
#ifndef HX_H
#define HX_H
#ifdef __cplusplus
extern "C" {
#endif
#include <stddef.h>
#include <stdint.h>

#define HX_MAX_FRAME   64   /* the bridge's packet buffer */
#define HX_MAX_PARAMS  58
#define HX_BRIDGE_ID   0xFE
#define HX_LOCAL       0xA0 /* supervisor instruction */

#define HX_PING  0x01
#define HX_READ  0x02
#define HX_WRITE 0x03

/* Registers. */
#define HX_REG_MODE      0x21 /* 0 position, 1 speed, 2 open-loop PWM */
#define HX_REG_TORQUE    0x28
#define HX_REG_PWM       0x2C
#define HX_REG_TELEMETRY 0x38 /* 15 bytes */
#define HX_TELEMETRY_LEN 15
#define HX_STATUS_LEN    13
/* Verified on nine HX-30HM units, firmware 3.15 (servo_bus.rs). */
#define HX_PWM_DIRECTION_BIT 10

/* Supervisor operations (parameter 0 of an A0 packet). */
enum { HX_OP_STOP = 0, HX_OP_ARM = 1, HX_OP_STATUS = 2, HX_OP_HEARTBEAT = 3, HX_OP_DISARM = 4, HX_OP_HEARTBEAT_MASK = 5 };

/* Build a packet into out (HX_MAX_FRAME bytes). Returns its length, 0 if it cannot be built. */
size_t hx_packet(uint8_t *out, uint8_t id, uint8_t instruction, const uint8_t *params, size_t n);

/* Signed open-loop drive, -1000..1000, as the two PWM register bytes. Returns 0 on a bad magnitude. */
int hx_pwm_bytes(int drive, uint8_t out[2]);

/*
 * Reassembles frames from a byte stream. A byte that cannot start or
 * continue a frame is dropped and counted: unlike a recording host, a
 * controller has to keep going after line noise, and the count says so.
 */
typedef struct {
    uint8_t  buf[HX_MAX_FRAME];
    size_t   len;
    uint32_t frames, dropped_bytes, checksum_errors;
} hx_framer_t;

void hx_framer_init(hx_framer_t *f);
/* Feed one byte; returns the frame length when buf holds a checksum-valid frame (consume it before the next byte), else 0. */
size_t hx_framer_push(hx_framer_t *f, uint8_t byte);

typedef struct {
    uint8_t  version, latched, reason, fault_id;
    uint16_t armed_mask, fresh_mask; /* bit 0 = the profile's first ID */
    uint8_t  temperature_max_c, voltage_min_raw, voltage_max_raw;
    uint16_t current_max_raw;
} hx_status_t;
/* Decode the 13 status bytes; returns 0 unless they are a consistent version-1 status. */
int hx_status_decode(const uint8_t *p, size_t n, hx_status_t *out);

typedef struct {
    uint16_t position_raw; /* 0..4095 per turn */
    int16_t  speed_counts_s;
    uint16_t load_raw, current_raw;
    uint8_t  voltage_raw /* 0.1 V */, temperature_c, status, moving;
} hx_telemetry_t;
int hx_telemetry_decode(const uint8_t *p, size_t n, hx_telemetry_t *out);

const char *hx_reason_name(uint8_t reason);
#ifdef __cplusplus
}
#endif
#endif
