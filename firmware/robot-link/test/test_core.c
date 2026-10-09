/* The link engine against the behavioural bridge, millisecond by millisecond. */
#include "../core/robot_link.h"
#include "fake_bridge.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int failures;
#define CHECK(cond, ...) do { if (!(cond)) { failures++; printf("  FAIL %s:%d: ", __FILE__, __LINE__); printf(__VA_ARGS__); printf("\n"); } } while (0)

typedef struct {
    rl_t rl;
    fb_t fb;
    uint32_t now;
    uint32_t host_bytes;
    rl_gait_sample_t log[RL_GAIT_LOG];
} world_t;

static void to_bridge(void *ctx, const uint8_t *bytes, size_t n)
{
    world_t *w = ctx;
    w->host_bytes += (uint32_t)n;
    /* 115200 baud: a byte every ~87 us; the frame lands within the same millisecond. */
    fb_rx(&w->fb, bytes, n);
}

static void world_init(world_t *w, uint16_t present)
{
    memset(w, 0, sizeof *w);
    rl_config_t cfg = rl_default_config();
    cfg.present_mask = present;
    rl_init(&w->rl, cfg, to_bridge, w);
    fb_init(&w->fb, present);
}

/* Run `ms` milliseconds, optionally renewing the session (and a hold) every 50 ms. */
static void run(world_t *w, uint32_t ms, bool alive, uint8_t hold_id, int duty)
{
    for (uint32_t k = 0; k < ms; k++) {
        w->now++;
        fb_tick(&w->fb, w->now);
        uint8_t buf[64];
        size_t n = fb_tx(&w->fb, buf, sizeof buf);
        if (n) rl_rx(&w->rl, buf, n, w->now);
        if (alive && w->now % 50 == 0) {
            rl_session_alive(&w->rl, w->now);
            if (hold_id) rl_hold(&w->rl, hold_id, duty, w->now);
        }
        rl_tick(&w->rl, w->now);
    }
}

static void test_boot_status_and_telemetry(void)
{
    world_t w;
    world_init(&w, 0x1C0); /* IDs 10, 11, 12 */
    run(&w, 600, false, 0, 0);
    CHECK(w.rl.status_valid && w.rl.status.latched && w.rl.status.reason == 1, "boot status read: latched at boot");
    CHECK(rl_link_ok(&w.rl, w.now), "link ok");
    for (uint8_t id = 10; id <= 12; id++) CHECK(w.rl.servo[rl_axis(&w.rl, id)].valid, "telemetry of %u", id);
    CHECK(w.rl.timeouts == 0 && w.rl.unexpected == 0, "clean: %u timeouts %u unexpected", w.rl.timeouts, w.rl.unexpected);
    CHECK(rl_axis(&w.rl, 4) < 0 && rl_axis(&w.rl, 13) < 0, "absent servos are not axes");
}

static void test_arm_hold_release(void)
{
    world_t w;
    world_init(&w, 0x1C0);
    run(&w, 100, true, 0, 0);
    /* Arming needs fresh telemetry: the first poll of 10 may not have happened in 100 ms, so wait for it. */
    run(&w, 600, true, 0, 0);
    CHECK(rl_arm(&w.rl, 10, w.now), "arm 10: %s", w.rl.note);
    run(&w, 100, true, 0, 0);
    CHECK(!w.fb.latched && (w.fb.armed & 1 << 6), "the FPGA armed 10 and cleared its boot latch (reason %u)", w.fb.reason);
    CHECK(w.rl.status.armed_mask == (1 << 6), "the engine saw it");
    /* Hold at 300: clamped to the 100 cap, after a mode read and torque on. */
    run(&w, 400, true, 10, 300);
    fb_servo_t *s = &w.fb.servo[6];
    CHECK(s->torque == 1 && s->torque_writes == 1, "torque enabled once (%u)", s->torque_writes);
    CHECK(s->pwm_raw == 100, "drive clamped to the cap: %u", s->pwm_raw);
    CHECK(s->pwm_writes >= 5 && s->pwm_writes <= 9, "rewritten every 50 ms: %u", s->pwm_writes);
    CHECK(w.rl.servo[6].mode == 2, "mode read");
    /* Reverse. */
    run(&w, 200, true, 10, -40);
    CHECK(s->pwm_raw == (40 | 1 << HX_PWM_DIRECTION_BIT), "signed drive uses the direction bit: %04x", s->pwm_raw);
    /* Release: zero written at once, and no further drive. */
    rl_release(&w.rl);
    run(&w, 10, true, 0, 0);
    CHECK(s->pwm_raw == 0, "zero written on release (%u)", s->pwm_raw);
    uint32_t writes = s->pwm_writes;
    run(&w, 300, true, 0, 0);
    CHECK(s->pwm_writes == writes, "nothing more after release");
    CHECK(!w.fb.latched && w.fb.stops == 0, "the lease was kept by heartbeats: latched %d reason %u", w.fb.latched, w.fb.reason);
    CHECK(w.rl.refused == 0 && w.rl.timeouts == 0, "no refusals (%u) or timeouts (%u)", w.rl.refused, w.rl.timeouts);
}

