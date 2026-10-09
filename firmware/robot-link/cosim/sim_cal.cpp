// The gait player against the leg's real calibration profile: the same
// bridge.v and hx_safety.v built with the bridge-calibration parameters
// (IDs 1-3, CALIBRATION=1, 400/600 ms leases), the knee and hip plan from the
// leg's gait pack (knee and hip in position mode, the worm in speed mode
// across encoder zero, counted in turns), real UART bits on both sides. It
// checks what only the RTL can: that the taught windows anchored to live
// feedback (the worm's with its confirmed turns), arming, the control modes,
// every goal and speed and the leases are accepted, and that a STOP or a lost
// session ends in a verified torque-off.
#include "world.h"

static std::vector<uint8_t> plan_bytes;

static void set_travel(World &w);
// Inside their taught windows, away from the gait's first pose; the worm
// reads 3796 and its gait is across encoder zero.
static void place(World &w)
{
    w.servo(1)->shaft = 3000;
    w.servo(2)->shaft = -300;
    w.servo(3)->shaft = 2500;
    set_travel(w);
}
// What the operator does on the page: pick the place that matches the leg.
static bool confirm_worm(World &w, double at)
{
    int32_t places[2];
    int n = rl_turn_candidates(&w.rl, 2, w.now_ms(), places);
    for (int c = 0; c < n; c++)
        if (std::fabs(places[c] - at) < 64) return rl_turn_confirm(&w.rl, 2, places[c], w.now_ms());
    return false;
}

static bool load_plan(rl_plan_t *p);
// The pack's travel table, as gait_store reads it: the plan's windows here.
static rl_plan_t fixture;
static void set_travel(World &w)
{
    const rl_plan_t &p = fixture;
    for (int k = 0; k < p.axes; k++)
            rl_travel_set_pack(&w.rl, p.axis[k].id, p.axis[k].window_lo, p.axis[k].window_hi, p.axis[k].multi_turn, p.axis[k].id == 2, p.axis[k].turn_margin_counts);
}

