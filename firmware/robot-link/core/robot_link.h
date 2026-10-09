/*
 * robot_link -- the ESP32's side of the FPGA bench bridge, free of any
 * platform: no ESP-IDF, no sockets, no malloc, no clock of its own. The
 * same file runs in the firmware, in the host unit tests (test/) and
 * against the real supervisor RTL under Verilator (cosim/).
 *
 * What it owns:
 *   - one transaction at a time on the FPGA's host UART (the bridge latches
 *     a fault if a byte arrives while it is forwarding, and drops a refused
 *     packet without a reply: every request waits for its reply or a timeout);
 *   - telemetry polling (the supervisor trips an armed servo whose telemetry
 *     is older than 200 ms, and only a host can ask for it);
 *   - the operator's lease: heartbeats go to the FPGA only while a session
 *     says it is alive, so a closed page, a dropped Wi-Fi link or a crashed
 *     ESP32 all end in the FPGA's own 300 ms command timeout;
 *   - hold-to-move: a jog is driven only while it is being renewed, and is
 *     written to zero the moment it is not.
 *
 * What it does not own: any safety promise. Temperature, voltage, current,
 * the S2 button, leases and stopping are the FPGA's (hx_safety.v), and hold
 * whatever this code does.
 */
#ifndef ROBOT_LINK_H
#define ROBOT_LINK_H
#ifdef __cplusplus
extern "C" {
#endif
#include "hx.h"
#include "gait.h"
#include <stdbool.h>

#define RL_MAX_SERVOS 9

typedef struct {
    uint8_t  first_id;        /* the FPGA profile's first ID (bit 0 of its masks) */
    uint16_t present_mask;    /* servos on the bus, bit 0 = first_id */
    int      duty_cap;        /* largest |drive| a jog may command, 0..1000 */
    uint32_t reply_timeout_ms;
    uint32_t gap_ms;          /* quiet time between transactions */
    uint32_t heartbeat_ms;    /* FPGA lease is 300 ms */
    uint32_t fast_poll_ms;    /* armed servos (FPGA wants < 200 ms) */
    uint32_t slow_poll_ms;    /* the others */
    uint32_t status_ms;
    uint32_t jog_ms;          /* PWM rewrite period while held */
    uint32_t hold_timeout_ms; /* a jog not renewed for this long ends */
    uint32_t session_timeout_ms;
    uint32_t gait_period_ms;  /* goal period while a gait plays (the panel's sweep period) */
    uint32_t gait_approach_ms;/* the first pose must be reached within this */
    float    gait_tolerance_counts; /* a motor this close to its goal has arrived */
} rl_config_t;

/* ---- gait playback on the leg (the FPGA's calibration profile) ----------- */
enum { RL_GAIT_IDLE, RL_GAIT_PREPARE, RL_GAIT_APPROACH, RL_GAIT_PLAYING, RL_GAIT_STOPPING };
typedef struct {
    uint32_t ms;
    float    gait_t, command, desired;
    int32_t  actual;   /* encoder counts, plus the confirmed turns of a multi-turn motor */
    uint8_t  id;
} rl_gait_sample_t;
#define RL_GAIT_LOG 2048 /* the firmware's log size (the caller owns the buffer: rl_gait_set_log) */
typedef struct {
    int      phase;
    bool     have_plan;
    rl_plan_t plan;
    bool     playing;
    float    speed_scale;
    float    t;                  /* gait time, s */
    uint32_t clock_ms, phase_ms, next_goal_ms;
    int      axis, step;         /* preparation: plan axis and step */
    bool     waiting, done, ok;  /* the gait's own transaction */
    uint8_t  read[2];
    rl_gov_t gov[RL_GAIT_MAX_AXES];
    float    goal[RL_GAIT_MAX_AXES], goal_v[RL_GAIT_MAX_AXES], desired[RL_GAIT_MAX_AXES];
    uint8_t  goal_pending;       /* plan-axis bits */
    /* Stop verification. */
    int      stationary[RL_GAIT_MAX_AXES];
    int32_t  last_position[RL_GAIT_MAX_AXES];
    uint32_t verify_from_ms;
    uint8_t  verify_torque;
    uint16_t verify_pwm;
    bool     stop_verified;
    char     result[128];
    uint32_t log_count;          /* total samples; the log keeps the last log_cap */
    rl_gait_sample_t *log;       /* the caller's buffer (none: nothing is logged) */
    uint32_t log_cap;
} rl_gait_t;

rl_config_t rl_default_config(void);

/*
 * Turns. Every reading moves `counts` by the nearest-turn difference from
 * the last one (as the Rust panel's EncoderTurns and the FPGA do), so
 * `counts` is always congruent to the reading modulo 4096. Which turn that
 * is, the engine cannot know: a multi-turn motor's turn is known only once
 * the operator has confirmed it (rl_turn_confirm), and is forgotten on
 * anything that could hide a turn: a missed or corrupt reading, a half-turn
 * jump, a gap between readings longer than RL_TURN_GAP_MS, the engine
 * pausing for a tunnel or bus scan, or a gait stop that was not verified.
 */
/*
 * Taught travel (the leg's calibration profile, status version 5 and later).
 * The FPGA arms a motor only inside a window it was sent, anchored to the
 * live reading. Each motor's window comes from the stored gait pack (built
 * from calibration.json), or from poses the operator taught on the page,
 * which are used at once but marked changed until the Mac promotes them
 * (leg_gait_pack --pull-calibration) and a pack built from them is stored. An
 * end the operator opened is unbounded (as the panel's cleared pose) so the
 * motor can be jogged to a new end; a gait refuses a changed or open motor.
 * A motor found outside its travel is armed with the window stretched only
 * to where it already is, so the FPGA lets it move back in and no further out.
 */
#define RL_TRAVEL_OPEN  8000000 /* the FPGA's numeric bound: an open end */
#define RL_TRAVEL_MIN   64      /* the narrowest travel that may be taught */
enum { RL_END_LO = 0, RL_END_HI = 1 };
typedef struct {
    bool     known;              /* the stored pack names this motor */
    bool     multi_turn;
    bool     reversed;           /* the calibration's lower pose is the high-count end */
    int32_t  margin;             /* turn margin (multi-turn) */
    int32_t  pack_lo, pack_hi;   /* from the pack: calibration.json when it was built */
    int32_t  lo, hi;             /* taught now (the pack's, or set on the page) */
    bool     open[2];            /* an end opened on the page */
    bool     armed;              /* the window below was sent and armed */
    bool     recovering;         /* ... stretched to where the motor was */
    int32_t  armed_lo, armed_hi;
} rl_travel_t;
bool rl_travel_changed(const rl_travel_t *t);

#define RL_TURN_GAP_MS  600 /* 3000 counts/s, the speed cap, covers under half a turn in 0.68 s */
#define RL_TURN_POLL_MS 200 /* a motor whose turn is known is read at least this often */
typedef struct {
    bool     valid;
    uint32_t at_ms;
    hx_telemetry_t t;
    int      mode;        /* register 0x21; -1 not read yet */
    bool     torque_on;   /* torque enable written since it was armed */
    int32_t  counts;      /* the reading plus counted turns */
    bool     tracked;     /* counts has a previous reading to count from */
    bool     turn_known;  /* the operator confirmed which turn counts is on */
    int32_t  turn_lo, turn_hi; /* the pack travel it was confirmed against */
    rl_travel_t travel;
} rl_servo_t;

typedef void (*rl_write_fn)(void *ctx, const uint8_t *bytes, size_t n);

typedef struct {
    rl_config_t cfg;
    rl_write_fn write;
    void       *ctx;
    hx_framer_t framer;

    /* What the FPGA and the servos last said. */
    bool        status_valid;
    uint32_t    status_at_ms;
    hx_status_t status;
    rl_servo_t  servo[RL_MAX_SERVOS];

    /* Requests. */
    bool     stop_req;
    uint16_t arm_req, disarm_req;
    uint8_t  jog_id;       /* 0: none */
    int      jog_duty;
    uint32_t jog_until_ms;
    bool     zero_pending[RL_MAX_SERVOS];
    bool     session;
    uint32_t session_until_ms;
    bool     paused;       /* a tunnel owns the UART */
    uint32_t tick_ms;      /* the last rl_tick's clock */

    /* The transaction in flight. */
    int      txn, txn_axis;
    uint32_t txn_at_ms, idle_at_ms;
    uint32_t next_heartbeat_ms, next_status_ms, next_jog_ms;
    uint32_t next_poll_ms[RL_MAX_SERVOS];
    unsigned poll_cursor;

    /* Counters and the last refusal, for the page. */
    uint32_t sent, replies, timeouts, unexpected, refused;
    char     note[192];
    uint32_t note_at_ms;

    /* Arming on the calibration profile: window, arm, PWM mode for jogging. */
    struct {
        int      axis, step;     /* axis -1: idle */
        bool     waiting, done, ok;
        uint8_t  read[2];
        uint32_t clock_ms;
    } arm_job;
    bool     travel_dirty;       /* a taught pose changed: the firmware saves it */

    rl_gait_t gait;
} rl_t;

void rl_init(rl_t *rl, rl_config_t cfg, rl_write_fn write, void *ctx);
/* Bytes from the FPGA. */
void rl_rx(rl_t *rl, const uint8_t *bytes, size_t n, uint32_t now_ms);
/* Call every millisecond or so: sends at most one request. */
void rl_tick(rl_t *rl, uint32_t now_ms);

/* The operator. Each returns false, with rl->note saying why, when refused. */
void rl_session_alive(rl_t *rl, uint32_t now_ms); /* the owner's page is there */
void rl_session_end(rl_t *rl);                    /* it closed: stop what it held */
void rl_stop(rl_t *rl);                           /* anyone, any time */
/* Arm for hold-to-move. On the calibration profile: the motor's taught window
 * (or the recovery window) anchored at its reading, ARM, PWM zero, then open-loop
 * PWM mode (unlocking only to change it). */
bool rl_arm(rl_t *rl, uint8_t id, uint32_t now_ms);
bool rl_disarm(rl_t *rl, uint8_t id, uint32_t now_ms);
/* Drive `id` at `duty` (-1000..1000, clamped to the cap) until not renewed. */
bool rl_hold(rl_t *rl, uint8_t id, int duty, uint32_t now_ms);
void rl_release(rl_t *rl);

/* While paused the engine sends nothing (a raw tunnel has the UART); bytes received are still parsed. */
void rl_pause(rl_t *rl, bool paused);

/* Gait playback. Load a parsed plan (it points into the caller's buffer) while idle;
 * start with a live owner session; control play/pause and speed; stop. A start
 * arms each plan axis through its taught window exactly as the Rust panel does
 * (calibration_serial.rs prepare_drive: servo position mode, or servo speed for a
 * multi-turn motor, whose turn must have been confirmed), approaches the
 * first pose through the plan's governor, then plays. Any STOP, a latch or the
 * session ending ends in a verified stop. */
/* Where gait samples go: `cap` samples of the caller's memory, kept for as long as the engine runs. */
void rl_gait_set_log(rl_t *rl, rl_gait_sample_t *buf, uint32_t cap);
bool rl_gait_load(rl_t *rl, const rl_plan_t *plan, uint32_t now_ms);
bool rl_gait_start(rl_t *rl, float speed_scale, uint32_t now_ms);
void rl_gait_control(rl_t *rl, bool playing, float speed_scale);
void rl_gait_stop(rl_t *rl, const char *why);
bool rl_gait_active(const rl_t *rl);
const char *rl_gait_phase_name(int phase);

/* Taught travel. The pack's (resets anything taught on the page); a taught
 * state restored from storage; set an end at the motor's position; open an
 * end; back to the pack's. Teaching needs a fresh reading (and a confirmed
 * turn for a multi-turn motor); none of it while a gait is active. */
void rl_travel_set_pack(rl_t *rl, uint8_t id, int32_t lo, int32_t hi, bool multi_turn, bool reversed, int32_t margin);
bool rl_travel_restore(rl_t *rl, uint8_t id, int32_t lo, int32_t hi, bool open_lo, bool open_hi);
bool rl_travel_teach(rl_t *rl, uint8_t id, int end, uint32_t now_ms);
bool rl_travel_open(rl_t *rl, uint8_t id, int end, uint32_t now_ms);
bool rl_travel_revert(rl_t *rl, uint8_t id, uint32_t now_ms);
/* Where a motor is in its travel's counts: the reading, or with its turns when multi-turn. */
int32_t rl_position(const rl_t *rl, uint8_t id);

/* Multi-turn motors. The places motor `id` may be (its reading plus whole
 * turns, inside its taught travel widened by the turn margin), at most two;
 * returns how many (0 without a fresh reading). */
int  rl_turn_candidates(const rl_t *rl, uint8_t id, uint32_t now_ms, int32_t out[2]);
/* The operator saw the leg at `counts` (one of the candidates): the motor's
 * turn is known from now until a reading is missed. Refused while a gait is active. */
bool rl_turn_confirm(rl_t *rl, uint8_t id, int32_t counts, uint32_t now_ms);
/* Forget every confirmed turn (a new gait pack may count from another turn). */
void rl_turn_forget_all(rl_t *rl, const char *why, uint32_t now_ms);

bool rl_link_ok(const rl_t *rl, uint32_t now_ms); /* the FPGA answered recently */
int  rl_axis(const rl_t *rl, uint8_t id);         /* index of a present servo, else -1 */
#ifdef __cplusplus
}
#endif
#endif