static void test_hold_that_is_not_renewed_ends(void)
{
    world_t w;
    world_init(&w, 0x1C0);
    run(&w, 700, true, 0, 0);
    rl_arm(&w.rl, 11, w.now);
    run(&w, 300, true, 11, 60);
    fb_servo_t *s = &w.fb.servo[7];
    CHECK(s->pwm_raw == 60, "driving: %u", s->pwm_raw);
    /* The page keeps the session alive but stops renewing the hold (a lost pointerup). */
    run(&w, 400, true, 0, 0);
    CHECK(s->pwm_raw == 0, "the hold timed out and the drive was zeroed: %u", s->pwm_raw);
    CHECK(!w.fb.latched, "the FPGA stayed armed (the session is alive)");
}

static void test_session_loss_stops_and_fpga_lease_backs_it(void)
{
    world_t w;
    world_init(&w, 0x1C0);
    run(&w, 700, true, 0, 0);
    rl_arm(&w.rl, 12, w.now);
    run(&w, 300, true, 12, 50);
    CHECK(w.fb.servo[8].pwm_raw == 50, "driving");
    /* The page vanishes without a word. */
    run(&w, 1100, false, 0, 0);
    CHECK(w.fb.latched && w.fb.reason == 10, "the engine sent STOP when the session went quiet: latched %d reason %u", w.fb.latched, w.fb.reason);
    CHECK(w.fb.servo[8].pwm_raw == 0 && w.fb.servo[8].torque == 0, "stopped and torque off");

    /* Now the ESP32 itself dies while holding: the FPGA's own lease stops it. */
    world_init(&w, 0x1C0);
    run(&w, 700, true, 0, 0);
    rl_arm(&w.rl, 12, w.now);
    run(&w, 300, true, 12, 50);
    CHECK(w.fb.servo[8].pwm_raw == 50, "driving");
    for (int k = 0; k < 400; k++) { w.now++; fb_tick(&w.fb, w.now); } /* no host at all */
    /* Telemetry (200 ms) or the command lease (300 ms), whichever the FPGA reaches first. */
    CHECK(w.fb.latched && (w.fb.reason == 7 || w.fb.reason == 8) && w.fb.fault_id == 12, "the FPGA tripped on its own: latched %d reason %u id %u", w.fb.latched, w.fb.reason, w.fb.fault_id);
    CHECK(w.fb.servo[8].pwm_raw == 0, "and zeroed the drive");
}

static void test_stop_from_anyone_and_refusals(void)
{
    world_t w;
    world_init(&w, 0x1C0);
    run(&w, 700, true, 0, 0);
    rl_arm(&w.rl, 10, w.now);
    run(&w, 300, true, 10, 50);
    rl_stop(&w.rl);
    run(&w, 5, true, 0, 0);
    CHECK(w.fb.latched && w.fb.reason == 10 && w.fb.servo[6].pwm_raw == 0, "STOP is immediate");
    run(&w, 300, true, 0, 0);
    CHECK(w.rl.status_valid && w.rl.status.latched, "the engine knows it is latched");
    CHECK(!rl_hold(&w.rl, 10, 50, w.now), "a hold while latched is refused locally");
    CHECK(strstr(w.rl.note, "latched") != NULL, "and says so: %s", w.rl.note);
    /* The stop button (S2): the FPGA latches on its own; the engine's next drive is dropped and seen as refused. */
    run(&w, 300, true, 0, 0);
    rl_arm(&w.rl, 10, w.now);
    run(&w, 300, true, 10, 50);
    CHECK(w.fb.servo[6].pwm_raw == 50, "driving again after rearm");
    w.fb.s2 = true;
    run(&w, 300, true, 10, 50);
    CHECK(w.fb.reason == 9, "S2 latched the FPGA");
    CHECK(!w.rl.jog_id && w.rl.status.latched, "the engine saw the latch and ended the jog");
    CHECK(w.fb.servo[6].pwm_raw == 0, "nothing is driven");
    w.fb.s2 = false;
    /* A servo not in PWM mode is never driven. */
    world_init(&w, 0x1C0);
    w.fb.servo[6].mode = 0;
    run(&w, 700, true, 0, 0);
    rl_arm(&w.rl, 10, w.now);
    run(&w, 400, true, 10, 50);
    CHECK(w.fb.servo[6].pwm_writes == 0 && w.fb.servo[6].torque == 0, "position-mode servo not driven");
    CHECK(strstr(w.rl.note, "control mode 0") != NULL, "%s", w.rl.note);
}

static void test_telemetry_keeps_pace_and_bus_noise(void)
{
    world_t w;
    world_init(&w, 0x1FF); /* all nine */
    run(&w, 1500, true, 0, 0);
    for (int i = 0; i < 9; i++) CHECK(w.rl.servo[i].valid, "servo %d polled", i + 4);
    for (int i = 0; i < 9; i++) if (w.rl.servo[i].valid) rl_arm(&w.rl, (uint8_t)(i + 4), w.now);
    run(&w, 3000, true, 0, 0);
    CHECK(w.fb.armed == 0x1FF && !w.fb.latched, "nine armed servos stayed fresh for 3 s at 115200 baud: armed %03x latched %d reason %u id %u", w.fb.armed, w.fb.latched, w.fb.reason, w.fb.fault_id);
    /* Line noise between frames is dropped and counted, not fatal. */
    uint8_t noise[] = {0x00, 0xFF, 0x12, 0xFF, 0xFF};
    rl_rx(&w.rl, noise, sizeof noise, w.now);
    run(&w, 500, true, 0, 0);
    CHECK(w.rl.framer.dropped_bytes >= 3, "noise dropped: %u", w.rl.framer.dropped_bytes);
    CHECK(!w.fb.latched, "and the bus went on");
}

