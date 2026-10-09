#include "robot_link.h"
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <string.h>

enum { T_NONE, T_STOP, T_STATUS, T_ARM, T_DISARM, T_HEARTBEAT, T_TELEMETRY, T_MODE, T_TORQUE, T_PWM, T_ZERO,
       /* The gait's own: a supervisor command, a servo write, a servo read, a goal. */
       T_G_LOCAL, T_G_WRITE, T_G_READ, T_GOAL,
       /* The calibrated arm's. */
       T_J_LOCAL, T_J_WRITE, T_J_READ };

rl_config_t rl_default_config(void)
{
    rl_config_t c = {
        .first_id = 4, .present_mask = 0, .duty_cap = 100,
        .reply_timeout_ms = 25, .gap_ms = 2, .heartbeat_ms = 100,
        .fast_poll_ms = 50, .slow_poll_ms = 500, .status_ms = 200,
        .jog_ms = 50, .hold_timeout_ms = 250, .session_timeout_ms = 1000,
        .gait_period_ms = 30, .gait_approach_ms = 30000, .gait_tolerance_counts = 12.f,
    };
    return c;
}

static void note(rl_t *rl, uint32_t now, const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(rl->note, sizeof rl->note, fmt, ap);
    va_end(ap);
    rl->note_at_ms = now;
}

void rl_init(rl_t *rl, rl_config_t cfg, rl_write_fn write, void *ctx)
{
    memset(rl, 0, sizeof *rl);
    rl->cfg = cfg;
    rl->write = write;
    rl->ctx = ctx;
    hx_framer_init(&rl->framer);
    for (int i = 0; i < RL_MAX_SERVOS; i++) rl->servo[i].mode = -1;
    rl->arm_job.axis = -1;
}

int rl_axis(const rl_t *rl, uint8_t id)
{
    int axis = (int)id - rl->cfg.first_id;
    return axis >= 0 && axis < RL_MAX_SERVOS && (rl->cfg.present_mask >> axis & 1) ? axis : -1;
}

static bool before(uint32_t a, uint32_t b) /* a is earlier than b, across wrap */
{
    return (int32_t)(a - b) < 0;
}

bool rl_link_ok(const rl_t *rl, uint32_t now)
{
    return rl->status_valid && !before(rl->status_at_ms + 3 * rl->cfg.status_ms + 2 * rl->cfg.reply_timeout_ms, now);
}

static bool session_alive(const rl_t *rl, uint32_t now)
{
    return rl->session && before(now, rl->session_until_ms);
}

static bool jog_active(const rl_t *rl, uint32_t now)
{
    return rl->jog_id && before(now, rl->jog_until_ms) && session_alive(rl, now);
}

static void end_jog(rl_t *rl)
{
    int axis = rl->jog_id ? rl_axis(rl, rl->jog_id) : -1;
    if (axis >= 0) rl->zero_pending[axis] = true;
    rl->jog_id = 0;
    rl->jog_duty = 0;
}

/* ---- the operator ----------------------------------------------------- */

void rl_session_alive(rl_t *rl, uint32_t now)
{
    rl->session = true;
    rl->session_until_ms = now + rl->cfg.session_timeout_ms;
}

static void gait_end(rl_t *rl, const char *why);
static void forget_turn(rl_t *rl, int i, uint32_t now, const char *why);
static uint8_t id_of(const rl_t *rl, int axis);
static const char *pose_name(const rl_travel_t *t, int end);

void rl_session_end(rl_t *rl)
{
    if (rl_gait_active(rl)) gait_end(rl, "the operator session ended");
    end_jog(rl);
    rl->arm_req = 0;
    rl->arm_job.axis = -1;
    /* Whatever it armed is stopped now, not when the lease runs out. */
    if (rl->session && (!rl->status_valid || rl->status.armed_mask)) rl->stop_req = true;
    rl->session = false;
}

void rl_stop(rl_t *rl)
{
    if (rl_gait_active(rl)) gait_end(rl, "STOP");
    end_jog(rl);
    rl->arm_req = rl->disarm_req = 0;
    rl->arm_job.axis = -1;
    rl->stop_req = true;
}

bool rl_arm(rl_t *rl, uint8_t id, uint32_t now)
{
    int axis = rl_axis(rl, id);
    if (axis < 0) return note(rl, now, "servo %u is not on this bus", id), false;
    if (!session_alive(rl, now)) return note(rl, now, "no live operator session"), false;
    if (rl_gait_active(rl)) return note(rl, now, "a gait is playing"), false;
    if (!rl->servo[axis].valid) return note(rl, now, "servo %u has not answered a telemetry read", id), false;
    if (rl->status_valid && rl->status.version >= 5) {
        /* The calibration profile arms only inside a window it was sent. */
        const rl_servo_t *s = &rl->servo[axis];
        if (!s->travel.known) return note(rl, now, "motor %u has no taught travel: store a gait pack (it carries calibration.json's poses)", id), false;
        if (s->travel.multi_turn && !s->turn_known) return note(rl, now, "motor %u's turn is not confirmed: pick where the leg is first", id), false;
        if (rl->arm_job.axis >= 0) return note(rl, now, "still arming motor %u", id_of(rl, rl->arm_job.axis)), false;
    }
    /* The FPGA arms only on fresh telemetry: poll it first (see rl_tick). */
    rl->next_poll_ms[axis] = now;
    rl->arm_req |= (uint16_t)(1u << axis);
    return true;
}

bool rl_disarm(rl_t *rl, uint8_t id, uint32_t now)
{
    int axis = rl_axis(rl, id);
    if (axis < 0) return note(rl, now, "servo %u is not on this bus", id), false;
    if (rl->jog_id == id) end_jog(rl);
    rl->disarm_req |= (uint16_t)(1u << axis);
    return true;
}

bool rl_hold(rl_t *rl, uint8_t id, int duty, uint32_t now)
{
    int axis = rl_axis(rl, id);
    if (axis < 0) return note(rl, now, "servo %u is not on this bus", id), false;
    if (!session_alive(rl, now)) return note(rl, now, "no live operator session"), false;
    if (rl_gait_active(rl)) return note(rl, now, "a gait is playing"), false;
    if (!rl->status_valid || rl->status.latched) return note(rl, now, "the FPGA is latched (%s)", rl->status_valid ? hx_reason_name(rl->status.reason) : "no status yet"), false;
    if (!(rl->status.armed_mask >> axis & 1)) return note(rl, now, "servo %u is not armed", id), false;
    const rl_travel_t *tr = &rl->servo[axis].travel;
    if (rl->status.version >= 5 && tr->armed && duty) {
        /* The FPGA drops drive outward past its window's edge; say so instead of timing out. */
        int32_t p = rl_position(rl, id);
        if (duty > 0 && p >= tr->armed_hi) return note(rl, now, "motor %u is at its window's end by the %s (%ld): only ◄", id, pose_name(tr, RL_END_HI), (long)tr->armed_hi), false;
        if (duty < 0 && p <= tr->armed_lo) return note(rl, now, "motor %u is at its window's end by the %s (%ld): only ►", id, pose_name(tr, RL_END_LO), (long)tr->armed_lo), false;
    }
    if (rl->jog_id && rl->jog_id != id) end_jog(rl); /* one servo at a time */
    if (duty > rl->cfg.duty_cap) duty = rl->cfg.duty_cap;
    if (duty < -rl->cfg.duty_cap) duty = -rl->cfg.duty_cap;
    if (rl->jog_id != id || rl->jog_duty != duty) rl->next_jog_ms = now;
    rl->jog_id = id;
    rl->jog_duty = duty;
    rl->jog_until_ms = now + rl->cfg.hold_timeout_ms;
    return true;
}

