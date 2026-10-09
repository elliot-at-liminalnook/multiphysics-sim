// The cosim world: the supervised bridge RTL (bridge.v with SAFETY_ENABLE,
// hx_safety.v) under Verilator, real UART bits at 115200 on the host side and
// 1 Mbaud on the servo side, C++ servos answering on the bus, and the link
// engine as the host. Shared by the bench profile (sim_main.cpp) and the
// leg's calibration profile (sim_cal.cpp), which are separate Verilator builds.
#ifndef COSIM_WORLD_H
#define COSIM_WORLD_H
#include "Vtop.h"
#include "verilated.h"
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <deque>
#include <vector>
#include "robot_link.h"

static const int HOST_CLKS = 434, SERVO_CLKS = 50, CLK_PER_MS = 50000;
static int failures = 0;
#define CHECK(cond, ...) do { if (!(cond)) { failures++; printf("  FAIL line %d: ", __LINE__); printf(__VA_ARGS__); printf("\n"); } } while (0)

// Bit-banged UART transmitter: one byte at a time from a queue onto a pin.
struct UartTx {
    int clks_per_bit;
    std::deque<uint8_t> queue;
    int bit = -1, count = 0;
    uint16_t shifter = 0;
    uint8_t level = 1;
    void step() {
        if (bit < 0) {
            if (queue.empty()) { level = 1; return; }
            shifter = (uint16_t)(0x200 | (queue.front() << 1)); // start 0, data, stop 1
            queue.pop_front();
            bit = 0;
            count = 0;
        }
        level = (shifter >> bit) & 1;
        if (++count == clks_per_bit) { count = 0; if (++bit == 10) bit = -1; }
    }
    bool idle() const { return bit < 0 && queue.empty(); }
};

// Receiver sampling mid-bit.
struct UartRx {
    int clks_per_bit;
    int bit = -1, count = 0;
    uint8_t shifter = 0, last = 1;
    bool step(uint8_t level, uint8_t &out) {
        bool got = false;
        if (bit < 0) {
            if (last == 1 && level == 0) { bit = 0; count = clks_per_bit / 2; }
        } else if (--count == 0) {
            count = clks_per_bit;
            if (bit >= 1 && bit <= 8) shifter = (uint8_t)((shifter >> 1) | (level << 7));
            if (bit == 9) { if (level) { out = shifter; got = true; } bit = -1; }
            else bit++;
        }
        last = level;
        return got;
    }
};

// A servo on the bus: answers reads and writes after a turnaround. The
// shaft is in counts with its turns; the encoder reads it modulo 4096. In
// position mode (0) with torque on it moves toward its goal at the goal's
// speed within one turn (never across encoder zero, as the part); in speed
// mode (1) it turns at its speed goal. The mode register changes only while
// unlocked (0x37 = 0).
struct Servo {
    uint8_t id;
    uint8_t mode = 2, torque = 0, error = 0, temperature = 35, voltage = 118, lock = 1;
    uint16_t pwm = 0, position = 1500, goal = 1500, goal_speed = 0;
    int16_t speed_goal = 0;
    double shaft = 1500;
    int16_t speed = 0;
    uint32_t pwm_writes = 0, torque_writes = 0, goal_writes = 0, mode_writes = 0, speed_writes = 0;
    void advance_ms() {
        double before = shaft;
        if (torque && mode == 0) {
            double within = shaft - 4096.0 * std::floor(shaft / 4096.0);
            double step = goal_speed / 1000.0, d = goal - within;
            shaft += d > step ? step : d < -step ? -step : d;
        }
        if (torque && mode == 1) shaft += speed_goal / 1000.0;
        if (torque && mode == 2) shaft += 3.0 * (pwm & 0x3ff) * ((pwm >> HX_PWM_DIRECTION_BIT & 1) ? -1 : 1) / 1000.0; // ~3 counts/s per unit of drive
        long whole = std::lround(shaft);
        position = (uint16_t)(((whole % 4096) + 4096) % 4096);
        speed = (int16_t)((shaft - before) * 1000.0);
    }
};

struct World {
    Vtop top;
    UartTx host_tx{HOST_CLKS}, servo_tx{SERVO_CLKS};
    UartRx host_rx{HOST_CLKS}, servo_rx{SERVO_CLKS};
    hx_framer_t servo_framer;
    std::vector<Servo> servos;
    rl_t rl;
    uint64_t cycle = 0;
    int reply_delay = 0;
    std::vector<uint8_t> reply;
    uint32_t forwarded = 0;
    std::vector<rl_gait_sample_t> log = std::vector<rl_gait_sample_t>(RL_GAIT_LOG);

    World(std::vector<uint8_t> ids, uint8_t first_id = 4) {
        for (uint8_t id : ids) servos.push_back(Servo{id});
        hx_framer_init(&servo_framer);
        rl_config_t cfg = rl_default_config();
        cfg.first_id = first_id;
        cfg.present_mask = 0;
        for (uint8_t id : ids) cfg.present_mask |= (uint16_t)(1u << (id - first_id));
        rl_init(&rl, cfg, [](void *ctx, const uint8_t *b, size_t n) {
            World *w = (World *)ctx;
            for (size_t i = 0; i < n; i++) w->host_tx.queue.push_back(b[i]);
        }, this);
        rl_gait_set_log(&rl, log.data(), RL_GAIT_LOG);
        top.clk = 0;
        top.key2 = 0;
        top.host_rx = 1;
        top.servo_rx = 1;
        top.eval();
    }
    uint32_t now_ms() const { return (uint32_t)(cycle / CLK_PER_MS); }
    Servo *servo(uint8_t id) { for (auto &s : servos) if (s.id == id) return &s; return nullptr; }