static bool load_plan(rl_plan_t *p)
{
    FILE *f = fopen("../test/fixtures/leg-plan.bin", "rb");
    if (!f) return false;
    plan_bytes.assign(16384, 0);
    size_t n = fread(plan_bytes.data(), 1, plan_bytes.size(), f);
    fclose(f);
    plan_bytes.resize(n);
    return rl_plan_parse(plan_bytes.data(), n, p);
}

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);
    rl_plan_t plan;
    CHECK(load_plan(&plan), "fixture plan");
    fixture = plan;
    {
        World w({1, 2, 3}, 1);
        place(w);
        w.run(700, true);
        CHECK(w.rl.status_valid && w.rl.status.version == 8, "calibration profile answers (version %u)", w.rl.status.version);
        CHECK(rl_gait_load(&w.rl, &plan, w.now_ms()), "load: %s", w.rl.note);
        CHECK(!rl_gait_start(&w.rl, 1.f, w.now_ms()), "no start before the worm's turn is confirmed");
        CHECK(confirm_worm(w, -300), "worm confirmed at -300: %s", w.rl.note);
        CHECK(rl_gait_start(&w.rl, 1.f, w.now_ms()), "start: %s", w.rl.note);
        w.run(2500, true);
        CHECK(w.rl.status.armed_mask == 0x7 && !w.rl.status.latched, "the RTL armed all three through their windows, the worm's anchored at -300 (armed %x latched %d %s; %s)",
              w.rl.status.armed_mask, w.rl.status.latched, hx_reason_name(w.rl.status.reason), w.rl.gait.result);
        CHECK(w.servo(1)->mode == 0 && w.servo(2)->mode == 1 && w.servo(3)->mode == 0 && w.servo(1)->lock && w.servo(2)->lock && w.servo(3)->lock,
              "position, speed, position mode, relocked");
        w.run(5000, true);
        CHECK(w.rl.gait.phase == RL_GAIT_PLAYING && w.rl.gait.t > 1.f, "playing (%s, %.2f s)", rl_gait_phase_name(w.rl.gait.phase), w.rl.gait.t);
        CHECK(w.rl.refused == 0 && !w.rl.status.latched, "every goal and speed accepted by the RTL (refused %u, latched %d %s)", w.rl.refused, w.rl.status.latched, hx_reason_name(w.rl.status.reason));
        CHECK(w.servo(2)->shaft > 500 && w.rl.servo[1].counts > 500 && std::lround(w.servo(2)->shaft) - w.rl.servo[1].counts < 100,
              "the worm crossed encoder zero into its gait: shaft %.0f, counted %ld", w.servo(2)->shaft, (long)w.rl.servo[1].counts);
        float worst[3] = {0, 0, 0};
        for (uint32_t k = 0; k < w.rl.gait.log_count && k < RL_GAIT_LOG; k++) {
            const rl_gait_sample_t *s = &w.rl.gait.log[k];
            if (s->gait_t > 0.5f) worst[s->id - 1] = fmaxf(worst[s->id - 1], fabsf((float)s->actual - s->command));
        }
        CHECK(worst[0] < 80.f && worst[1] < 80.f && worst[2] < 80.f, "the model shafts followed the command within 80 counts: %.1f %.1f %.1f", worst[0], worst[1], worst[2]);
        printf("gait ok: %.2f s played, %u knee goals, %u worm speeds, %u hip goals through the RTL, worst |actual - command| %.1f / %.1f / %.1f counts, %u timeouts\n",
               w.rl.gait.t, w.servo(1)->goal_writes, w.servo(2)->speed_writes, w.servo(3)->goal_writes, worst[0], worst[1], worst[2], w.rl.timeouts);
        rl_stop(&w.rl);
        w.run(2000, true);
        CHECK(w.rl.gait.phase == RL_GAIT_IDLE && w.rl.gait.stop_verified, "stop verified: %s", w.rl.gait.result);
        CHECK(w.rl.status.latched && w.rl.status.reason == 10 && !w.servo(1)->torque && !w.servo(2)->torque && !w.servo(3)->torque, "host stop, torque off");
        CHECK(w.rl.servo[1].turn_known, "a verified stop keeps the worm's turn");
        printf("gait stop ok: %s\n", w.rl.gait.result);
        // Again, from where it stopped: the RTL re-anchors the worm's window at its counted turns.
        CHECK(rl_gait_start(&w.rl, 1.f, w.now_ms()), "second start: %s", w.rl.note);
        w.run(4000, true);
        CHECK(w.rl.gait.phase == RL_GAIT_PLAYING && w.rl.refused == 0 && !w.rl.status.latched, "playing again (%s, refused %u, %s)",
              rl_gait_phase_name(w.rl.gait.phase), w.rl.refused, w.rl.gait.result);
        rl_stop(&w.rl);
        w.run(2000, true);
        CHECK(w.rl.gait.stop_verified, "second stop verified: %s", w.rl.gait.result);
        printf("gait replay ok\n");
    }
    {
        // The page goes away mid-gait.
        World w({1, 2, 3}, 1);
        place(w);
        w.run(700, true);
        rl_gait_load(&w.rl, &plan, w.now_ms());
        confirm_worm(w, -300);
        rl_gait_start(&w.rl, 1.f, w.now_ms());
        w.run(4000, true);
        CHECK(rl_gait_active(&w.rl) && w.rl.status.armed_mask == 0x7, "running (%s)", w.rl.gait.result);
        w.run(3000, false);
        CHECK(w.rl.gait.phase == RL_GAIT_IDLE && w.rl.gait.stop_verified && w.rl.status.latched, "session loss -> verified stop: %s", w.rl.gait.result);
        printf("gait session loss ok: %s\n", w.rl.gait.result);
    }
    {
        // The ESP32 dies mid-gait: the RTL's own leases stop both motors.
        World w({1, 2, 3}, 1);
        place(w);
        w.run(700, true);
        rl_gait_load(&w.rl, &plan, w.now_ms());
        confirm_worm(w, -300);
        rl_gait_start(&w.rl, 1.f, w.now_ms());
        w.run(4000, true);
        CHECK(w.servo(1)->torque && w.servo(2)->torque && w.servo(3)->torque, "all three energized before the host goes quiet");
        rl_pause(&w.rl, true); // nothing more leaves the ESP32
        w.run(1000, false);
        // Still silent: only the RTL's own telemetry/command leases can have done this.
        CHECK(!w.servo(1)->torque && !w.servo(2)->torque && !w.servo(3)->torque && w.servo(1)->pwm == 0, "the RTL's stop pair turned torque off with the host silent");
        double coast = w.servo(2)->shaft;
        w.run(200, false);
        CHECK(w.servo(2)->shaft == coast, "the worm stopped turning");
        rl_pause(&w.rl, false);
        w.run(300, false);
        CHECK(w.rl.status.latched, "latched (%s)", hx_reason_name(w.rl.status.reason));
        CHECK(!w.rl.servo[1].turn_known, "the engine forgot the worm's turn while it was silent");
        printf("gait host loss ok: torque off within 1 s of the host going silent\n");
    }
    {
        // Hold-to-move on the calibration profile. The knee drooped past its taught 3329: the
        // RTL arms it only in a window stretched to the reading, drive back in passes, drive
        // further out is refused, and the engine zeroes the jog at the window's edge itself.
        World w({1, 2, 3}, 1);
        place(w);
        w.servo(1)->shaft = 3520;
        w.rl.cfg.duty_cap = 1000;
        w.run(700, true);
        CHECK(rl_arm(&w.rl, 1, w.now_ms()), "arm: %s", w.rl.note);
        w.run(400, true);
        CHECK(w.rl.status.armed_mask == 0x1 && w.rl.servo[0].travel.recovering && w.servo(1)->mode == 2 && w.servo(1)->lock,
              "the RTL armed the knee in its recovery window, PWM mode (armed %x, mode %u, %s)", w.rl.status.armed_mask, w.servo(1)->mode, w.rl.note);
        uint32_t refused = w.rl.refused;
        // The engine refuses outward itself; prove the RTL would too by writing it raw.
        CHECK(!rl_hold(&w.rl, 1, 200, w.now_ms()), "outward refused by the engine: %s", w.rl.note);
        w.run(800, true, 1, -300);
        rl_release(&w.rl);
        w.run(100, true);
        CHECK(w.servo(1)->shaft < 3329, "back inside its travel through the RTL: %.0f", w.servo(1)->shaft);
        w.run(1500, true, 1, 300);
        rl_release(&w.rl);
        w.run(100, true);
        CHECK(w.servo(1)->shaft <= 3520 + 120 && w.servo(1)->pwm == 0, "stopped at the stretched edge, drive zero: %.0f (pwm %u)", w.servo(1)->shaft, w.servo(1)->pwm);
        CHECK(w.rl.refused == refused && !w.rl.status.latched, "nothing refused, nothing tripped (refused %u, %s)", w.rl.refused - refused, hx_reason_name(w.rl.status.reason));
        // A raw outward write at the edge: the RTL drops it (no reply).
        uint32_t before = w.rl.timeouts;
        uint8_t pwm[3] = {HX_REG_PWM, 100, 0}, frame[HX_MAX_FRAME];
        rl_pause(&w.rl, true);
        size_t n = hx_packet(frame, 1, HX_WRITE, pwm, 3);
        for (size_t i = 0; i < n; i++) w.host_tx.queue.push_back(frame[i]);
        w.run(30, true);
        uint32_t forwarded = w.forwarded;
        w.run(30, true);
        CHECK(w.servo(1)->pwm == 0 && w.forwarded == forwarded, "the RTL dropped outward drive at the window's edge (pwm %u)", w.servo(1)->pwm);
        rl_pause(&w.rl, false);
        (void)before;
        rl_stop(&w.rl);
        w.run(300, true);
        CHECK(w.rl.status.armed_mask == 0 && !w.servo(1)->torque, "STOP: disarmed, torque off");
        printf("calibrated arm ok: recovered the knee from 3520 to %.0f and held it at the stretched edge, RTL drops outward drive\n", w.servo(1)->shaft);
    }
    printf(failures ? "%d FAILURE(S)\n" : "all calibration cosim checks passed\n", failures);
    return failures ? 1 : 0;
}