void rl_release(rl_t *rl)
{
    end_jog(rl);
}

void rl_pause(rl_t *rl, bool paused)
{
    if (paused && !rl->paused) {
        end_jog(rl);
        /* Whoever has the UART may move a motor this engine cannot see. */
        for (int i = 0; i < RL_MAX_SERVOS; i++) forget_turn(rl, i, rl->tick_ms, "the link was lent to a tunnel or bus scan");
    }
    rl->paused = paused;
    rl->txn = T_NONE;
}

/* ---- transactions ----------------------------------------------------- */

static void send(rl_t *rl, uint32_t now, int txn, int axis, uint8_t id, uint8_t instruction, const uint8_t *params, size_t n)
{
    uint8_t frame[HX_MAX_FRAME];
    size_t len = hx_packet(frame, id, instruction, params, n);
    if (!len) return;
    rl->txn = txn;
    rl->txn_axis = axis;
    rl->txn_at_ms = now;
    rl->sent++;
    rl->write(rl->ctx, frame, len);
}

static void local(rl_t *rl, uint32_t now, int txn, int axis, const uint8_t *params, size_t n)
{
    send(rl, now, txn, axis, HX_BRIDGE_ID, HX_LOCAL, params, n);
}

static uint8_t id_of(const rl_t *rl, int axis)
{
    return (uint8_t)(rl->cfg.first_id + axis);
}

static void finish(rl_t *rl, uint32_t now)
{
    rl->txn = T_NONE;
    rl->idle_at_ms = now;
}

static void gait_observe(rl_t *rl, uint8_t id, uint32_t now);
static bool gait_tick(rl_t *rl, uint32_t now);
static bool arm_tick(rl_t *rl, uint32_t now);

/* The held jog on axis i drives outward at or past the edge of the window it was armed in. */
static bool jog_outward(const rl_t *rl, int i)
{
    const rl_travel_t *t = &rl->servo[i].travel;
    if (!t->armed || rl->jog_id != id_of(rl, i) || !rl->jog_duty) return false;
    int32_t p = rl_position(rl, rl->jog_id);
    return (rl->jog_duty > 0 && p >= t->armed_hi) || (rl->jog_duty < 0 && p <= t->armed_lo);
}

/* ---- turns -------------------------------------------------------------- */

static int plan_axis_of(const rl_t *rl, uint8_t id)
{
    for (int k = 0; rl->gait.have_plan && k < rl->gait.plan.axes; k++)
        if (rl->gait.plan.axis[k].id == id) return k;
    return -1;
}

/* Anything that could hide a turn: the motor's turn is no longer known, and a gait driving it by turns stops. */
static void forget_turn(rl_t *rl, int i, uint32_t now, const char *why)
{
    rl_servo_t *s = &rl->servo[i];
    if (!s->turn_known) return;
    s->turn_known = false;
    uint8_t id = id_of(rl, i);
    note(rl, now, "motor %u's turn is no longer known (%s): confirm it again", id, why);
    int k = plan_axis_of(rl, id);
    if (k >= 0 && rl->gait.plan.axis[k].multi_turn && (rl->gait.phase == RL_GAIT_PREPARE || rl->gait.phase == RL_GAIT_APPROACH || rl->gait.phase == RL_GAIT_PLAYING)) {
        char text[96];
        snprintf(text, sizeof text, "motor %u's turn was lost (%s)", id, why);
        gait_end(rl, text);
    }
}

/* A good reading: count the turn from the last one (nearest turn, as EncoderTurns). */
static void track_turns(rl_t *rl, int i, uint32_t previous_ms, uint32_t now)
{
    rl_servo_t *s = &rl->servo[i];
    int32_t raw = s->t.position_raw;
    if (raw > 4095) {
        forget_turn(rl, i, now, "an invalid encoder reading");
        s->tracked = false;
        return;
    }
    if (!s->tracked) {
        s->counts = raw;
        s->tracked = true;
        return;
    }
    int32_t delta = ((raw - (s->counts & 4095)) % 4096 + 4096 + 2048) % 4096 - 2048;
    if (delta == -2048) forget_turn(rl, i, now, "a half-turn jump between readings");
    else if (before(previous_ms + RL_TURN_GAP_MS, now)) forget_turn(rl, i, now, "too long between readings");
    s->counts += delta;
}

void rl_turn_forget_all(rl_t *rl, const char *why, uint32_t now)
{
    for (int i = 0; i < RL_MAX_SERVOS; i++) forget_turn(rl, i, now, why);
}

int32_t rl_position(const rl_t *rl, uint8_t id)
{
    int i = rl_axis(rl, id);
    if (i < 0) return 0;
    const rl_servo_t *s = &rl->servo[i];
    return s->travel.multi_turn ? s->counts : (int32_t)s->t.position_raw;
}

int rl_turn_candidates(const rl_t *rl, uint8_t id, uint32_t now, int32_t out[2])
{
    int i = rl_axis(rl, id);
    if (i < 0) return 0;
    const rl_servo_t *s = &rl->servo[i];
    const rl_travel_t *t = &s->travel;
    if (!t->known || !t->multi_turn || !s->valid || !before(now, s->at_ms + 500) || s->t.position_raw > 4095) return 0;
    /* The places are judged against the pack's travel: its turn count is the frame. */
    int32_t raw = s->t.position_raw, lo = t->pack_lo - t->margin, hi = t->pack_hi + t->margin;
    int32_t c = raw + 4096 * (int32_t)floor((double)(lo - raw) / 4096.0);
    int n = 0;
    for (; c <= hi && n < 2; c += 4096)
        if (c >= lo) out[n++] = c;
    return n;
}

bool rl_turn_confirm(rl_t *rl, uint8_t id, int32_t counts, uint32_t now)
{
    int i = rl_axis(rl, id);
    if (i < 0) return note(rl, now, "motor %u is not on this bus", id), false;
    rl_servo_t *s = &rl->servo[i];
    if (!s->travel.known || !s->travel.multi_turn) return note(rl, now, "motor %u is not counted in turns", id), false;
    if (rl_gait_active(rl)) return note(rl, now, "stop the gait first"), false;
    if (rl->status_valid && (rl->status.armed_mask >> i & 1)) return note(rl, now, "motor %u is armed: stop it first", id), false;
    int32_t places[2];
    int n = rl_turn_candidates(rl, id, now, places);
    if (!n) return note(rl, now, "motor %u has no fresh reading inside its taught travel", id), false;
    /* The reading may have moved a few counts since the operator looked. */
    for (int c = 0; c < n; c++) {
        if (counts < places[c] - 64 || counts > places[c] + 64) continue;
        s->counts = places[c];
        s->tracked = s->turn_known = true;
        s->turn_lo = s->travel.pack_lo;
        s->turn_hi = s->travel.pack_hi;
        note(rl, now, "motor %u's turn confirmed: %ld counts", id, (long)places[c]);
        return true;
    }
    return note(rl, now, "%ld is not where motor %u's reading allows it to be", (long)counts, id), false;
}

/* ---- taught travel ------------------------------------------------------ */

/* The calibration's name for an encoder end: a reversed motor's lower pose is its high-count end. */
static const char *pose_name(const rl_travel_t *t, int end)
{
    return (end == RL_END_HI) != t->reversed ? "upper pose" : "lower pose";
}

bool rl_travel_changed(const rl_travel_t *t)
{
    return t->known && (t->lo != t->pack_lo || t->hi != t->pack_hi || t->open[0] || t->open[1]);
}