static void test_wire_format(void)
{
    uint8_t f[HX_MAX_FRAME];
    size_t n = hx_packet(f, 254, 0xA0, (uint8_t[]){1, 12}, 2);
    CHECK(n == 8 && memcmp(f, (uint8_t[]){255, 255, 254, 4, 160, 1, 12, 80}, 8) == 0, "ARM 12 matches servo_safety.rs");
    n = hx_packet(f, 1, 2, (uint8_t[]){0x38, 2}, 2);
    CHECK(memcmp(f, (uint8_t[]){255, 255, 1, 4, 2, 0x38, 2, 0xbe}, 8) == 0, "read matches servo_bus.rs");
    uint8_t pwm[2];
    CHECK(hx_pwm_bytes(-100, pwm) && pwm[0] == 100 && pwm[1] == 4, "signed PWM matches servo_bus.rs");
    CHECK(!hx_pwm_bytes(1001, pwm), "magnitude bound");
    hx_telemetry_t t;
    uint8_t regs[15] = {0, 8, 1, 0x80, 0, 0, 104, 49};
    CHECK(hx_telemetry_decode(regs, 15, &t) && t.position_raw == 2048 && t.speed_counts_s == -1 && t.voltage_raw == 104 && t.temperature_c == 49, "telemetry decode");
    hx_status_t s;
    uint8_t st[13] = {1, 0, 0, 254, 0, 1, 0, 1, 60, 90, 126, 208, 7};
    CHECK(hx_status_decode(st, 13, &s) && s.armed_mask == 0x100 && s.current_max_raw == 2000, "status decode");
    st[1] = 1;
    CHECK(!hx_status_decode(st, 13, &s), "latched with armed bits is inconsistent");
    /* The leg's calibration profile 8, as read back after its 2026-09-25 load. */
    uint8_t cal[13] = {8, 1, 10, 254, 0, 0, 0, 0, 60, 90, 126, 208, 7};
    CHECK(hx_status_decode(cal, 13, &s) && s.version == 8 && s.latched && s.reason == 10, "calibration profile status decode");
    cal[2] = 13;
    CHECK(hx_status_decode(cal, 13, &s) && !strcmp(hx_reason_name(s.reason), "ambiguous encoder jump"), "calibration encoder-jump reason");
    cal[0] = 4;
    CHECK(!hx_status_decode(cal, 13, &s), "unknown profile version");
}


/* ---- gait playback against the calibration profile ---------------------- */

static uint8_t plan_bytes[16384];
static size_t plan_len;

/* The first gait of the leg's pack (fixtures/leg-plan.json says how it was made). */
static bool load_fixture(rl_plan_t *plan)
{
    FILE *f = fopen("fixtures/leg-plan.bin", "rb");
    if (!f) return false;
    plan_len = fread(plan_bytes, 1, sizeof plan_bytes, f);
    fclose(f);
    return rl_plan_parse(plan_bytes, plan_len, plan);
}

/* The worm's true place, counts plus turns, in the plan's frame (taught travel -1407..3116). */
#define WORM_AT (-300.f)

static void world_init_leg(world_t *w)
{
    memset(w, 0, sizeof *w);
    rl_config_t cfg = rl_default_config();
    cfg.first_id = 1;
    cfg.present_mask = 0x7; /* knee (1), worm (2), belt/hip (3) */
    rl_init(&w->rl, cfg, to_bridge, w);
    rl_gait_set_log(&w->rl, w->log, RL_GAIT_LOG);
    fb_init_calibration(&w->fb, 0x7);
    /* Inside their taught windows (2514..3329, -1407..3116 and 2236..3322), away from the
     * gait's first pose. The worm reads 3796: its gait (653..1499) is across encoder zero. */
    w->fb.servo[0].position_f = 3000.f;
    w->fb.servo[1].position_f = WORM_AT;
    w->fb.servo[2].position_f = 2500.f;
    /* The pack's travel table, as gait_store reads it: the plan's windows here. */
    rl_plan_t p;
    if (load_fixture(&p))
        for (int k = 0; k < p.axes; k++)
            rl_travel_set_pack(&w->rl, p.axis[k].id, p.axis[k].window_lo, p.axis[k].window_hi, p.axis[k].multi_turn, p.axis[k].id == 2, p.axis[k].turn_margin_counts);
}

/* What the page does: the operator picks the place that matches the leg. */
static bool confirm_worm(world_t *w, float at)
{
    int32_t places[2];
    int n = rl_turn_candidates(&w->rl, 2, w->now, places);
    for (int c = 0; c < n; c++)
        if (fabsf((float)places[c] - at) < 64.f) return rl_turn_confirm(&w->rl, 2, places[c], w->now);
    return false;
}

