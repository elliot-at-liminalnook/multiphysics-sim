// The robot FPGA image (profile 9: IDs 1-16, taught windows, the
// re-addressing permit, the ESP32 link at 1 Mbaud) under Verilator with the
// link engine as host: the leg's three motors play the gait through it at
// the new baud rate, and a spare servo with a factory ID is given a free ID
// through the RTL's permit, nothing else passing.
#include "world.h"
#include <cmath>

static std::vector<uint8_t> plan_bytes;
static rl_plan_t fixture;

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

static void place(World &w)
{
    w.servo(1)->shaft = 3000;
    w.servo(2)->shaft = -300;
    w.servo(3)->shaft = 2500;
    rl_set_present(&w.rl, 0x7);
    for (int k = 0; k < fixture.axes; k++)
        rl_travel_set_pack(&w.rl, fixture.axis[k].id, fixture.axis[k].window_lo, fixture.axis[k].window_hi, fixture.axis[k].multi_turn,
                           fixture.axis[k].id == 2, fixture.axis[k].turn_margin_counts);
}

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);
    CHECK(load_plan(&fixture), "fixture plan");
    {
        World w({1, 2, 3, 20}, 1);
        place(w);
        w.run(700, true);
        CHECK(w.rl.status_valid && w.rl.status.version == 9, "the robot profile answers at 1 Mbaud (version %u)", w.rl.status.version);
        CHECK(rl_gait_load(&w.rl, &fixture, w.now_ms()), "load: %s", w.rl.note);
        int32_t places[2];
        int n = rl_turn_candidates(&w.rl, 2, w.now_ms(), places);
        CHECK(n == 1 && rl_turn_confirm(&w.rl, 2, places[0], w.now_ms()), "worm confirmed: %s", w.rl.note);
        CHECK(rl_gait_start(&w.rl, 1.f, w.now_ms()), "start: %s", w.rl.note);
        w.run(7000, true);
        CHECK(w.rl.gait.phase == RL_GAIT_PLAYING && w.rl.refused == 0 && !w.rl.status.latched && w.rl.timeouts == 0,
              "playing through the 16-axis RTL (%s, refused %u, timeouts %u, %s)", rl_gait_phase_name(w.rl.gait.phase), w.rl.refused, w.rl.timeouts, w.rl.gait.result);
        printf("robot image gait ok: %.2f s played, %u knee goals, %u worm speeds at 1 Mbaud, %u timeouts\n",
               w.rl.gait.t, w.servo(1)->goal_writes, w.servo(2)->speed_writes, w.rl.timeouts);
        rl_stop(&w.rl);
        w.run(2000, true);
        CHECK(w.rl.gait.stop_verified, "stop verified: %s", w.rl.gait.result);
        // A spare with a factory ID outside the range becomes ID 5 through the permit.
        CHECK(rl_readdress(&w.rl, 20, 5, w.now_ms()), "readdress: %s", w.rl.note);
        w.run(800, true);
        CHECK(w.rl.readdress.ok && w.servo(5) && !w.servo(20) && w.servo(5)->lock, "the spare is ID 5 and locked: %s", w.rl.readdress.result);
        printf("robot image readdress ok: %s\n", w.rl.readdress.result);
        // No permit now: an ID write is dropped by the RTL.
        uint8_t q[] = {0x05, 9}, frame[HX_MAX_FRAME];
        rl_pause(&w.rl, true);
        size_t len = hx_packet(frame, 5, HX_WRITE, q, 2);
        for (size_t i = 0; i < len; i++) w.host_tx.queue.push_back(frame[i]);
        w.run(30, true);
        rl_pause(&w.rl, false);
        CHECK(w.servo(5) && !w.servo(9), "without a permit the RTL drops an ID write");
    }
    printf(failures ? "%d FAILURE(S)\n" : "all robot image cosim checks passed\n", failures);
    return failures ? 1 : 0;
}