void rl_travel_set_pack(rl_t *rl, uint8_t id, int32_t lo, int32_t hi, bool multi_turn, bool reversed, int32_t margin)
{
    int i = rl_axis(rl, id);
    if (i < 0) return;
    rl_travel_t *t = &rl->servo[i].travel;
    bool armed = t->armed, recovering = t->recovering;
    int32_t alo = t->armed_lo, ahi = t->armed_hi;
    *t = (rl_travel_t){.known = true, .multi_turn = multi_turn, .reversed = reversed, .margin = margin, .pack_lo = lo, .pack_hi = hi, .lo = lo, .hi = hi,
                       .armed = armed, .recovering = recovering, .armed_lo = alo, .armed_hi = ahi};
}

bool rl_travel_restore(rl_t *rl, uint8_t id, int32_t lo, int32_t hi, bool open_lo, bool open_hi)
{
    int i = rl_axis(rl, id);
    if (i < 0 || !rl->servo[i].travel.known || lo >= hi || lo < -RL_TRAVEL_OPEN || hi > RL_TRAVEL_OPEN) return false;
    rl_travel_t *t = &rl->servo[i].travel;
    t->lo = lo;
    t->hi = hi;
    t->open[0] = open_lo;
    t->open[1] = open_hi;
    return true;
}

static rl_travel_t *travel_for_edit(rl_t *rl, uint8_t id, uint32_t now)
{
    int i = rl_axis(rl, id);
    const char *why = i < 0 ? "is not on this bus" : !rl->servo[i].travel.known ? "has no taught travel: store a gait pack"
                    : rl_gait_active(rl) ? "is driven by a playing gait: stop it first" : NULL;
    if (why) {
        note(rl, now, "motor %u %s", id, why);
        return NULL;
    }
    return &rl->servo[i].travel;
}

bool rl_travel_teach(rl_t *rl, uint8_t id, int end, uint32_t now)
{
    rl_travel_t *t = travel_for_edit(rl, id, now);
    if (!t || (end != RL_END_LO && end != RL_END_HI)) return false;
    const rl_servo_t *s = &rl->servo[rl_axis(rl, id)];
    if (!s->valid || !before(now, s->at_ms + 250)) return note(rl, now, "motor %u has no fresh reading", id), false;
    if (t->multi_turn && !s->turn_known) return note(rl, now, "motor %u's turn is not confirmed: pick where the leg is first", id), false;
    int32_t p = rl_position(rl, id);
    int32_t lo = end == RL_END_LO ? p : t->lo, hi = end == RL_END_HI ? p : t->hi;
    bool other_open = t->open[end == RL_END_LO ? RL_END_HI : RL_END_LO];
    if (!other_open && hi - lo < RL_TRAVEL_MIN)
        return note(rl, now, "motor %u: its lower and upper poses must be at least %d counts apart (%ld..%ld)", id, RL_TRAVEL_MIN, (long)lo, (long)hi), false;
    if (t->multi_turn && !other_open && (int64_t)hi - lo + 2 * t->margin >= 2 * 4096)
        return note(rl, now, "motor %u: %ld..%ld spans two turns, so a reading would allow three places", id, (long)lo, (long)hi), false;
    if (end == RL_END_LO) t->lo = p;
    else t->hi = p;
    t->open[end] = false;
    rl->travel_dirty = true;
    note(rl, now, "motor %u's %s set at %ld%s", id, pose_name(t, end), (long)p,
         t->armed ? "; its armed window changes when it is armed again" : "");
    return true;
}

bool rl_travel_open(rl_t *rl, uint8_t id, int end, uint32_t now)
{
    rl_travel_t *t = travel_for_edit(rl, id, now);
    if (!t || (end != RL_END_LO && end != RL_END_HI)) return false;
    if (t->open[!end]) return note(rl, now, "motor %u's %s is open: set it before opening this one", id, pose_name(t, !end)), false;
    t->open[end] = true;
    rl->travel_dirty = true;
    note(rl, now, "motor %u's %s is open: jog it to the new place while watching, then set it there", id, pose_name(t, end));
    return true;
}

bool rl_travel_revert(rl_t *rl, uint8_t id, uint32_t now)
{
    rl_travel_t *t = travel_for_edit(rl, id, now);
    if (!t) return false;
    t->lo = t->pack_lo;
    t->hi = t->pack_hi;
    t->open[0] = t->open[1] = false;
    rl->travel_dirty = true;
    note(rl, now, "motor %u's travel is the pack's again (%ld..%ld)", id, (long)t->lo, (long)t->hi);
    return true;
}

static void on_status(rl_t *rl, const uint8_t *params, size_t n, uint32_t now)
{
    hx_status_t s;
    if (!hx_status_decode(params, n, &s)) {
        rl->unexpected++;
        return;
    }
    uint16_t newly_armed = s.armed_mask & ~(rl->status_valid ? rl->status.armed_mask : 0);
    rl->status = s;
    rl->status_valid = true;
    rl->status_at_ms = now;
    if ((rl->gait.phase == RL_GAIT_APPROACH || rl->gait.phase == RL_GAIT_PLAYING) && s.latched) {
        char why[64];
        snprintf(why, sizeof why, "the FPGA latched (%s)", hx_reason_name(s.reason));
        gait_end(rl, why);
    }
    for (int i = 0; i < RL_MAX_SERVOS; i++)
        if (newly_armed >> i & 1 && before(now + rl->cfg.fast_poll_ms, rl->next_poll_ms[i])) rl->next_poll_ms[i] = now;
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (s.armed_mask >> i & 1) continue;
        /* Disarmed (a stop torque-offs every servo): torque must be enabled again, and nothing may still be held. */
        rl->servo[i].torque_on = false;
        rl->servo[i].travel.armed = rl->servo[i].travel.recovering = false;
        if (rl->jog_id == id_of(rl, i)) {
            rl->jog_id = 0;
            rl->jog_duty = 0;
        }
    }
}