static void test_plan_matches_the_rust_governor(void)
{
    rl_plan_t p;
    CHECK(load_fixture(&p), "fixture plan parses");
    CHECK(p.axes == 3 && p.axis[0].id == 1 && p.axis[1].id == 2 && p.axis[2].id == 3, "plan drives knee, worm and hip");
    CHECK(p.axis[0].window_lo == 2514 && p.axis[0].window_hi == 3329 && !p.axis[0].multi_turn && p.axis[0].drive == RL_DRIVE_POSITION, "knee: taught poses, position mode");
    CHECK(p.axis[1].window_lo == -1407 && p.axis[1].window_hi == 3116 && p.axis[1].multi_turn && p.axis[1].drive == RL_DRIVE_SPEED, "worm: taught poses across a turn, speed mode");
    CHECK(p.axis[1].turn_margin_counts == 256 && fabsf(p.axis[1].hold_tolerance_counts - 16.f) < 1e-6f, "worm: turn margin and the panel's hold deadband");
    float worst = rl_plan_check(&p);
    CHECK(worst < 0.5f, "C governor follows the Rust governor: worst %.3f counts", worst);
    printf("  plan: %u samples over %.2f s, governor check worst %.3f counts over %u steps\n", p.samples, p.period_s, worst, p.check_steps);
    for (int k = 0; k < p.axes; k++)
        for (int s = 0; s < p.samples; s++) {
            float d = rl_plan_desired(&p, k, s * p.period_s / p.samples);
            CHECK(d >= p.axis[k].window_lo + 5.9f && d <= p.axis[k].window_hi - 5.9f, "desired inside the window margin");
        }
    /* Periodic, and linear between samples. */
    CHECK(fabsf(rl_plan_desired(&p, 0, 0.1f) - rl_plan_desired(&p, 0, 0.1f + 2 * p.period_s)) < 0.01f, "periodic");
    uint8_t truncated[64];
    memcpy(truncated, plan_bytes, sizeof truncated);
    CHECK(!rl_plan_parse(truncated, sizeof truncated, &p), "truncated plan refused");
    /* A multi-turn axis in position mode would drive the wrong way round: refused. */
    uint8_t bad[sizeof plan_bytes];
    memcpy(bad, plan_bytes, plan_len);
    bad[12 + RL_PLAN_AXIS_BYTES + 1] = RL_DRIVE_POSITION;
    CHECK(!rl_plan_parse(bad, plan_len, &p), "multi-turn axis in position mode refused");
}

static void test_position_goal_matches_servo_command(void)
{
    /* calibration_serial.rs servo_command(ServoPosition): goal clamped to the
     * window, wrapped to one turn; speed |v| * 1.3 + 150 within 150..3000. */
    uint8_t b[7];
    rl_position_goal(3500.f, 200.f, 2514, 3329, b);
    CHECK(b[0] == 0x2A && (b[1] | b[2] << 8) == 3329 && b[3] == 0 && b[4] == 0 && (b[5] | b[6] << 8) == 410, "clamped goal, speed 410");
    rl_position_goal(2600.4f, -5000.f, 2514, 3329, b);
    CHECK((b[1] | b[2] << 8) == 2600 && (b[5] | b[6] << 8) == 3000, "rounded goal, speed capped");
    rl_position_goal(2600.5f, 0.f, 2514, 3329, b);
    CHECK((b[1] | b[2] << 8) == 2601 && (b[5] | b[6] << 8) == 150, "half rounds away from zero, minimum speed");
}

static int speed_of(const uint8_t b[3])
{
    uint16_t v = (uint16_t)(b[1] | b[2] << 8);
    return (v & 0x7fff) * ((v & 0x8000) ? -1 : 1);
}

static void test_speed_goal_matches_servo_command(void)
{
    /* calibration_serial.rs servo_command(ServoSpeed)'s own test vectors. */
    uint8_t b[3];
    const int32_t lo = 1000, hi = 3000;
    rl_speed_goal(2010.f, 300.f, 2000, lo, hi, 16.f, b);
    CHECK(b[0] == 0x2E && speed_of(b) == 340, "reference plus trim: %d", speed_of(b));
    rl_speed_goal(1990.f, -300.f, 2000, lo, hi, 16.f, b);
    CHECK(speed_of(b) == -340, "negative: %d", speed_of(b));
    rl_speed_goal(2005.f, 0.f, 2000, lo, hi, 16.f, b);
    CHECK(speed_of(b) == 0, "settled within the hold tolerance");
    rl_speed_goal(hi + 5.f, 200.f, hi, lo, hi, 16.f, b);
    CHECK(speed_of(b) == 0, "nothing outward at the window edge");
    rl_speed_goal(hi - 50.f, -200.f, hi, lo, hi, 16.f, b);
    CHECK(speed_of(b) == -400, "inward from the edge: %d", speed_of(b));
    rl_speed_goal(-900.f, 0.f, 2500, -1407, 3116, 16.f, b);
    CHECK(speed_of(b) == -3000, "capped at 3000 counts/s, turns included: %d", speed_of(b));
}

static void run_gait(world_t *w, uint32_t ms, bool alive)
{
    run(w, ms, alive, 0, 0);
}