    void on_bus_frame(const uint8_t *f, size_t len) {
        if (f[2] == 0xFE) {
            // Broadcast (the supervisor's stop pair: PWM zero, torque off): every servo, no reply.
            if (f[4] == HX_WRITE && len >= 8) for (auto &s : servos) {
                if (f[5] == HX_REG_PWM) { s.pwm = 0; s.pwm_writes++; }
                if (f[5] == HX_REG_TORQUE) { s.torque = f[6]; s.torque_writes++; }
            }
            return;
        }
        Servo *s = servo(f[2]);
        if (!s) return;
        forwarded++;
        uint8_t inst = f[4];
        const uint8_t *p = f + 5;
        size_t n = len - 6;
        uint8_t frame[64];
        size_t rl_len = 0;
        if (inst == HX_READ && n == 2) {
            uint8_t regs[15] = {0};
            if (p[0] == HX_REG_TELEMETRY && p[1] == 15) {
                regs[0] = (uint8_t)s->position; regs[1] = (uint8_t)(s->position >> 8);
                uint16_t sp = (uint16_t)(s->speed < 0 ? (-s->speed) | 0x8000 : s->speed);
                regs[2] = (uint8_t)sp; regs[3] = (uint8_t)(sp >> 8);
                regs[6] = s->voltage; regs[7] = s->temperature; regs[9] = s->error;
                rl_len = hx_packet(frame, s->id, s->error, regs, 15);
            } else if (p[0] == HX_REG_MODE && p[1] == 1) {
                regs[0] = s->mode;
                rl_len = hx_packet(frame, s->id, 0, regs, 1);
            } else if (p[0] == HX_REG_TORQUE && p[1] == 1) {
                regs[0] = s->torque;
                rl_len = hx_packet(frame, s->id, 0, regs, 1);
            } else if (p[0] == HX_REG_PWM && p[1] == 2) {
                regs[0] = (uint8_t)s->pwm; regs[1] = (uint8_t)(s->pwm >> 8);
                rl_len = hx_packet(frame, s->id, 0, regs, 2);
            }
        } else if (inst == HX_WRITE && n >= 2) {
            if (p[0] == HX_REG_TORQUE && n == 2) { s->torque = p[1]; s->torque_writes++; }
            if (p[0] == HX_REG_PWM && n == 3) { s->pwm = (uint16_t)(p[1] | p[2] << 8); s->pwm_writes++; }
            if (p[0] == HX_REG_MODE && n == 2) { if (!s->lock) s->mode = p[1]; s->mode_writes++; }
            if (p[0] == 0x37 && n == 2) s->lock = p[1];
            if (p[0] == 0x2A && n >= 3) { s->goal = (uint16_t)(p[1] | p[2] << 8); s->goal_speed = n == 7 ? (uint16_t)(p[5] | p[6] << 8) : 1000; s->goal_writes++; }
            if (p[0] == 0x2E && n == 3) {
                uint16_t v = (uint16_t)(p[1] | p[2] << 8);
                s->speed_goal = (int16_t)((v & 0x8000) ? -(v & 0x7fff) : (v & 0x7fff));
                s->speed_writes++;
            }
            rl_len = hx_packet(frame, s->id, 0, nullptr, 0);
        }
        if (rl_len) { reply.assign(frame, frame + rl_len); reply_delay = CLK_PER_MS; } // 1 ms turnaround, like the part
    }

    // One 50 MHz clock.
    void tick() {
        host_tx.step();
        top.host_rx = host_tx.level;
        if (reply_delay > 0 && --reply_delay == 0) for (uint8_t b : reply) servo_tx.queue.push_back(b);
        servo_tx.step();
        // One wire: the bus is the AND of every driver (idle high).
        top.servo_rx = top.servo_tx & servo_tx.level;
        top.clk = 1; top.eval();
        top.clk = 0; top.eval();
        cycle++;
        uint8_t b;
        if (host_rx.step(top.host_tx, b)) rl_rx(&rl, &b, 1, now_ms());
        if (servo_rx.step(top.servo_tx & servo_tx.level, b) && servo_tx.idle()) {
            size_t len = hx_framer_push(&servo_framer, b);
            if (len) { uint8_t f[64]; memcpy(f, servo_framer.buf, len); servo_framer.len = 0; on_bus_frame(f, len); }
        }
        if (cycle % 2500 == 0) rl_tick(&rl, now_ms()); // the firmware task runs every 50 us
    }
    // Run `ms`, renewing the session (and a hold) every 50 ms when asked.
    void run(uint32_t ms, bool alive, uint8_t hold = 0, int duty = 0) {
        for (uint32_t k = 0; k < ms; k++) {
            for (int c = 0; c < CLK_PER_MS; c++) tick();
            for (auto &s : servos) s.advance_ms();
            if (alive && now_ms() % 50 == 0) {
                rl_session_alive(&rl, now_ms());
                if (hold) rl_hold(&rl, hold, duty, now_ms());
            }
        }
    }
};

#endif