static void on_frame(rl_t *rl, const uint8_t *f, size_t len, uint32_t now)
{
    uint8_t id = f[2], error = f[4];
    const uint8_t *params = f + 5;
    size_t n = len - 6;
    rl->replies++;
    if (id == HX_BRIDGE_ID) {
        /* The supervisor's status: the answer to a local command, or the receipt of a sync write. */
        on_status(rl, params, n, now);
        if (rl->txn == T_G_LOCAL) {
            rl->gait.done = rl->gait.ok = true;
            finish(rl, now);
        } else if (rl->txn == T_J_LOCAL) {
            rl->arm_job.done = rl->arm_job.ok = true;
            finish(rl, now);
        } else if (rl->txn == T_STOP || rl->txn == T_STATUS || rl->txn == T_ARM || rl->txn == T_DISARM || rl->txn == T_HEARTBEAT) {
            if (rl->txn == T_ARM && rl->status_valid && !(rl->status.armed_mask >> rl->txn_axis & 1))
                note(rl, now, "the FPGA did not arm servo %u (%s)", id_of(rl, rl->txn_axis), rl->status.latched ? hx_reason_name(rl->status.reason) : "telemetry not fresh");
            finish(rl, now);
        }
        return;
    }
    int axis = rl_axis(rl, id);
    if (axis < 0 || rl->txn == T_NONE || axis != rl->txn_axis) {
        rl->unexpected++;
        return;
    }
    rl_servo_t *s = &rl->servo[axis];
    switch (rl->txn) {
    case T_TELEMETRY:
        if (hx_telemetry_decode(params, n, &s->t)) {
            uint32_t previous = s->at_ms;
            if (!s->valid) s->tracked = false;
            s->valid = true;
            s->at_ms = now;
            /* The reply's own error byte is the device status the FPGA also judges. */
            s->t.status = error ? error : s->t.status;
            track_turns(rl, axis, previous, now);
            if (jog_outward(rl, axis)) {
                note(rl, now, "motor %u reached its window's end by the %s: stopped", id, pose_name(&rl->servo[axis].travel, rl->jog_duty > 0 ? RL_END_HI : RL_END_LO));
                end_jog(rl);
            }
            gait_observe(rl, id, now);
        } else {
            rl->unexpected++;
            forget_turn(rl, axis, now, "a corrupt reading");
        }
        break;
    case T_G_WRITE:
    case T_GOAL:
        rl->gait.done = rl->gait.ok = true;
        break;
    case T_G_READ:
        if (n >= 1 && n <= 2) {
            memcpy(rl->gait.read, params, n);
            rl->gait.done = rl->gait.ok = true;
        } else rl->unexpected++;
        break;
    case T_J_WRITE:
        rl->arm_job.done = rl->arm_job.ok = true;
        break;
    case T_J_READ:
        if (n >= 1 && n <= 2) {
            memcpy(rl->arm_job.read, params, n);
            rl->arm_job.done = rl->arm_job.ok = true;
        } else rl->unexpected++;
        break;
    case T_MODE:
        if (n == 1) s->mode = params[0];
        else rl->unexpected++;
        break;
    case T_TORQUE:
        s->torque_on = true;
        break;
    case T_ZERO:
        rl->zero_pending[axis] = false;
        break;
    case T_PWM:
        break;
    default:
        rl->unexpected++;
        return;
    }
    finish(rl, now);
}

void rl_rx(rl_t *rl, const uint8_t *bytes, size_t n, uint32_t now)
{
    for (size_t i = 0; i < n; i++) {
        size_t len = hx_framer_push(&rl->framer, bytes[i]);
        if (!len) continue;
        uint8_t frame[HX_MAX_FRAME];
        memcpy(frame, rl->framer.buf, len);
        rl->framer.len = 0;
        if (!rl->paused) on_frame(rl, frame, len, now);
    }
}

static void timed_out(rl_t *rl, uint32_t now)
{
    rl->timeouts++;
    switch (rl->txn) {
    case T_G_LOCAL:
    case T_G_WRITE:
    case T_G_READ:
    case T_GOAL:
        rl->gait.done = true;
        rl->gait.ok = false;
        if (rl->txn == T_GOAL) rl->refused++;
        break;
    case T_J_LOCAL:
    case T_J_WRITE:
    case T_J_READ:
        rl->arm_job.done = true;
        rl->arm_job.ok = false;
        break;
    case T_TORQUE:
    case T_PWM:
        /* The supervisor drops a write it refuses without a word. */
        rl->refused++;
        note(rl, now, "servo %u did not acknowledge a drive command (refused by the FPGA, or not answering)", id_of(rl, rl->txn_axis));
        break;
    case T_MODE:
        note(rl, now, "servo %u did not report its control mode", id_of(rl, rl->txn_axis));
        break;
    case T_TELEMETRY:
        forget_turn(rl, rl->txn_axis, now, "a missed reading");
        break;
    default:
        break;
    }
    finish(rl, now);
}

void rl_tick(rl_t *rl, uint32_t now)
{
    rl->tick_ms = now;
    if (rl->paused) return;
    if (rl->txn != T_NONE) {
        if (before(now, rl->txn_at_ms + rl->cfg.reply_timeout_ms)) return;
        timed_out(rl, now);
    }
    if (before(now, rl->idle_at_ms + rl->cfg.gap_ms)) return;

    bool alive = session_alive(rl, now);
    if (rl->session && !alive) {
        note(rl, now, "the operator session went quiet: stopping");
        rl_session_end(rl);
    }
    if (rl->jog_id && !jog_active(rl, now)) end_jog(rl);

    /* 1. Stop outranks everything. */
    if (rl->stop_req) {
        rl->stop_req = false;
        uint8_t p[] = {HX_OP_STOP};
        local(rl, now, T_STOP, 0, p, 1);
        return;
    }
    /* 2. A released jog is written to zero until the servo acknowledges it. */
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (!rl->zero_pending[i]) continue;
        if (rl->status_valid && !(rl->status.armed_mask >> i & 1)) {
            rl->zero_pending[i] = false; /* disarmed: the FPGA has already zeroed and torque-offed it */
            continue;
        }
        uint8_t p[] = {HX_REG_PWM, 0, 0};
        send(rl, now, T_ZERO, i, id_of(rl, i), HX_WRITE, p, 3);
        return;
    }
    /* 3. The lease: only for a live operator. */
    if (alive && rl->status_valid && rl->status.armed_mask && !before(now, rl->next_heartbeat_ms)) {
        rl->next_heartbeat_ms = now + rl->cfg.heartbeat_ms;
        uint8_t p[] = {HX_OP_HEARTBEAT_MASK, (uint8_t)rl->status.armed_mask, (uint8_t)(rl->status.armed_mask >> 8)};
        local(rl, now, T_HEARTBEAT, 0, p, 3);
        return;
    }
    /* 4. Disarm, then arm. */
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (rl->disarm_req >> i & 1) {
            rl->disarm_req &= (uint16_t)~(1u << i);
            uint8_t p[] = {HX_OP_DISARM, id_of(rl, i)};
            local(rl, now, T_DISARM, i, p, 2);
            return;
        }
    }
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (!(rl->arm_req >> i & 1)) continue;
        if (!alive) { rl->arm_req &= (uint16_t)~(1u << i); continue; }
        if (rl->status_valid && rl->status.version >= 5) {
            /* The calibrated arm is a sequence of its own (arm_tick). */
            if (rl->arm_job.axis >= 0) break;
            rl->arm_req &= (uint16_t)~(1u << i);
            rl->arm_job.axis = i;
            rl->arm_job.step = 0;
            rl->arm_job.waiting = false;
            rl->arm_job.clock_ms = now;
            rl->next_poll_ms[i] = now;
            break;
        }
        /* Only with telemetry the FPGA will accept as fresh; a due poll goes first. */
        if (!rl->servo[i].valid || before(rl->servo[i].at_ms + 100, now)) {
            rl->next_poll_ms[i] = now;
            break;
        }
        rl->arm_req &= (uint16_t)~(1u << i);
        rl->servo[i].mode = -1; /* read again before it may be driven */
        rl->next_heartbeat_ms = now + rl->cfg.heartbeat_ms;
        rl->next_poll_ms[i] = now + rl->cfg.fast_poll_ms;
        uint8_t p[] = {HX_OP_ARM, id_of(rl, i)};
        local(rl, now, T_ARM, i, p, 2);
        return;
    }
    /* 5. Telemetry that is due comes before driving: stale telemetry trips the FPGA. */
    for (unsigned k = 0; k < RL_MAX_SERVOS; k++) {
        int i = (int)((rl->poll_cursor + k) % RL_MAX_SERVOS);
        if (!(rl->cfg.present_mask >> i & 1) || before(now, rl->next_poll_ms[i])) continue;
        bool armed = rl->status_valid && (rl->status.armed_mask >> i & 1);
        uint32_t period = armed || (rl->arm_req >> i & 1) ? rl->cfg.fast_poll_ms : rl->cfg.slow_poll_ms;
        if (rl->servo[i].turn_known && period > RL_TURN_POLL_MS) period = RL_TURN_POLL_MS;
        rl->next_poll_ms[i] = now + period;
        rl->poll_cursor = (unsigned)i + 1;
        uint8_t p[] = {HX_REG_TELEMETRY, HX_TELEMETRY_LEN};
        send(rl, now, T_TELEMETRY, i, id_of(rl, i), HX_READ, p, 2);
        return;
    }
    /* 6. Arming on the calibration profile, then the gait: preparation, goals, stop verification. */
    if (arm_tick(rl, now)) return;
    if (gait_tick(rl, now)) return;
    /* 7. The held jog. */
    if (jog_active(rl, now)) {
        int i = rl_axis(rl, rl->jog_id);
        rl_servo_t *s = &rl->servo[i];
        if (!rl->status_valid || rl->status.latched || !(rl->status.armed_mask >> i & 1)) {
            end_jog(rl);
        } else if (s->mode < 0) {
            uint8_t p[] = {HX_REG_MODE, 1};
            send(rl, now, T_MODE, i, rl->jog_id, HX_READ, p, 2);
            return;
        } else if (s->mode != 2) {
            note(rl, now, "servo %u is in control mode %d, not open-loop PWM (2): not driven", rl->jog_id, s->mode);
            end_jog(rl);
            rl->zero_pending[i] = false;
        } else if (!s->torque_on) {
            uint8_t p[] = {HX_REG_TORQUE, 1};
            send(rl, now, T_TORQUE, i, rl->jog_id, HX_WRITE, p, 2);
            return;
        } else if (jog_outward(rl, i)) {
            /* The FPGA only refuses the next outward write; the drive already given keeps
             * running. So the engine zeroes it at the window's edge itself, as the panel does. */
            note(rl, now, "motor %u reached its window's end by the %s: stopped", rl->jog_id, pose_name(&rl->servo[i].travel, rl->jog_duty > 0 ? RL_END_HI : RL_END_LO));
            end_jog(rl);
        } else if (!before(now, rl->next_jog_ms)) {
            rl->next_jog_ms = now + rl->cfg.jog_ms;
            uint8_t p[3] = {HX_REG_PWM};
            hx_pwm_bytes(rl->jog_duty, p + 1);
            send(rl, now, T_PWM, i, rl->jog_id, HX_WRITE, p, 3);
            return;
        }
    }
    /* 8. Status. */
    if (!before(now, rl->next_status_ms)) {
        rl->next_status_ms = now + rl->cfg.status_ms;
        uint8_t p[] = {HX_OP_STATUS};
        local(rl, now, T_STATUS, 0, p, 1);
    }
}