static void test_turns_are_confirmed_by_the_operator_and_forgotten_on_a_gap(void)
{
    world_t w;
    world_init_leg(&w);
    rl_plan_t p;
    CHECK(load_fixture(&p), "fixture");
    run_gait(&w, 700, true);
    CHECK(rl_gait_load(&w.rl, &p, w.now), "load: %s", w.rl.note);
    CHECK(!rl_gait_start(&w.rl, 1.f, w.now) && strstr(w.rl.note, "turn is not confirmed"), "no start before the worm's turn is known: %s", w.rl.note);
    CHECK(!rl_arm(&w.rl, 2, w.now) && strstr(w.rl.note, "turn is not confirmed"), "no arming the worm either: %s", w.rl.note);
    /* Reading 3796: inside the travel only as -300. */
    int32_t places[2];
    int n = rl_turn_candidates(&w.rl, 2, w.now, places);
    CHECK(n == 1 && places[0] == -300, "one place: %d (%ld)", n, n ? (long)places[0] : 0L);
    CHECK(!rl_turn_confirm(&w.rl, 2, 3796, w.now), "a place the reading does not allow is refused: %s", w.rl.note);
    CHECK(!rl_turn_confirm(&w.rl, 1, 3000, w.now) && strstr(w.rl.note, "not counted in turns"), "the knee is not counted in turns: %s", w.rl.note);
    /* In the band both turns reach, the reading allows two places. */
    w.fb.servo[1].position_f = 2900.f;
    run_gait(&w, 600, true);
    n = rl_turn_candidates(&w.rl, 2, w.now, places);
    CHECK(n == 2 && places[0] == -1196 && places[1] == 2900, "two places in the band: %d (%ld, %ld)", n, (long)places[0], (long)places[1]);
    CHECK(rl_turn_confirm(&w.rl, 2, 2900, w.now) && w.rl.servo[1].counts == 2900, "the operator's pick: %ld", (long)w.rl.servo[1].counts);
    /* The count follows the reading across encoder zero, both ways. */
    w.fb.servo[1].position_f = 4300.f;
    for (int k = 0; k < 20; k++) { w.fb.servo[1].position_f -= 100.f; run_gait(&w, 120, true); }
    run_gait(&w, RL_TURN_POLL_MS + 20, true);
    CHECK(w.rl.servo[1].turn_known && w.rl.servo[1].counts == w.fb.servo[1].position_f, "counted %ld, shaft %.0f", (long)w.rl.servo[1].counts, w.fb.servo[1].position_f);
    /* A missed reading forgets it. */
    w.fb.servo[1].present = false;
    run_gait(&w, 400, true);
    CHECK(!w.rl.servo[1].turn_known && strstr(w.rl.note, "missed reading"), "forgotten: %s", w.rl.note);
    w.fb.servo[1].present = true;
    run_gait(&w, 700, true);
    CHECK(rl_turn_confirm(&w.rl, 2, 2300, w.now) && w.rl.servo[1].turn_known, "confirmed again: %s", w.rl.note);
    /* So does lending the link to the tunnel. */
    rl_pause(&w.rl, true);
    CHECK(!w.rl.servo[1].turn_known && strstr(w.rl.note, "tunnel"), "forgotten while lent: %s", w.rl.note);
    rl_pause(&w.rl, false);
}


/* Run until the engine's arm sequence is done (or `ms` passes). */
static void run_arm(world_t *w, uint32_t ms)
{
    for (uint32_t k = 0; k < ms && (k < 5 || w->rl.arm_job.axis >= 0); k += 5) run(w, 5, true, 0, 0);
    run(w, 60, true, 0, 0);
}

/* Hold `duty` on motor `id` for `ms`, renewed every 50 ms as the page does. */
static void hold_for(world_t *w, uint8_t id, int duty, uint32_t ms)
{
    run(w, ms, true, id, duty);
    rl_release(&w->rl);
    run(w, 100, true, 0, 0);
}

static void test_calibrated_arm_jogs_inside_the_taught_window(void)
{
    world_t w;
    world_init_leg(&w);
    w.rl.cfg.duty_cap = 1000;
    run_gait(&w, 700, true);
    CHECK(rl_arm(&w.rl, 1, w.now), "arm the knee: %s", w.rl.note);
    run_arm(&w, 1500);
    CHECK(w.fb.armed == 0x1 && w.fb.window_valid[0] && w.fb.lower[0] == 2514 && w.fb.upper[0] == 3329, "armed in its taught window (armed %x, window %ld..%ld): %s",
          w.fb.armed, (long)w.fb.lower[0], (long)w.fb.upper[0], w.rl.note);
    CHECK(w.fb.servo[0].mode == 2 && w.fb.servo[0].lock == 1 && w.rl.servo[0].mode == 2, "open-loop PWM mode, relocked (mode %u)", w.fb.servo[0].mode);
    CHECK(w.rl.servo[0].travel.armed && !w.rl.servo[0].travel.recovering, "not recovering");
    /* Up to the high end: the FPGA stops it there, and the engine says so. */
    hold_for(&w, 1, 300, 1000);
    float top = w.fb.servo[0].position_f;
    CHECK(top >= 3329.f && top < 3329.f + 120.f, "jogged up to the window's end and held there: %.0f", top);
    CHECK(!rl_hold(&w.rl, 1, 300, w.now) && strstr(w.rl.note, "window's end"), "outward refused at the edge: %s", w.rl.note);
    hold_for(&w, 1, -300, 500);
    CHECK(w.fb.servo[0].position_f < top - 200.f, "back inward: %.0f", w.fb.servo[0].position_f);
    CHECK(!w.fb.latched, "nothing tripped (reason %u)", w.fb.reason);
    rl_stop(&w.rl);
    run_gait(&w, 200, true);
    CHECK(!w.rl.servo[0].travel.armed && w.fb.armed == 0, "STOP disarms");
}

