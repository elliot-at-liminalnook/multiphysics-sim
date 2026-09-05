/* The quadruped trot as a shared library for sim_couple::DynamicCoupler:
 * the same law as quadruped_gait.c, called in-process. Gait parameters are
 * fixed here (stride via simloop_set_stride) because a dylib has no argv.
 *
 *     cc -O2 -std=c99 -shared -fPIC -o libquadruped_gait.dylib quadruped_gait_dl.c -lm
 */
#include <math.h>

static const double PHASE[4] = {0.0, 0.5, 0.5, 0.0};
static double stride = 0.12, period = 0.6, duty = 0.5, lift = 0.05, height = 0.478, l1 = 0.25, l2 = 0.25, kp = 150.0, kd = 6.0, start = 0.5;
static double sample_period = 0.004;
static double previous[4][2];
static int have_previous[4];

void simloop_configure(double stride_m, double period_s, double lift_m, double height_m, double kp_, double kd_, double start_s) {
    stride = stride_m; period = period_s; lift = lift_m; height = height_m; kp = kp_; kd = kd_; start = start_s;
}

int simloop_open(const char *element, double p, int n_sensors, int n_actuators) {
    (void)element;
    sample_period = p;
    for (int i = 0; i < 4; i++) have_previous[i] = 0;
    return (n_sensors == 16 && n_actuators == 8) ? 0 : 1;
}

static void inverse(double x, double y, double *hip, double *knee) {
    double c = (x * x + y * y - l1 * l1 - l2 * l2) / (2.0 * l1 * l2);
    if (c > 1.0) c = 1.0;
    if (c < -1.0) c = -1.0;
    *knee = -acos(c);
    *hip = atan2(y, x) - atan2(l2 * sin(*knee), l1 + l2 * cos(*knee));
}

/* Channels are sorted by name: sensors fl.hip.angle, fl.hip.speed, fl.knee.angle,
 * fl.knee.speed, fr…, rl…, rr…; actuators fl.hip.torque, fl.knee.torque, fr…, rl…, rr…. */
void simloop_sample(double t, const double *s, int ns, double *a, int na) {
    (void)ns; (void)na;
    for (int i = 0; i < 4; i++) {
        double tx, ty, hip, knee, target[2];
        if (t < start) {
            tx = 0.0; ty = -height;
        } else {
            double phase = fmod((t - start) / period + PHASE[i], 1.0);
            if (phase < duty) { double u = phase / duty; tx = stride * (0.5 - u); ty = -height; }
            else { double u = (phase - duty) / (1.0 - duty); tx = stride * (u - 0.5); ty = -height + lift * sin(M_PI * u); }
        }
        inverse(tx, ty, &hip, &knee);
        target[0] = hip; target[1] = knee;
        for (int j = 0; j < 2; j++) {
            double last = have_previous[i] ? previous[i][j] : target[j];
            double rate = (target[j] - last) / sample_period;
            previous[i][j] = target[j];
            double q = s[4 * i + 2 * j], qd = s[4 * i + 2 * j + 1];
            a[2 * i + j] = kp * (target[j] - q) + kd * (rate - qd);
        }
        have_previous[i] = 1;
    }
}