/* ---- gait playback ------------------------------------------------------ */
/*
 * The Rust panel's gait, step for step (calibration_serial.rs prepare_drive
 * and controlled_motion_multi, hardware/calibration/gait.rs run_gait): per
 * motor STOP (first only), fresh telemetry, the taught window anchored at the
 * live reading (with its confirmed turns for a multi-turn motor), arm, torque
 * off, PWM zero, re-arm, the plan's control mode (unlocking only to change
 * it), park (position goal on the current pose, or speed zero), heartbeat,
 * torque on. Then the governor approaches the first pose with the gait clock
 * held, and the clock starts once every motor has arrived. Single-turn motors
 * get servo position goals (servo_command ServoPosition); a multi-turn motor
 * gets servo speed with a position trim (ServoSpeed), since position mode
 * cannot leave one turn. The engine's own telemetry poll and lease heartbeat
 * keep the FPGA's deadlines.
 */
enum { P_STOP, P_FRESH_W, P_WINDOW, P_FRESH_A, P_ARM, P_TORQUE_OFF, P_PWM_ZERO, P_FRESH_R, P_REARM,
       P_READ_MODE, P_UNLOCK, P_SET_MODE, P_LOCK, P_VERIFY_MODE, P_FRESH_P, P_PARK, P_HEARTBEAT, P_TORQUE_ON, P_DONE };
static const char *const prepare_names[] = {"STOP", "telemetry", "window", "telemetry", "arm", "torque off", "PWM zero", "telemetry", "re-arm",
    "mode read", "unlock", "control mode", "lock", "mode check", "telemetry", "park", "heartbeat", "torque on", "done"};

const char *rl_gait_phase_name(int phase)
{
    static const char *const names[] = {"idle", "preparing", "approaching the first pose", "playing", "stopping"};
    return phase >= 0 && phase <= RL_GAIT_STOPPING ? names[phase] : "?";
}

bool rl_gait_active(const rl_t *rl)
{
    return rl->gait.phase != RL_GAIT_IDLE;
}

static float clampf(float v, float lo, float hi)
{
    return v < lo ? lo : v > hi ? hi : v;
}

void rl_gait_set_log(rl_t *rl, rl_gait_sample_t *buf, uint32_t cap)
{
    rl->gait.log = buf;
    rl->gait.log_cap = buf ? cap : 0;
    rl->gait.log_count = 0;
}

bool rl_gait_load(rl_t *rl, const rl_plan_t *plan, uint32_t now)
{
    if (rl_gait_active(rl)) return note(rl, now, "stop the gait before loading another"), false;
    for (int k = 0; k < plan->axes; k++)
        if (rl_axis(rl, plan->axis[k].id) < 0) return note(rl, now, "the gait drives motor %u, which is not on this bus", plan->axis[k].id), false;
    rl->gait.plan = *plan;
    rl->gait.have_plan = true;
    return true;
}

bool rl_gait_start(rl_t *rl, float speed_scale, uint32_t now)
{
    rl_gait_t *g = &rl->gait;
    if (rl_gait_active(rl)) return note(rl, now, "a gait is already %s", rl_gait_phase_name(g->phase)), false;
    if (!g->have_plan) return note(rl, now, "no gait loaded"), false;
    if (!session_alive(rl, now)) return note(rl, now, "no live operator session"), false;
    if (!rl_link_ok(rl, now)) return note(rl, now, "the FPGA is not answering"), false;
    if (rl->status.version < 5) return note(rl, now, "the FPGA's profile has no taught windows: load the calibration image"), false;
    if (g->plan.axes > 1 && rl->status.version < 6) return note(rl, now, "this calibration profile arms one motor at a time"), false;
    for (int k = 0; k < g->plan.axes; k++) {
        const rl_plan_axis_t *a = &g->plan.axis[k];
        int i = rl_axis(rl, a->id);
        if (i < 0 || !rl->servo[i].valid) return note(rl, now, "motor %u has not answered a telemetry read", a->id), false;
        const rl_servo_t *s = &rl->servo[i];
        if (s->travel.known && rl_travel_changed(&s->travel))
            return note(rl, now, "motor %u's poses were changed on this page: promote them (leg_gait_pack --pull-calibration), then store the rebuilt pack", a->id), false;
        if (s->travel.known && (s->travel.pack_lo != a->window_lo || s->travel.pack_hi != a->window_hi))
            return note(rl, now, "the gait's window for motor %u disagrees with the stored pack's travel", a->id), false;
        if (!a->multi_turn) continue;
        if (!s->turn_known || s->turn_lo != a->window_lo || s->turn_hi != a->window_hi)
            return note(rl, now, "motor %u's turn is not confirmed: pick where the leg is first", a->id), false;
        if (s->counts < a->window_lo || s->counts > a->window_hi)
            return note(rl, now, "motor %u is at %ld, outside its taught travel %ld..%ld: bring it inside with the calibration panel", a->id,
                        (long)s->counts, (long)a->window_lo, (long)a->window_hi), false;
    }
    end_jog(rl);
    rl->arm_req = rl->disarm_req = 0;
    g->phase = RL_GAIT_PREPARE;
    g->axis = 0;
    g->step = P_STOP;
    g->waiting = g->done = g->ok = false;
    g->goal_pending = 0;
    g->t = 0.f;
    g->playing = true;
    g->speed_scale = clampf(speed_scale > 0.f ? speed_scale : 1.f, 0.05f, 1.f);
    g->phase_ms = g->clock_ms = now;
    g->stop_verified = false;
    g->result[0] = 0;
    g->log_count = 0;
    note(rl, now, "gait: preparing motor %u", g->plan.axis[0].id);
    return true;
}