static void test_a_motor_outside_its_travel_is_armed_to_come_back_in(void)
{
    world_t w;
    world_init_leg(&w);
    w.rl.cfg.duty_cap = 1000;
    w.fb.servo[0].position_f = 3520.f; /* the knee drooped past its taught 3329 */
    run_gait(&w, 700, true);
    CHECK(rl_arm(&w.rl, 1, w.now), "arm: %s", w.rl.note);
    run_arm(&w, 1500);
    CHECK(w.fb.armed == 0x1 && w.fb.lower[0] == 2514 && w.fb.upper[0] == 3520, "armed with the window stretched to the reading (%ld..%ld): %s",
          (long)w.fb.lower[0], (long)w.fb.upper[0], w.rl.note);
    CHECK(w.rl.servo[0].travel.recovering && strstr(w.rl.note, "only motion back"), "recovering: %s", w.rl.note);
    CHECK(!rl_hold(&w.rl, 1, 200, w.now), "no further out: %s", w.rl.note);
    hold_for(&w, 1, -300, 600);
    CHECK(w.fb.servo[0].position_f < 3329.f, "back inside its travel: %.0f", w.fb.servo[0].position_f);
    /* Out again only as far as it already was. */
    hold_for(&w, 1, 300, 1000);
    CHECK(w.fb.servo[0].position_f <= 3520.f + 120.f, "never past where it was: %.0f", w.fb.servo[0].position_f);
    CHECK(!w.fb.latched, "nothing tripped (reason %u)", w.fb.reason);
}

static void test_poses_taught_on_the_page(void)
{
    world_t w;
    world_init_leg(&w);
    w.rl.cfg.duty_cap = 1000;
    rl_plan_t p;
    load_fixture(&p);
    run_gait(&w, 700, true);
    rl_gait_load(&w.rl, &p, w.now);
    /* Open the knee's high end, jog past the old pose, set it there. */
    CHECK(rl_travel_open(&w.rl, 1, RL_END_HI, w.now), "open: %s", w.rl.note);
    CHECK(!rl_travel_open(&w.rl, 1, RL_END_LO, w.now), "never both ends open: %s", w.rl.note);
    CHECK(rl_arm(&w.rl, 1, w.now), "arm: %s", w.rl.note);
    run_arm(&w, 1500);
    CHECK(w.fb.upper[0] == RL_TRAVEL_OPEN, "the FPGA's window is open above: %ld", (long)w.fb.upper[0]);
    hold_for(&w, 1, 300, 600);
    float at = w.fb.servo[0].position_f;
    CHECK(at > 3400.f, "jogged past the old pose: %.0f", at);
    run_gait(&w, 120, true);
    CHECK(rl_travel_teach(&w.rl, 1, RL_END_HI, w.now), "set high here: %s", w.rl.note);
    const rl_travel_t *t = &w.rl.servo[0].travel;
    CHECK(!t->open[RL_END_HI] && fabsf((float)t->hi - at) < 30.f && t->lo == 2514 && rl_travel_changed(t) && w.rl.travel_dirty, "taught %ld..%ld", (long)t->lo, (long)t->hi);
    CHECK(!rl_travel_teach(&w.rl, 1, RL_END_LO, w.now) && strstr(w.rl.note, "at least"), "an empty travel is refused: %s", w.rl.note);
    /* A gait refuses a motor whose poses changed here, until they are promoted. */
    rl_stop(&w.rl);
    run_gait(&w, 300, true);
    rl_turn_confirm(&w.rl, 2, (int32_t)WORM_AT, w.now);
    CHECK(!rl_gait_start(&w.rl, 1.f, w.now) && strstr(w.rl.note, "pull-calibration"), "gait refused: %s", w.rl.note);
    CHECK(rl_travel_revert(&w.rl, 1, w.now) && !rl_travel_changed(t), "revert");
    /* The worm is reversed in calibration.json: its low-count end is its upper pose. */
    CHECK(rl_travel_open(&w.rl, 2, RL_END_LO, w.now) && strstr(w.rl.note, "upper pose"), "reversed motor named by pose: %s", w.rl.note);
    rl_travel_revert(&w.rl, 2, w.now);
    /* A new pack with the taught travel makes it the pack's own. */
    CHECK(rl_travel_restore(&w.rl, 1, 2514, 3450, false, false) && rl_travel_changed(t), "restored from storage");
    rl_travel_set_pack(&w.rl, 1, 2514, 3450, false, false, 0);
    CHECK(!rl_travel_changed(t) && t->hi == 3450, "promoted");
}

