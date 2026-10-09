// The link engine against the real supervised bridge RTL (bridge.v with
// SAFETY_ENABLE, hx_safety.v) under Verilator: real UART bits at 115200 on
// the host side and 1 Mbaud on the servo side, a C++ servo answering on the
// bus, the FPGA's own timeouts in clocks. What the unit tests check against a
// model, this checks against the design that will be on the board.
#include "world.h"

int main(int argc, char **argv) {
    Verilated::commandArgs(argc, argv);
    {
        World w({10, 11, 12});
        w.run(700, true);
        CHECK(w.rl.status_valid && w.rl.status.latched && w.rl.status.reason == 1, "boot: latched, reason boot (valid %d reason %u)", w.rl.status_valid, w.rl.status.reason);
        CHECK(w.rl.servo[6].valid && w.rl.servo[7].valid && w.rl.servo[8].valid, "telemetry from all three over real UART bits");
        CHECK(w.rl.timeouts == 0 && w.rl.unexpected == 0 && w.rl.framer.checksum_errors == 0, "clean link: %u timeouts %u unexpected %u checksum errors", w.rl.timeouts, w.rl.unexpected, w.rl.framer.checksum_errors);
        printf("boot ok: %u frames in, %u sent\n", w.rl.replies, w.rl.sent);

        CHECK(rl_arm(&w.rl, 10, w.now_ms()), "arm 10: %s", w.rl.note);
        w.run(200, true);
        CHECK(w.rl.status_valid && !w.rl.status.latched && w.rl.status.armed_mask == (1 << 6), "the RTL armed 10 and cleared the boot latch (latched %d reason %s armed %03x)", w.rl.status.latched, hx_reason_name(w.rl.status.reason), w.rl.status.armed_mask);
        w.run(500, true, 10, 300);
        Servo *s = w.servo(10);
        CHECK(s->torque == 1 && s->torque_writes >= 1, "torque enabled (%u writes)", s->torque_writes);
        CHECK(s->pwm == 100, "drive clamped to the cap, forwarded by the RTL: %u", s->pwm);
        CHECK(s->pwm_writes >= 5, "rewritten while held: %u", s->pwm_writes);
        w.run(200, true, 10, -40);
        CHECK(s->pwm == (40 | 1 << HX_PWM_DIRECTION_BIT), "reverse: %04x", s->pwm);
        rl_release(&w.rl);
        w.run(20, true);
        CHECK(s->pwm == 0, "zero on release: %u", s->pwm);
        w.run(1500, true);
        CHECK(!w.rl.status.latched && w.rl.status.armed_mask == (1 << 6), "heartbeats kept the RTL's lease for 1.5 s (latched %d reason %s)", w.rl.status.latched, hx_reason_name(w.rl.status.reason));
        CHECK(w.rl.refused == 0, "nothing refused (%u)", w.rl.refused);
        printf("arm/hold/release ok: %u pwm writes, %u frames in, %u timeouts\n", s->pwm_writes, w.rl.replies, w.rl.timeouts);

        // The page vanishes: the engine stops it; the RTL latches host stop.
        w.run(300, true, 10, 50);
        CHECK(s->pwm == 50, "driving again");
        w.run(1300, false);
        CHECK(w.rl.status.latched && w.rl.status.reason == 10, "session loss -> STOP -> latched host stop (latched %d reason %s)", w.rl.status.latched, hx_reason_name(w.rl.status.reason));
        CHECK(s->pwm == 0 && s->torque == 0, "the RTL's stop pair zeroed the drive and disabled torque (pwm %u torque %u)", s->pwm, s->torque);
        printf("session loss ok\n");
    }
    {
        // The ESP32 dies mid-hold: the RTL's own timeouts stop the servo.
        World w({10, 11, 12});
        w.run(700, true);
        rl_arm(&w.rl, 11, w.now_ms());
        w.run(500, true, 11, 60);
        Servo *s = w.servo(11);
        CHECK(s->pwm == 60, "driving 11");
        rl_pause(&w.rl, true); // nothing more leaves the ESP32
        w.run(700, false);
        rl_pause(&w.rl, false);
        w.run(300, false);
        CHECK(w.rl.status.latched && (w.rl.status.reason == 7 || w.rl.status.reason == 8) && w.rl.status.fault_id == 11, "the RTL tripped on its own (latched %d reason %s id %u)", w.rl.status.latched, hx_reason_name(w.rl.status.reason), w.rl.status.fault_id);
        CHECK(s->pwm == 0 && s->torque == 0, "and stopped the servo (pwm %u torque %u)", s->pwm, s->torque);
        printf("host loss ok: %s\n", hx_reason_name(w.rl.status.reason));
    }
    {
        // S2 on the dock while driving.
        World w({10, 11, 12});
        w.run(700, true);
        rl_arm(&w.rl, 12, w.now_ms());
        w.run(500, true, 12, 60);
        w.top.key2 = 1;
        w.run(400, true, 12, 60);
        CHECK(w.rl.status.latched && w.rl.status.reason == 9, "S2 latched (reason %s)", hx_reason_name(w.rl.status.reason));
        CHECK(w.servo(12)->pwm == 0 && !w.rl.jog_id, "stopped; the engine ended the jog");
        printf("stop button ok\n");
    }
    printf(failures ? "%d FAILURE(S)\n" : "all cosim checks passed\n", failures);
    return failures ? 1 : 0;
}