void rl_gait_control(rl_t *rl, bool playing, float speed_scale)
{
    rl->gait.playing = playing;
    if (speed_scale > 0.f) rl->gait.speed_scale = clampf(speed_scale, 0.05f, 1.f);
}

/* Any end goes through a STOP and its verification. */
static void gait_end(rl_t *rl, const char *why)
{
    rl_gait_t *g = &rl->gait;
    if (g->phase == RL_GAIT_IDLE || g->phase == RL_GAIT_STOPPING) return;
    snprintf(g->result, sizeof g->result, "%s", why);
    g->phase = RL_GAIT_STOPPING;
    g->phase_ms = 0; /* stamped on the first stopping tick */
    g->axis = 0;
    g->step = 0;
    g->waiting = false;
    g->goal_pending = 0;
    for (int k = 0; k < RL_GAIT_MAX_AXES; k++) {
        g->stationary[k] = 0;
        g->last_position[k] = -1;
    }
    rl->stop_req = true;
}

void rl_gait_stop(rl_t *rl, const char *why)
{
    gait_end(rl, why && *why ? why : "stopped by the operator");
}

static void gait_fail(rl_t *rl, uint32_t now, const char *fmt, ...)
{
    char why[128];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(why, sizeof why, fmt, ap);
    va_end(ap);
    note(rl, now, "gait: %s", why);
    gait_end(rl, why);
}

/* Where plan axis a's motor is, in the plan's counts: the reading, or for a multi-turn motor its counted turns. */
static int32_t axis_position(const rl_t *rl, const rl_plan_axis_t *a)
{
    const rl_servo_t *s = &rl->servo[rl_axis(rl, a->id)];
    return a->multi_turn ? s->counts : (int32_t)s->t.position_raw;
}

static void gait_observe(rl_t *rl, uint8_t id, uint32_t now)
{
    rl_gait_t *g = &rl->gait;
    if (g->phase == RL_GAIT_IDLE || g->phase == RL_GAIT_STOPPING) return;
    int k = 0;
    while (k < g->plan.axes && g->plan.axis[k].id != id) k++;
    if (k == g->plan.axes) return;
    const hx_telemetry_t *t = &rl->servo[rl_axis(rl, id)].t;
    /* The panel's commissioning limits (calibration_serial.rs healthy()). */
    if (t->position_raw > 4095 || t->temperature_c >= 55 || t->voltage_raw < 90 || t->voltage_raw > 126) {
        gait_fail(rl, now, "motor %u outside commissioning limits: %u C, %.1f V, encoder %u", id, t->temperature_c, t->voltage_raw / 10.0, t->position_raw);
        return;
    }
    if (g->phase != RL_GAIT_APPROACH && g->phase != RL_GAIT_PLAYING) return;
    if (!g->log_cap) return;
    rl_gait_sample_t *s = &g->log[g->log_count++ % g->log_cap];
    *s = (rl_gait_sample_t){.ms = now, .gait_t = g->t, .command = g->goal[k], .desired = g->desired[k], .actual = axis_position(rl, &g->plan.axis[k]), .id = id};
}

static bool fresh_since(const rl_t *rl, int i, uint32_t since, uint32_t now)
{
    const rl_servo_t *s = &rl->servo[i];
    return s->valid && !before(s->at_ms, since) && before(now, s->at_ms + 100);
}

static void gait_local(rl_t *rl, uint32_t now, const uint8_t *p, size_t n)
{
    rl->gait.waiting = true;
    rl->gait.done = rl->gait.ok = false;
    local(rl, now, T_G_LOCAL, 0, p, n);
}

static void gait_servo(rl_t *rl, uint32_t now, int txn, int i, uint8_t instruction, const uint8_t *p, size_t n)
{
    rl->gait.waiting = true;
    rl->gait.done = rl->gait.ok = false;
    send(rl, now, txn, i, id_of(rl, i), instruction, p, n);
}

static void put_i32(uint8_t *p, int32_t v)
{
    p[0] = (uint8_t)v;
    p[1] = (uint8_t)(v >> 8);
    p[2] = (uint8_t)(v >> 16);
    p[3] = (uint8_t)(v >> 24);
}

static bool gait_prepare(rl_t *rl, uint32_t now)
{
    rl_gait_t *g = &rl->gait;
    for (;;) {
        const rl_plan_axis_t *a = &g->plan.axis[g->axis];
        int i = rl_axis(rl, a->id);
        rl_servo_t *s = &rl->servo[i];
        if (g->waiting) {
            g->waiting = false;
            if (!g->ok) return gait_fail(rl, now, "motor %u: no reply to %s", a->id, prepare_names[g->step]), false;
            int next = g->step + 1;
            if ((g->step == P_ARM || g->step == P_REARM) && !(rl->status.armed_mask >> i & 1))
                return gait_fail(rl, now, "the FPGA did not arm motor %u (%s)", a->id,
                                 rl->status.latched ? hx_reason_name(rl->status.reason) : "outside its taught window, or telemetry not fresh"), false;
            if (g->step == P_READ_MODE && g->read[0] == a->drive) next = P_LOCK;
            if (g->step == P_VERIFY_MODE && g->read[0] != a->drive)
                return gait_fail(rl, now, "motor %u did not accept %s mode; it may need a power cycle", a->id, a->drive == RL_DRIVE_SPEED ? "speed" : "position"), false;
            g->step = next;
            g->clock_ms = now;
        }
        switch (g->step) {
        case P_STOP:
            if (g->axis > 0) { g->step = P_FRESH_W; continue; }
            { uint8_t p[] = {HX_OP_STOP}; gait_local(rl, now, p, 1); }
            return true;
        case P_FRESH_W:
        case P_FRESH_A:
        case P_FRESH_R:
        case P_FRESH_P:
            if (fresh_since(rl, i, g->clock_ms, now)) { g->step++; continue; }
            if (before(g->clock_ms + 1000, now)) return gait_fail(rl, now, "motor %u: no telemetry", a->id), false;
            rl->next_poll_ms[i] = now;
            return false;
        case P_WINDOW: {
            if (a->multi_turn && !s->turn_known) return gait_fail(rl, now, "motor %u's turn is not known", a->id), false;
            uint8_t p[14] = {7, a->id};
            /* The FPGA takes the anchor as its own turn count; it must match the reading modulo a turn. */
            put_i32(p + 2, axis_position(rl, a));
            put_i32(p + 6, a->window_lo);
            put_i32(p + 10, a->window_hi);
            gait_local(rl, now, p, sizeof p);
            return true;
        }
        case P_ARM:
        case P_REARM: { uint8_t p[] = {HX_OP_ARM, a->id}; gait_local(rl, now, p, 2); return true; }
        case P_TORQUE_OFF: { uint8_t p[] = {HX_REG_TORQUE, 0}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 2); return true; }
        case P_PWM_ZERO: { uint8_t p[] = {HX_REG_PWM, 0, 0}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 3); return true; }
        case P_READ_MODE:
        case P_VERIFY_MODE: { uint8_t p[] = {HX_REG_MODE, 1}; gait_servo(rl, now, T_G_READ, i, HX_READ, p, 2); return true; }
        case P_UNLOCK: { uint8_t p[] = {0x37, 0}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 2); return true; }
        case P_SET_MODE: { uint8_t p[] = {HX_REG_MODE, a->drive}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 2); return true; }
        case P_LOCK: { uint8_t p[] = {0x37, 1}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 2); return true; }
        case P_PARK:
            if (a->drive == RL_DRIVE_SPEED) {
                uint8_t p[] = {0x2E, 0, 0};
                gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, sizeof p);
            } else {
                uint8_t p[] = {0x2A, (uint8_t)s->t.position_raw, (uint8_t)(s->t.position_raw >> 8), 0, 0, 0x90, 0x01};
                gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, sizeof p);
            }
            return true;
        case P_HEARTBEAT: { uint8_t p[] = {HX_OP_HEARTBEAT, a->id}; gait_local(rl, now, p, 2); return true; }
        case P_TORQUE_ON: { uint8_t p[] = {HX_REG_TORQUE, 1}; gait_servo(rl, now, T_G_WRITE, i, HX_WRITE, p, 2); return true; }
        default: /* P_DONE */
            if (g->axis + 1 < g->plan.axes) {
                g->axis++;
                g->step = P_STOP;
                g->clock_ms = now;
                note(rl, now, "gait: preparing motor %u", g->plan.axis[g->axis].id);
                continue;
            }
            for (int k = 0; k < g->plan.axes; k++) {
                float x = (float)axis_position(rl, &g->plan.axis[k]);
                g->gov[k] = (rl_gov_t){x, 0.f};
                g->goal[k] = g->desired[k] = x;
            }
            g->phase = RL_GAIT_APPROACH;
            g->phase_ms = g->clock_ms = g->next_goal_ms = now;
            note(rl, now, "gait: approaching the first pose");
            return false;
        }
    }
}