static void test_gait_prepares_approaches_plays_and_stops(void)
{
    world_t w;
    world_init_leg(&w);
    rl_plan_t p;
    CHECK(load_fixture(&p), "fixture");
    run_gait(&w, 700, true);
    CHECK(rl_gait_load(&w.rl, &p, w.now), "load: %s", w.rl.note);
    CHECK(confirm_worm(&w, WORM_AT), "worm confirmed: %s", w.rl.note);
    CHECK(rl_gait_start(&w.rl, 1.f, w.now), "start: %s", w.rl.note);
    run_gait(&w, 2000, true);
    CHECK(w.fb.armed == 0x7 && !w.fb.latched, "all three motors armed: armed 0x%x latched %d reason %u (%s)", w.fb.armed, w.fb.latched, w.fb.reason, w.rl.gait.result);
    CHECK(w.fb.continuous[1] == w.rl.servo[1].counts, "the FPGA counts the worm's turns from the anchor: %ld vs %ld", (long)w.fb.continuous[1], (long)w.rl.servo[1].counts);
    CHECK(w.fb.servo[0].mode == 0 && w.fb.servo[1].mode == 1 && w.fb.servo[2].mode == 0, "position, speed, position mode");
    CHECK(w.fb.servo[0].lock == 1 && w.fb.servo[1].lock == 1 && w.fb.servo[2].lock == 1, "mode registers locked again");
    CHECK(w.fb.servo[0].torque && w.fb.servo[1].torque && w.fb.servo[2].torque, "torque on");
    run_gait(&w, 5000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_PLAYING, "playing after the approach (phase %s, %s)", rl_gait_phase_name(w.rl.gait.phase), w.rl.gait.result);
    CHECK(w.rl.gait.t > 1.f, "gait clock running: %.2f s", w.rl.gait.t);
    CHECK(w.fb.refused == 0 && w.rl.refused == 0, "no write refused: bridge %u engine %u", w.fb.refused, w.rl.refused);
    CHECK(w.fb.servo[1].position_f > 500.f && w.rl.servo[1].counts > 500, "the worm crossed encoder zero to its gait: shaft %.0f, counted %ld", w.fb.servo[1].position_f, (long)w.rl.servo[1].counts);
    /* The shaft follows the governed command (the model servo is ideal). */
    float worst[3] = {0};
    for (uint32_t k = 0; k < w.rl.gait.log_count && k < RL_GAIT_LOG; k++) {
        const rl_gait_sample_t *s = &w.rl.gait.log[k];
        if (s->gait_t < 0.5f) continue;
        float e = fabsf((float)s->actual - s->command);
        if (e > worst[s->id - 1]) worst[s->id - 1] = e;
    }
    CHECK(w.rl.gait.log_count > 100, "samples logged: %u", w.rl.gait.log_count);
    CHECK(worst[0] < 60.f && worst[1] < 60.f && worst[2] < 60.f, "tracking within 60 counts of the command: %.1f %.1f %.1f", worst[0], worst[1], worst[2]);
    uint32_t goals = w.fb.servo[0].goal_writes, speeds = w.fb.servo[1].speed_writes;
    CHECK(goals > 100 && speeds > 100 && w.fb.servo[1].goal_writes == 0, "knee goals %u, worm speeds %u, no worm position goal (%u)", goals, speeds, w.fb.servo[1].goal_writes);
    printf("  gait: %.2f s of gait time, %u samples, %u knee goals, %u worm speeds, worst |actual - command| %.1f / %.1f / %.1f counts, %u host bytes\n",
           w.rl.gait.t, w.rl.gait.log_count, goals, speeds, worst[0], worst[1], worst[2], w.host_bytes);
    /* Pause holds the clock; speed scale is honoured. */
    rl_gait_control(&w.rl, false, 0.5f);
    float t0 = w.rl.gait.t;
    run_gait(&w, 500, true);
    CHECK(fabsf(w.rl.gait.t - t0) < 1e-6f, "paused clock holds");
    rl_gait_control(&w.rl, true, 0.5f);
    run_gait(&w, 1000, true);
    CHECK(fabsf(w.rl.gait.t - t0 - 0.5f) < 0.08f, "half speed: %.3f s in 1 s", w.rl.gait.t - t0);
    /* STOP ends in a verified torque-off, and a verified stop keeps the turn. */
    rl_stop(&w.rl);
    run_gait(&w, 2000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_IDLE && w.rl.gait.stop_verified, "stopped and verified: %s", w.rl.gait.result);
    CHECK(w.fb.latched && w.fb.armed == 0 && !w.fb.servo[0].torque && !w.fb.servo[1].torque && !w.fb.servo[2].torque, "FPGA latched, torque off");
    CHECK(w.fb.reason == 10, "host stop, not a trip: reason %u", w.fb.reason);
    CHECK(w.rl.servo[1].turn_known && w.rl.servo[1].counts == lroundf(w.fb.servo[1].position_f), "turn kept: %ld vs shaft %.0f", (long)w.rl.servo[1].counts, w.fb.servo[1].position_f);
    printf("  gait stop: %s\n", w.rl.gait.result);
    /* Played again without asking, from where it stopped. */
    CHECK(rl_gait_start(&w.rl, 1.f, w.now), "second start: %s", w.rl.note);
    run_gait(&w, 3000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_PLAYING && w.fb.refused == 0, "playing again (%s, refused %u)", rl_gait_phase_name(w.rl.gait.phase), w.fb.refused);
    rl_stop(&w.rl);
    run_gait(&w, 2000, true);
}

static void test_gait_ends_when_the_worm_reading_is_missed(void)
{
    world_t w;
    world_init_leg(&w);
    rl_plan_t p;
    load_fixture(&p);
    run_gait(&w, 700, true);
    rl_gait_load(&w.rl, &p, w.now);
    confirm_worm(&w, WORM_AT);
    rl_gait_start(&w.rl, 1.f, w.now);
    run_gait(&w, 5000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_PLAYING, "playing (%s)", w.rl.gait.result);
    w.fb.servo[1].present = false; /* the worm stops answering */
    run_gait(&w, 200, true);
    CHECK(w.rl.gait.phase != RL_GAIT_PLAYING && strstr(w.rl.gait.result, "turn was lost"), "the gait stops: %s", w.rl.gait.result);
    w.fb.servo[1].present = true;
    run_gait(&w, 2000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_IDLE && !w.rl.servo[1].turn_known && w.fb.latched, "stopped, latched, turn unknown: %s", w.rl.gait.result);
    CHECK(!rl_gait_start(&w.rl, 1.f, w.now) && strstr(w.rl.note, "not confirmed"), "no restart without confirming: %s", w.rl.note);
}

static void test_gait_ends_when_the_session_does(void)
{
    world_t w;
    world_init_leg(&w);
    rl_plan_t p;
    load_fixture(&p);
    run_gait(&w, 700, true);
    rl_gait_load(&w.rl, &p, w.now);
    confirm_worm(&w, WORM_AT);
    rl_gait_start(&w.rl, 1.f, w.now);
    run_gait(&w, 3000, true);
    CHECK(rl_gait_active(&w.rl) && w.fb.armed == 0x7, "running");
    run_gait(&w, 3000, false); /* the page went away */
    CHECK(w.rl.gait.phase == RL_GAIT_IDLE && w.rl.gait.stop_verified, "session loss stops it: %s", w.rl.gait.result);
    CHECK(strstr(w.rl.gait.result, "session") != NULL, "reason names the session: %s", w.rl.gait.result);
    CHECK(w.fb.latched && w.fb.armed == 0, "FPGA latched");
}

static void test_gait_refused_outside_the_window_and_on_the_bench_profile(void)
{
    world_t w;
    world_init_leg(&w);
    w.fb.servo[0].position_f = 2400.f; /* knee below its taught pose */
    rl_plan_t p;
    load_fixture(&p);
    run_gait(&w, 700, true);
    rl_gait_load(&w.rl, &p, w.now);
    confirm_worm(&w, WORM_AT);
    CHECK(rl_gait_start(&w.rl, 1.f, w.now), "start accepted");
    run_gait(&w, 3000, true);
    CHECK(w.rl.gait.phase == RL_GAIT_IDLE && strstr(w.rl.gait.result, "did not arm motor 1"), "refused: %s", w.rl.gait.result);
    CHECK(w.fb.armed == 0 && w.fb.servo[0].goal_writes == 0, "nothing armed, no goal sent");

    /* The worm inside the turn margin but past its taught pose: confirmable, not playable. */
    world_t m;
    world_init_leg(&m);
    m.fb.servo[1].position_f = -1500.f;
    run_gait(&m, 700, true);
    rl_gait_load(&m.rl, &p, m.now);
    CHECK(confirm_worm(&m, -1500.f), "confirmed inside the margin: %s", m.rl.note);
    CHECK(!rl_gait_start(&m.rl, 1.f, m.now) && strstr(m.rl.note, "outside its taught travel"), "refused: %s", m.rl.note);

    world_t b;
    world_init(&b, 0x1C0);
    run(&b, 700, true, 0, 0);
    rl_plan_t q = p;
    q.axes = 2;
    q.axis[0].id = 10;
    q.axis[1] = p.axis[2];
    q.axis[1].id = 12;
    CHECK(rl_gait_load(&b.rl, &q, b.now), "load on the bench");
    CHECK(!rl_gait_start(&b.rl, 1.f, b.now) && strstr(b.rl.note, "calibration image"), "bench profile refused: %s", b.rl.note);
}

int main(void)
{
    test_wire_format();
    test_boot_status_and_telemetry();
    test_arm_hold_release();
    test_hold_that_is_not_renewed_ends();
    test_session_loss_stops_and_fpga_lease_backs_it();
    test_stop_from_anyone_and_refusals();
    test_telemetry_keeps_pace_and_bus_noise();
    test_plan_matches_the_rust_governor();
    test_position_goal_matches_servo_command();
    test_speed_goal_matches_servo_command();
    test_turns_are_confirmed_by_the_operator_and_forgotten_on_a_gap();
    test_calibrated_arm_jogs_inside_the_taught_window();
    test_a_motor_outside_its_travel_is_armed_to_come_back_in();
    test_poses_taught_on_the_page();
    test_gait_prepares_approaches_plays_and_stops();
    test_gait_ends_when_the_worm_reading_is_missed();
    test_gait_ends_when_the_session_does();
    test_gait_refused_outside_the_window_and_on_the_bench_profile();
    printf(failures ? "%d FAILURE(S)\n" : "all robot-link core tests passed\n", failures);
    return failures ? 1 : 0;
}