static bool gait_drive(rl_t *rl, uint32_t now)
{
    rl_gait_t *g = &rl->gait;
    const rl_plan_t *p = &g->plan;
    if (g->waiting) {
        g->waiting = false;
        if (!g->ok) return gait_fail(rl, now, "motor %u did not acknowledge a goal (refused by the FPGA, or not answering)", p->axis[g->axis].id), false;
    }
    if (!g->goal_pending && !before(now, g->next_goal_ms)) {
        float dt = clampf((float)(now - g->clock_ms) / 1000.f, 0.001f, 0.2f);
        g->clock_ms = now;
        g->next_goal_ms = now + rl->cfg.gait_period_ms;
        bool started = g->phase == RL_GAIT_PLAYING;
        if (started && g->playing) g->t += dt * g->speed_scale;
        bool arrived = true;
        for (int k = 0; k < p->axes; k++) {
            const rl_plan_axis_t *a = &p->axis[k];
            g->desired[k] = rl_plan_desired(p, k, started ? g->t : 0.f);
            rl_gov_step(a, &g->gov[k], g->desired[k], dt);
            g->goal[k] = clampf(g->gov[k].x, (float)a->window_lo + 6.f, (float)a->window_hi - 6.f);
            g->goal_v[k] = g->gov[k].v;
            float actual = (float)axis_position(rl, a);
            /* A speed-driven motor rests anywhere within its hold tolerance of the goal. */
            float tolerance = a->drive == RL_DRIVE_SPEED ? fmaxf(rl->cfg.gait_tolerance_counts, a->hold_tolerance_counts + 4.f) : rl->cfg.gait_tolerance_counts;
            if (fabsf(g->desired[k] - g->gov[k].x) >= 2.f || fabsf(actual - g->goal[k]) >= tolerance) arrived = false;
        }
        if (!started && arrived) {
            g->phase = RL_GAIT_PLAYING;
            g->phase_ms = now;
            g->t = 0.f;
            note(rl, now, "gait: playing");
        } else if (!started && before(g->phase_ms + rl->cfg.gait_approach_ms, now)) {
            return gait_fail(rl, now, "the leg did not reach the gait's first pose within %u s", (unsigned)(rl->cfg.gait_approach_ms / 1000)), false;
        }
        g->goal_pending = (uint8_t)((1u << p->axes) - 1);
    }
    for (int k = 0; k < p->axes; k++) {
        if (!(g->goal_pending >> k & 1)) continue;
        g->goal_pending &= (uint8_t)~(1u << k);
        const rl_plan_axis_t *a = &p->axis[k];
        g->axis = k;
        if (a->drive == RL_DRIVE_SPEED) {
            uint8_t bytes[3];
            rl_speed_goal(g->goal[k], g->goal_v[k], axis_position(rl, a), a->window_lo, a->window_hi, a->hold_tolerance_counts, bytes);
            gait_servo(rl, now, T_GOAL, rl_axis(rl, a->id), HX_WRITE, bytes, sizeof bytes);
        } else {
            uint8_t bytes[7];
            rl_position_goal(g->goal[k], g->goal_v[k], a->window_lo + 6, a->window_hi - 6, bytes);
            gait_servo(rl, now, T_GOAL, rl_axis(rl, a->id), HX_WRITE, bytes, sizeof bytes);
        }
        return true;
    }
    return false;
}

/* After the STOP: torque off, PWM zero and two stationary readings per motor. */
static bool gait_verify(rl_t *rl, uint32_t now)
{
    rl_gait_t *g = &rl->gait;
    const rl_plan_t *p = &g->plan;
    if (rl->stop_req) return false; /* the STOP itself goes first */
    if (!g->phase_ms) g->phase_ms = g->verify_from_ms = now;
    bool all = true;
    for (int k = 0; k < p->axes; k++) all = all && g->stationary[k] >= 2;
    if (all || before(g->phase_ms + 1500, now)) {
        g->stop_verified = all;
        /* A motor that may still be turning cannot keep its count. */
        for (int k = 0; !all && k < p->axes; k++) forget_turn(rl, rl_axis(rl, p->axis[k].id), now, "its stop was not verified");
        size_t n = strlen(g->result);
        snprintf(g->result + n, sizeof g->result - n, all ? "; torque-off verified" : "; torque-off NOT verified: cut motor power");
        g->phase = RL_GAIT_IDLE;
        note(rl, now, "gait ended: %s", g->result);
        return false;
    }
    int k = g->axis, i = rl_axis(rl, p->axis[k].id);
    bool missed = false;
    if (g->waiting) {
        g->waiting = false;
        if (!g->ok) { missed = true; g->step = 3; } /* a missed reading restarts this motor's count */
        else if (g->step == 1) { g->verify_torque = g->read[0]; g->step = 2; }
        else if (g->step == 2) { g->verify_pwm = (uint16_t)(g->read[0] | g->read[1] << 8); g->step = 3; }
    }
    switch (g->step) {
    case 0:
        if (fresh_since(rl, i, g->verify_from_ms, now)) { g->step = 1; return gait_verify(rl, now); }
        rl->next_poll_ms[i] = now;
        return false;
    case 1: { uint8_t q[] = {HX_REG_TORQUE, 1}; gait_servo(rl, now, T_G_READ, i, HX_READ, q, 2); return true; }
    case 2: { uint8_t q[] = {HX_REG_PWM, 2}; gait_servo(rl, now, T_G_READ, i, HX_READ, q, 2); return true; }
    default: {
        const hx_telemetry_t *t = &rl->servo[i].t;
        int32_t pos = t->position_raw;
        bool still = !missed && g->verify_torque == 0 && g->verify_pwm == 0 && t->speed_counts_s == 0 && pos == g->last_position[k];
        g->stationary[k] = still ? g->stationary[k] + 1 : 0;
        g->last_position[k] = pos;
        g->step = 0;
        g->axis = (k + 1) % p->axes;
        if (g->axis == 0) g->verify_from_ms = now + 15;
        return false;
    }
    }
}

static bool gait_tick(rl_t *rl, uint32_t now)
{
    switch (rl->gait.phase) {
    case RL_GAIT_PREPARE: return gait_prepare(rl, now);
    case RL_GAIT_APPROACH:
    case RL_GAIT_PLAYING: return gait_drive(rl, now);
    case RL_GAIT_STOPPING: return gait_verify(rl, now);
    default: return false;
    }
}

/* ---- arming on the calibration profile ---------------------------------- */
/*
 * The panel's arm_with and prepare_drive for hold-to-move: fresh telemetry,
 * the taught window anchored at the reading (stretched to the reading when
 * the motor is outside it: recovery), fresh telemetry, ARM, PWM zero, then
 * open-loop PWM mode, unlocking the mode register only to change it.
 */
enum { J_FRESH, J_WINDOW, J_FRESH_A, J_ARM, J_PWM_ZERO, J_READ_MODE, J_UNLOCK, J_SET_MODE, J_LOCK, J_VERIFY, J_DONE };
static const char *const job_names[] = {"telemetry", "window", "telemetry", "arm", "PWM zero", "mode read", "unlock", "PWM mode", "lock", "mode check", "done"};

static void job_fail(rl_t *rl, uint32_t now, bool disarm, const char *fmt, ...)
{
    char why[128];
    va_list ap;
    va_start(ap, fmt);
    vsnprintf(why, sizeof why, fmt, ap);
    va_end(ap);
    int i = rl->arm_job.axis;
    note(rl, now, "%s", why);
    rl->arm_job.axis = -1;
    if (disarm && i >= 0) rl->disarm_req |= (uint16_t)(1u << i);
}

static void job_local(rl_t *rl, uint32_t now, const uint8_t *p, size_t n)
{
    rl->arm_job.waiting = true;
    rl->arm_job.done = rl->arm_job.ok = false;
    local(rl, now, T_J_LOCAL, 0, p, n);
}

static void job_servo(rl_t *rl, uint32_t now, int txn, int i, uint8_t instruction, const uint8_t *p, size_t n)
{
    rl->arm_job.waiting = true;
    rl->arm_job.done = rl->arm_job.ok = false;
    send(rl, now, txn, i, id_of(rl, i), instruction, p, n);
}

static bool arm_tick(rl_t *rl, uint32_t now)
{
    int i = rl->arm_job.axis;
    if (i < 0) return false;
    uint8_t id = id_of(rl, i);
    rl_servo_t *s = &rl->servo[i];
    rl_travel_t *t = &s->travel;
    if (!session_alive(rl, now)) return job_fail(rl, now, false, "arming motor %u stopped: the operator session ended", id), false;
    if (rl_gait_active(rl)) return job_fail(rl, now, false, "arming motor %u stopped: a gait started", id), false;
    for (;;) {
        if (rl->arm_job.waiting) {
            rl->arm_job.waiting = false;
            if (!rl->arm_job.ok) return job_fail(rl, now, rl->arm_job.step > J_ARM, "motor %u: no reply to %s while arming", id, job_names[rl->arm_job.step]), false;
            int next = rl->arm_job.step + 1;
            if (rl->arm_job.step == J_ARM && !(rl->status.armed_mask >> i & 1)) {
                int32_t p = rl_position(rl, id);
                return job_fail(rl, now, false, "the FPGA did not arm motor %u: reading %ld, window %ld..%ld%s", id, (long)p, (long)t->armed_lo, (long)t->armed_hi,
                                rl->status.latched && rl->status.reason != 10 && rl->status.reason != 1 && rl->status.reason != 9 ? ", latched by a fault" : ", telemetry not fresh or the window refused"), false;
            }
            if (rl->arm_job.step == J_READ_MODE && rl->arm_job.read[0] == 2) next = J_DONE;
            if (rl->arm_job.step == J_VERIFY && rl->arm_job.read[0] != 2)
                return job_fail(rl, now, true, "motor %u did not accept open-loop PWM mode; it may need a power cycle", id), false;
            rl->arm_job.step = next;
            rl->arm_job.clock_ms = now;
        }
        switch (rl->arm_job.step) {
        case J_FRESH:
        case J_FRESH_A:
            if (fresh_since(rl, i, rl->arm_job.clock_ms, now)) { rl->arm_job.step++; continue; }
            if (before(rl->arm_job.clock_ms + 1000, now)) return job_fail(rl, now, false, "motor %u: no telemetry while arming", id), false;
            rl->next_poll_ms[i] = now;
            return false;
        case J_WINDOW: {
            if (t->multi_turn && !s->turn_known) return job_fail(rl, now, false, "motor %u's turn is not known", id), false;
            int32_t p = rl_position(rl, id);
            int32_t lo = t->open[RL_END_LO] ? -RL_TRAVEL_OPEN : t->lo, hi = t->open[RL_END_HI] ? RL_TRAVEL_OPEN : t->hi;
            /* Outside its travel: stretch the window only to where it already is. */
            t->armed_lo = p < lo ? p : lo;
            t->armed_hi = p > hi ? p : hi;
            uint8_t q[14] = {7, id};
            put_i32(q + 2, p);
            put_i32(q + 6, t->armed_lo);
            put_i32(q + 10, t->armed_hi);
            job_local(rl, now, q, sizeof q);
            return true;
        }
        case J_ARM: {
            uint8_t q[] = {HX_OP_ARM, id};
            rl->next_heartbeat_ms = now + rl->cfg.heartbeat_ms;
            job_local(rl, now, q, 2);
            return true;
        }
        case J_PWM_ZERO: { uint8_t q[] = {HX_REG_PWM, 0, 0}; job_servo(rl, now, T_J_WRITE, i, HX_WRITE, q, 3); return true; }
        case J_READ_MODE:
        case J_VERIFY: { uint8_t q[] = {HX_REG_MODE, 1}; job_servo(rl, now, T_J_READ, i, HX_READ, q, 2); return true; }
        case J_UNLOCK: { uint8_t q[] = {0x37, 0}; job_servo(rl, now, T_J_WRITE, i, HX_WRITE, q, 2); return true; }
        case J_SET_MODE: { uint8_t q[] = {HX_REG_MODE, 2}; job_servo(rl, now, T_J_WRITE, i, HX_WRITE, q, 2); return true; }
        case J_LOCK: { uint8_t q[] = {0x37, 1}; job_servo(rl, now, T_J_WRITE, i, HX_WRITE, q, 2); return true; }
        default: /* J_DONE */
            s->mode = 2;
            t->armed = true;
            t->recovering = t->armed_lo < (t->open[0] ? -RL_TRAVEL_OPEN : t->lo) || t->armed_hi > (t->open[1] ? RL_TRAVEL_OPEN : t->hi);
            rl->arm_job.axis = -1;
            if (t->recovering)
                note(rl, now, "motor %u armed outside its travel (%ld..%ld): only motion back toward it", id, (long)(t->open[0] ? -RL_TRAVEL_OPEN : t->lo), (long)(t->open[1] ? RL_TRAVEL_OPEN : t->hi));
            else note(rl, now, "motor %u armed in window %ld..%ld", id, (long)t->armed_lo, (long)t->armed_hi);
            return false;
        }
    }
}
