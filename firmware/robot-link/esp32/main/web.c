/*
 * The page and its WebSocket. Messages from the page (JSON text frames):
 *
 *   {"t":"claim"}                      take the link (refused while someone else has it)
 *   {"t":"ping"}                       the owner's heartbeat, every <= 500 ms
 *   {"t":"stop"}                       anyone, any time
 *   {"t":"arm","id":10}  {"t":"disarm","id":10}
 *   {"t":"hold","id":10,"duty":-60}    repeat every <= 100 ms while the button is down
 *   {"t":"release"}
 *   {"t":"gait_select","offset":0,"length":2908,"name":"3701-Bayesian-020"}
 *                                      load a plan from the stored pack (offset and
 *                                      length from GET /api/gaits: gaits[i].plan)
 *   {"t":"gait_play","speed":0.5,"supported":true}
 *                                      arm, approach and play (the leg suspended, clear)
 *   {"t":"travel","id":1,"action":"set_lower"|"set_upper"|"open_lower"|"open_upper"|"revert"}
 *                                      (the calibration's poses; set_lo/set_hi/open_lo/open_hi name encoder ends)
 *                                      teach the motor's travel at its current reading
 *                                      (kept on the ESP32 until the Mac promotes it)
 *   {"t":"turn_confirm","id":2,"counts":-300}
 *                                      the operator sees the leg where a multi-turn motor at
 *                                      `counts` puts it (one of gait.axes[k].candidates)
 *   {"t":"drive_select","variants":[{"offset":0,"length":5432,"name":"forward","keys":"w"},...]}
 *                                      load keyboard driving: the pack's drive.variants with
 *                                      their gaits' plans (one must have keys "": stepping in place)
 *   {"t":"drive","keys":"wa"}          the held keys, every <= 200 ms while held; "" or a
 *                                      lapse steps in place, then the clock stops at a cycle start
 *   {"t":"gait_control","playing":false,"speed":0.5}
 *   {"t":"gait_stop"}                  anyone, any time (a STOP that is verified)
 *
 * HTTP: GET /api/state, POST /api/stop, GET /api/gaits (the stored pack's
 * JSON), POST /api/gaits (upload a pack), POST /api/gait {"action":"select"|"drive_select"|"stop",...},
 * GET /api/calibration (each motor's pack and taught travel, for leg_gait_pack
 * --pull-calibration), POST /api/calibration {"action":"set_lo"|…,"id":1,"operator_at_leg":true},
 * POST /api/gait {"action":"confirm_turn","id":2,"counts":-300,"operator_sees_leg":true}
 * (only for someone looking at the leg: it is what makes the worm's window right),
 * GET /api/gait_log (the last run's samples).
 *
 * The server pushes {"t":"state",...} to every socket ten times a second,
 * and answers a refused command with {"t":"refused","why":...}. Closing the
 * owner's socket ends its session (the engine then zeroes and stops).
 */
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include "esp_http_server.h"
#include "esp_log.h"
#include "esp_heap_caps.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "cJSON.h"
#include "link_shared.h"
#include "gait_store.h"
#include "robot_cfg.h"
#include "history.h"
#include "netcfg.h"

static const char *TAG = "robot-web";
static httpd_handle_t server;

/* The selected gait's plan; the engine's plan points into it, so it changes only while no gait is active. */
/* From the heap at web_start: the Wi-Fi transport's DMA pool is taken before app_main,
 * from the same internal RAM a static buffer would shrink. */
#define PLAN_BUF 16384
static uint8_t *plan_buf;
static char gait_name[96];
static uint32_t gait_offset, gait_length;

/* Call with the lock held. */
static bool select_gait(uint32_t offset, uint32_t length, const char *name, char *why, size_t why_len)
{
    uint32_t now = rl_now_ms();
    if (rl_gait_active(&g_rl)) return snprintf(why, why_len, "stop the playing gait first"), false;
    if (!plan_buf || !gait_store_read_plan(offset, length, plan_buf, PLAN_BUF)) return snprintf(why, why_len, "no such plan in the stored pack"), false;
    rl_plan_t plan;
    if (!rl_plan_parse(plan_buf, length, &plan)) return snprintf(why, why_len, "the plan does not parse"), false;
    float check = rl_plan_check(&plan);
    if (!(check < 1.f)) return snprintf(why, why_len, "this firmware's governor differs from the pack's by %.2f counts; rebuild one of them", check), false;
    if (!rl_gait_load(&g_rl, &plan, now)) return snprintf(why, why_len, "%s", g_rl.note), false;
    snprintf(gait_name, sizeof gait_name, "%s", name && *name ? name : "gait");
    gait_offset = offset;
    gait_length = length;
    ESP_LOGI(TAG, "gait selected: %s (%u axes, governor check %.3f counts)", gait_name, plan.axes, check);
    return true;
}
/* ---- keyboard driving ------------------------------------------------------------
 * The pack's drive variants (one steered cycle per held W/A/S/D chord, on one clock):
 * staged from flash without the lock into a fresh buffer, installed under it only
 * while no gait plays, so the plans a drive is playing are never overwritten. */
#define DRIVE_BUF (RL_DRIVE_MAX * 32768) /* a whole robot's plans are about 20 KB each */
static uint8_t *drive_buf;
static int drive_count;
static char drive_chord[RL_DRIVE_MAX][5], drive_name[RL_DRIVE_MAX][32];
typedef struct {
    uint8_t  *buf;
    int       n, step;
    rl_plan_t plans[RL_DRIVE_MAX];
    char      chord[RL_DRIVE_MAX][5], name[RL_DRIVE_MAX][32];
} drive_staged_t;
static drive_staged_t *staging; /* one HTTP server task: one staging at a time; heap, not .bss */

/* {"variants":[{"offset","length","name","keys"}...]} from GET /api/gaits (drive.variants[k].gait -> gaits[g].plan). No lock. */
static bool drive_stage(const cJSON *m, char *why, size_t why_len)
{
    const cJSON *vs = cJSON_GetObjectItem(m, "variants"), *v;
    int n = cJSON_GetArraySize(vs);
    if (!cJSON_IsArray(vs) || n < 1 || n > RL_DRIVE_MAX) return snprintf(why, why_len, "drive_select needs 1 to %d variants (GET /api/gaits: drive.variants, gaits[k].plan)", RL_DRIVE_MAX), false;
    if (!staging && !(staging = calloc(1, sizeof *staging))) return snprintf(why, why_len, "no memory for the drive variants"), false;
    size_t total = 0;
    cJSON_ArrayForEach(v, vs) total += ((size_t)cJSON_GetNumberValue(cJSON_GetObjectItem(v, "length")) + 3u) & ~(size_t)3u;
    if (total == 0 || total > DRIVE_BUF) return snprintf(why, why_len, "the drive variants' plans total %u bytes (at most %d KB)", (unsigned)total, DRIVE_BUF / 1024), false;
    free(staging->buf);
    staging->buf = heap_caps_malloc(total, MALLOC_CAP_SPIRAM);
    if (!staging->buf) staging->buf = malloc(total);
    if (!staging->buf) return snprintf(why, why_len, "no memory for the drive variants (%u bytes)", (unsigned)total), false;
    staging->n = 0;
    staging->step = -1;
    size_t used = 0;
    cJSON_ArrayForEach(v, vs) {
        int k = staging->n;
        const cJSON *off = cJSON_GetObjectItem(v, "offset"), *len = cJSON_GetObjectItem(v, "length"), *name = cJSON_GetObjectItem(v, "name"), *keys = cJSON_GetObjectItem(v, "keys");
        if (!cJSON_IsNumber(off) || !cJSON_IsNumber(len) || !cJSON_IsString(keys) || strlen(keys->valuestring) > 4)
            return snprintf(why, why_len, "variants[%d] needs offset, length and keys", k), false;
        uint32_t length = (uint32_t)len->valuedouble;
        if (used + length > total) return snprintf(why, why_len, "variants[%d] does not fit", k), false;
        if (!gait_store_read_plan((uint32_t)off->valuedouble, length, staging->buf + used, total - used) || !rl_plan_parse(staging->buf + used, length, &staging->plans[k]))
            return snprintf(why, why_len, "variants[%d] is not a plan in the stored pack", k), false;
        float check = rl_plan_check(&staging->plans[k]);
        if (!(check < 1.f)) return snprintf(why, why_len, "variants[%d]: this firmware's governor differs from the pack's by %.2f counts", k, check), false;
        used += (length + 3u) & ~3u;
        snprintf(staging->chord[k], sizeof staging->chord[k], "%s", keys->valuestring);
        snprintf(staging->name[k], sizeof staging->name[k], "%s", cJSON_IsString(name) ? name->valuestring : "variant");
        if (!keys->valuestring[0]) staging->step = k;
        staging->n++;
    }
    if (staging->step < 0) return snprintf(why, why_len, "one variant must have no keys: stepping in place"), false;
    return true;
}

/* Lock held. */
static bool drive_install(char *why, size_t why_len)
{
    uint32_t now = rl_now_ms();
    if (rl_gait_active(&g_rl)) return snprintf(why, why_len, "stop the playing gait first"), false;
    if (!staging || !staging->buf || !rl_drive_load(&g_rl, staging->plans, staging->n, staging->step, now)) return snprintf(why, why_len, "%s", g_rl.note), false;
    free(drive_buf);
    drive_buf = staging->buf;
    staging->buf = NULL;
    drive_count = staging->n;
    memcpy(drive_chord, staging->chord, sizeof drive_chord);
    memcpy(drive_name, staging->name, sizeof drive_name);
    snprintf(gait_name, sizeof gait_name, "keyboard driving");
    gait_offset = gait_length = 0;
    ESP_LOGI(TAG, "keyboard driving: %d variants on a %.3f s cycle", staging->n, g_rl.gait.plan.period_s);
    return true;
}

/* The variant a held chord plays: keys in w a s d order, as the pack writes them. */
static int drive_variant(const char *keys)
{
    char chord[5] = "";
    int n = 0;
    for (const char *c = "wasd"; *c; c++)
        if (strchr(keys, *c)) chord[n++] = *c;
    for (int k = 0; k < drive_count; k++)
        if (!strcmp(drive_chord[k], chord)) return k;
    return -1;
}

/* One motor's taught travel and turn into `o`, for the page and GET /api/calibration (lock held). */
static void add_travel(const rl_t *rl, cJSON *o, int i, uint32_t now)
{
    const rl_servo_t *s = &rl->servo[i];
    const rl_travel_t *t = &s->travel;
    uint8_t id = (uint8_t)(rl->cfg.first_id + i);
    cJSON_AddBoolToObject(o, "known", t->known);
    if (!t->known) return;
    cJSON_AddBoolToObject(o, "multi_turn", t->multi_turn);
    cJSON_AddBoolToObject(o, "reversed", t->reversed);
    cJSON *a = cJSON_AddArrayToObject(o, "pack");
    cJSON_AddItemToArray(a, cJSON_CreateNumber(t->pack_lo));
    cJSON_AddItemToArray(a, cJSON_CreateNumber(t->pack_hi));
    a = cJSON_AddArrayToObject(o, "taught");
    cJSON_AddItemToArray(a, cJSON_CreateNumber(t->lo));
    cJSON_AddItemToArray(a, cJSON_CreateNumber(t->hi));
    a = cJSON_AddArrayToObject(o, "open");
    cJSON_AddItemToArray(a, cJSON_CreateBool(t->open[0]));
    cJSON_AddItemToArray(a, cJSON_CreateBool(t->open[1]));
    cJSON_AddBoolToObject(o, "changed", rl_travel_changed(t));
    bool placed = s->valid && (!t->multi_turn || s->turn_known);
    if (placed) {
        int32_t p = rl_position(rl, id);
        cJSON_AddNumberToObject(o, "position", p);
        cJSON_AddBoolToObject(o, "inside", (t->open[0] || p >= t->lo) && (t->open[1] || p <= t->hi));
    }
    cJSON_AddBoolToObject(o, "armed", t->armed);
    if (t->armed) {
        a = cJSON_AddArrayToObject(o, "armed_window");
        cJSON_AddItemToArray(a, cJSON_CreateNumber(t->armed_lo));
        cJSON_AddItemToArray(a, cJSON_CreateNumber(t->armed_hi));
        cJSON_AddBoolToObject(o, "recovering", t->recovering);
    }
    if (t->multi_turn) {
        cJSON_AddNumberToObject(o, "turn_margin", t->margin);
        int32_t places[2];
        int n = rl_turn_candidates(rl, id, now, places);
        cJSON *c = cJSON_AddArrayToObject(o, "candidates");
        for (int m = 0; m < n; m++) cJSON_AddItemToArray(c, cJSON_CreateNumber(places[m]));
    }
}

/* A taught-travel edit by name (lock held); saves on success. */
static bool travel_action(uint8_t id, const char *action, char *why, size_t why_len)
{
    uint32_t now = rl_now_ms();
    bool ok;
    /* The calibration's pose names: a reversed motor's lower pose is its high-count end. */
    int i = rl_axis(&g_rl, id);
    bool reversed = i >= 0 && g_rl.servo[i].travel.reversed;
    const char *by_pose[][3] = {{"set_lower", "set_lo", "set_hi"}, {"set_upper", "set_hi", "set_lo"}, {"open_lower", "open_lo", "open_hi"}, {"open_upper", "open_hi", "open_lo"}};
    for (size_t k = 0; k < sizeof by_pose / sizeof by_pose[0]; k++)
        if (!strcmp(action, by_pose[k][0])) action = by_pose[k][reversed ? 2 : 1];
    if (!strcmp(action, "set_lo")) ok = rl_travel_teach(&g_rl, id, RL_END_LO, now);
    else if (!strcmp(action, "set_hi")) ok = rl_travel_teach(&g_rl, id, RL_END_HI, now);
    else if (!strcmp(action, "open_lo")) ok = rl_travel_open(&g_rl, id, RL_END_LO, now);
    else if (!strcmp(action, "open_hi")) ok = rl_travel_open(&g_rl, id, RL_END_HI, now);
    else if (!strcmp(action, "revert")) ok = rl_travel_revert(&g_rl, id, now);
    else return snprintf(why, why_len, "action must be set_lower, set_upper, open_lower, open_upper (the calibration's poses), set_lo, set_hi, open_lo, open_hi (encoder ends) or revert"), false;
    if (!ok) return snprintf(why, why_len, "%s", g_rl.note), false;
    robot_cfg_take_travel();
    esp_err_t r = robot_cfg_save();
    if (r != ESP_OK) ESP_LOGE(TAG, "taught travel not saved: %s", esp_err_to_name(r));
    ESP_LOGW(TAG, "motor %u travel %s: %s", id, action, g_rl.note);
    return true;
}

extern const char index_html_start[] asm("_binary_index_html_start");
extern const char index_html_end[] asm("_binary_index_html_end");

static esp_err_t index_get(httpd_req_t *req)
{
    httpd_resp_set_type(req, "text/html; charset=utf-8");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    return httpd_resp_send(req, index_html_start, index_html_end - index_html_start - 1);
}

/* The whole state as JSON, from `rl`: the engine with its lock held, or a snapshot of it. */
static cJSON *state_json(const rl_t *rl, int for_fd)
{
    uint32_t now = rl_now_ms();
    cJSON *o = cJSON_CreateObject();
    cJSON_AddStringToObject(o, "t", "state");
    cJSON_AddNumberToObject(o, "now_ms", now);
    cJSON_AddStringToObject(o, "owner", g_owner_kind == OWNER_NONE ? "none" : g_owner_kind == OWNER_TUNNEL ? "tunnel" : g_owner_kind == OWNER_SCAN ? "bus scan" : "page");
    cJSON_AddNumberToObject(o, "link_baud", g_link_baud);
    cJSON_AddNumberToObject(o, "robot_revision", g_robot.revision);
    cJSON_AddBoolToObject(o, "scanning", g_scan.running);
    if (rl->readdress.active) cJSON_AddStringToObject(o, "readdressing", "1");
    cJSON_AddBoolToObject(o, "you_own", g_owner_kind == OWNER_PAGE && g_owner_fd == for_fd);
    cJSON *link = cJSON_AddObjectToObject(o, "link");
    cJSON_AddBoolToObject(link, "ok", rl_link_ok(rl, now));
    cJSON_AddNumberToObject(link, "sent", rl->sent);
    cJSON_AddNumberToObject(link, "replies", rl->replies);
    cJSON_AddNumberToObject(link, "timeouts", rl->timeouts);
    cJSON_AddNumberToObject(link, "refused", rl->refused);
    cJSON_AddNumberToObject(link, "checksum_errors", rl->framer.checksum_errors);
    cJSON_AddNumberToObject(link, "dropped_bytes", rl->framer.dropped_bytes);
    if (rl->status_valid) {
        cJSON *f = cJSON_AddObjectToObject(o, "fpga");
        cJSON_AddNumberToObject(f, "version", rl->status.version);
        cJSON_AddBoolToObject(f, "latched", rl->status.latched);
        cJSON_AddNumberToObject(f, "reason", rl->status.reason);
        cJSON_AddStringToObject(f, "reason_name", hx_reason_name(rl->status.reason));
        cJSON_AddNumberToObject(f, "fault_id", rl->status.fault_id);
        cJSON_AddNumberToObject(f, "armed_mask", rl->status.armed_mask);
        cJSON_AddNumberToObject(f, "fresh_mask", rl->status.fresh_mask);
        cJSON_AddNumberToObject(f, "age_ms", now - rl->status_at_ms);
        cJSON *lim = cJSON_AddObjectToObject(f, "limits");
        cJSON_AddNumberToObject(lim, "temperature_c", rl->status.temperature_max_c);
        cJSON_AddNumberToObject(lim, "voltage_min_v", rl->status.voltage_min_raw / 10.0);
        cJSON_AddNumberToObject(lim, "voltage_max_v", rl->status.voltage_max_raw / 10.0);
        cJSON_AddNumberToObject(lim, "current_raw", rl->status.current_max_raw);
    }
    cJSON *servos = cJSON_AddArrayToObject(o, "servos");
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (!(rl->cfg.present_mask >> i & 1)) continue;
        const rl_servo_t *s = &rl->servo[i];
        cJSON *j = cJSON_CreateObject();
        cJSON_AddNumberToObject(j, "id", rl->cfg.first_id + i);
        cJSON_AddBoolToObject(j, "armed", rl->status_valid && (rl->status.armed_mask >> i & 1));
        cJSON_AddBoolToObject(j, "fresh", rl->status_valid && (rl->status.fresh_mask >> i & 1));
        cJSON_AddBoolToObject(j, "seen", s->valid);
        if (s->valid) {
            cJSON_AddNumberToObject(j, "age_ms", now - s->at_ms);
            cJSON_AddNumberToObject(j, "position_raw", s->t.position_raw);
            cJSON_AddNumberToObject(j, "position_deg", s->t.position_raw * 360.0 / 4096.0);
            cJSON_AddNumberToObject(j, "position_counts", s->counts);
            cJSON_AddBoolToObject(j, "turn_known", s->turn_known);
            cJSON_AddNumberToObject(j, "position", rl_position(rl, (uint8_t)(rl->cfg.first_id + i)));
            cJSON_AddNumberToObject(j, "speed_counts_s", s->t.speed_counts_s);
            cJSON_AddNumberToObject(j, "load_raw", s->t.load_raw);
            cJSON_AddNumberToObject(j, "voltage_v", s->t.voltage_raw / 10.0);
            cJSON_AddNumberToObject(j, "temperature_c", s->t.temperature_c);
            cJSON_AddNumberToObject(j, "current_raw", s->t.current_raw);
            cJSON_AddNumberToObject(j, "status", s->t.status);
            cJSON_AddNumberToObject(j, "mode", s->mode);
        }
        add_travel(rl, cJSON_AddObjectToObject(j, "travel"), i, now);
        cJSON_AddItemToArray(servos, j);
    }
    if (rl->arm_job.axis >= 0) cJSON_AddNumberToObject(o, "arming_id", rl->cfg.first_id + rl->arm_job.axis);
    cJSON *gait = cJSON_AddObjectToObject(o, "gait");
    const rl_gait_t *g = &rl->gait;
    cJSON_AddStringToObject(gait, "phase", rl_gait_phase_name(g->phase));
    cJSON_AddBoolToObject(gait, "loaded", g->have_plan);
    if (g->have_plan) {
        cJSON_AddStringToObject(gait, "name", gait_name);
        cJSON_AddNumberToObject(gait, "offset", gait_offset);
        cJSON_AddNumberToObject(gait, "length", gait_length);
        cJSON_AddNumberToObject(gait, "period_s", g->plan.period_s);
    }
    cJSON_AddNumberToObject(gait, "t", g->t);
    cJSON_AddBoolToObject(gait, "playing", g->playing);
    cJSON_AddNumberToObject(gait, "speed", g->speed_scale);
    cJSON_AddStringToObject(gait, "result", g->result);
    cJSON_AddBoolToObject(gait, "stop_verified", g->stop_verified);
    cJSON_AddNumberToObject(gait, "samples", g->log_count);
    if (g->drive) {
        cJSON *d = cJSON_AddObjectToObject(gait, "drive");
        int target = (int32_t)(now - (g->drive_cmd_ms + rl->cfg.drive_timeout_ms)) < 0 ? g->drive_target : g->drive_step;
        cJSON_AddStringToObject(d, "target", drive_name[target]);
        cJSON_AddStringToObject(d, "keys", drive_chord[target]);
        cJSON_AddBoolToObject(d, "paused", g->drive_paused);
        cJSON_AddBoolToObject(d, "stopping", g->drive_stop_at > 0.f);
        cJSON *w = cJSON_AddArrayToObject(d, "variants");
        for (int k = 0; k < g->drive_n && k < drive_count; k++) {
            cJSON *v = cJSON_CreateObject();
            cJSON_AddStringToObject(v, "name", drive_name[k]);
            cJSON_AddStringToObject(v, "keys", drive_chord[k]);
            cJSON_AddNumberToObject(v, "weight", roundf(g->drive_w[k] * 1000.f) / 1000.f);
            cJSON_AddItemToArray(w, v);
        }
    }
    if (g->phase == RL_GAIT_PREPARE) cJSON_AddNumberToObject(gait, "preparing_id", g->plan.axis[g->axis].id);
    cJSON *axes = cJSON_AddArrayToObject(gait, "axes");
    for (int k = 0; g->have_plan && k < g->plan.axes; k++) {
        cJSON *a = cJSON_CreateObject();
        cJSON_AddNumberToObject(a, "id", g->plan.axis[k].id);
        cJSON_AddNumberToObject(a, "goal", g->goal[k]);
        cJSON_AddNumberToObject(a, "desired", g->desired[k]);
        cJSON_AddNumberToObject(a, "window_lo", g->plan.axis[k].window_lo);
        cJSON_AddNumberToObject(a, "window_hi", g->plan.axis[k].window_hi);
        cJSON_AddStringToObject(a, "drive", g->plan.axis[k].drive == RL_DRIVE_SPEED ? "servo_speed" : "servo_position");
        cJSON_AddBoolToObject(a, "multi_turn", g->plan.axis[k].multi_turn);
        if (g->plan.axis[k].multi_turn) {
            const rl_servo_t *s = &rl->servo[rl_axis(rl, g->plan.axis[k].id)];
            bool known = s->turn_known && s->turn_lo == g->plan.axis[k].window_lo && s->turn_hi == g->plan.axis[k].window_hi;
            cJSON_AddBoolToObject(a, "turn_known", known);
            if (known) cJSON_AddNumberToObject(a, "position_counts", s->counts);
        }
        cJSON_AddItemToArray(axes, a);
    }
    gait_store_info_t pack = gait_store_info();
    cJSON *pj = cJSON_AddObjectToObject(o, "pack");
    cJSON_AddBoolToObject(pj, "valid", pack.valid);
    if (pack.valid) {
        cJSON_AddNumberToObject(pj, "json_bytes", pack.json_len);
        cJSON_AddNumberToObject(pj, "plan_bytes", pack.bin_len);
        cJSON_AddStringToObject(pj, "sha256", pack.sha256);
    }
    cJSON *jog = cJSON_AddObjectToObject(o, "jog");
    cJSON_AddNumberToObject(jog, "id", rl->jog_id);
    cJSON_AddNumberToObject(jog, "duty", rl->jog_duty);
    cJSON_AddNumberToObject(jog, "out", roundf(rl->jog_out));
    cJSON_AddBoolToObject(jog, "coasting", rl->jog_coasting);
    cJSON_AddNumberToObject(jog, "longest_gap_ms", rl->jog_gap_max_ms);
    cJSON_AddNumberToObject(o, "duty_cap", rl->cfg.duty_cap);
    if (rl->note[0]) {
        cJSON_AddStringToObject(o, "note", rl->note);
        cJSON_AddNumberToObject(o, "note_age_ms", now - rl->note_at_ms);
    }
    return o;
}

static esp_err_t send_json(httpd_req_t *req, cJSON *o)
{
    char *text = cJSON_PrintUnformatted(o);
    cJSON_Delete(o);
    httpd_resp_set_type(req, "application/json");
    httpd_resp_set_hdr(req, "Cache-Control", "no-store");
    esp_err_t r = httpd_resp_send(req, text, HTTPD_RESP_USE_STRLEN);
    free(text);
    return r;
}

static esp_err_t state_get(httpd_req_t *req)
{
    RL_LOCK("state_get");
    cJSON *o = state_json(&g_rl, -1);
    xSemaphoreGive(g_rl_lock);
    return send_json(req, o);
}

/* STOP works without a WebSocket, from anything that can POST. */
static esp_err_t stop_post(httpd_req_t *req)
{
    RL_LOCK("stop_post");
    link_stop_now();
    cJSON *o = state_json(&g_rl, -1);
    xSemaphoreGive(g_rl_lock);
    ESP_LOGW(TAG, "STOP over HTTP");
    return send_json(req, o);
}

static esp_err_t gaits_get(httpd_req_t *req)
{
    return gait_store_send_json(req);
}

static esp_err_t gaits_post(httpd_req_t *req)
{
    return gait_store_upload(req);
}

/* POST /api/gait: select a plan or stop, from anything that can POST.
 * Playing needs the owner's live session, so it is the page's (WebSocket) alone. */
static esp_err_t gait_post(httpd_req_t *req)
{
    /* Up to 4 KB: a drive_select names every variant. */
    size_t cap = 4096, got = 0;
    char *body = req->content_len <= cap ? malloc(req->content_len + 1) : NULL;
    while (body && got < req->content_len) {
        int r = httpd_req_recv(req, body + got, req->content_len - got);
        if (r == HTTPD_SOCK_ERR_TIMEOUT) continue;
        if (r <= 0) break;
        got += (size_t)r;
    }
    cJSON *m = body && got ? cJSON_ParseWithLength(body, got) : NULL;
    free(body);
    const cJSON *action = m ? cJSON_GetObjectItem(m, "action") : NULL;
    char why[200] = "";
    bool ok = true;
    /* Flash reads outside the lock. */
    bool staged_ok = cJSON_IsString(action) && !strcmp(action->valuestring, "drive_select") && drive_stage(m, why, sizeof why);
    RL_LOCK("gait_post");
    if (cJSON_IsString(action) && !strcmp(action->valuestring, "stop")) {
        rl_gait_stop(&g_rl, "stopped over HTTP");
        link_stop_now();
    } else if (cJSON_IsString(action) && !strcmp(action->valuestring, "select")) {
        const cJSON *off = cJSON_GetObjectItem(m, "offset"), *len = cJSON_GetObjectItem(m, "length"), *name = cJSON_GetObjectItem(m, "name");
        ok = cJSON_IsNumber(off) && cJSON_IsNumber(len)
             && select_gait((uint32_t)off->valuedouble, (uint32_t)len->valuedouble, cJSON_IsString(name) ? name->valuestring : "", why, sizeof why);
        if (!cJSON_IsNumber(off) || !cJSON_IsNumber(len)) snprintf(why, sizeof why, "select needs offset and length from GET /api/gaits");
    } else if (cJSON_IsString(action) && !strcmp(action->valuestring, "confirm_turn")) {
        const cJSON *id = cJSON_GetObjectItem(m, "id"), *counts = cJSON_GetObjectItem(m, "counts");
        if (!cJSON_IsTrue(cJSON_GetObjectItem(m, "operator_sees_leg"))) {
            ok = false;
            snprintf(why, sizeof why, "confirm_turn needs operator_sees_leg: true (someone looking at the leg picks where it is)");
        } else if (!cJSON_IsNumber(id) || !cJSON_IsNumber(counts)) {
            ok = false;
            snprintf(why, sizeof why, "confirm_turn needs id and counts (one of gait.axes[k].candidates in GET /api/state)");
        } else if (!(ok = rl_turn_confirm(&g_rl, (uint8_t)id->valueint, (int32_t)counts->valuedouble, rl_now_ms()))) {
            snprintf(why, sizeof why, "%s", g_rl.note);
        }
    } else if (cJSON_IsString(action) && !strcmp(action->valuestring, "drive_select")) {
        ok = staged_ok && drive_install(why, sizeof why);
    } else if (cJSON_IsString(action) && !strcmp(action->valuestring, "drive")) {
        ok = false;
        snprintf(why, sizeof why, "drive from the page: the front end holding the session steers (one owner per motion session)");
    } else {
        ok = false;
        snprintf(why, sizeof why, "action must be select, drive_select, confirm_turn or stop; play and drive from the page (it holds the session)");
    }
    cJSON *o = state_json(&g_rl, -1);
    xSemaphoreGive(g_rl_lock);
    cJSON_Delete(m);
    if (!ok) {
        cJSON_AddStringToObject(o, "refused", why);
        httpd_resp_set_status(req, "409 Conflict");
    }
    return send_json(req, o);
}

/* GET /api/calibration: each motor's travel from the pack and as taught on this page. */
static esp_err_t calibration_get(httpd_req_t *req)
{
    RL_LOCK("calibration_get");
    uint32_t now = rl_now_ms();
    cJSON *o = cJSON_CreateObject();
    gait_store_info_t pack = gait_store_info();
    cJSON_AddStringToObject(o, "pack_sha256", pack.valid ? pack.sha256 : "");
    cJSON *motors = cJSON_AddArrayToObject(o, "motors");
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        if (!(g_rl.cfg.present_mask >> i & 1) || !g_rl.servo[i].travel.known) continue;
        cJSON *j = cJSON_CreateObject();
        cJSON_AddNumberToObject(j, "id", g_rl.cfg.first_id + i);
        add_travel(&g_rl, j, i, now);
        cJSON_AddItemToArray(motors, j);
    }
    xSemaphoreGive(g_rl_lock);
    return send_json(req, o);
}

/* POST /api/calibration: a taught-travel edit, only for someone at the leg. */
static esp_err_t calibration_post(httpd_req_t *req)
{
    char body[256] = "";
    int n = req->content_len < sizeof body - 1 ? (int)req->content_len : (int)sizeof body - 1;
    int got = n > 0 ? httpd_req_recv(req, body, n) : 0;
    cJSON *m = got > 0 ? cJSON_ParseWithLength(body, (size_t)got) : NULL;
    const cJSON *action = m ? cJSON_GetObjectItem(m, "action") : NULL, *id = m ? cJSON_GetObjectItem(m, "id") : NULL;
    char why[200] = "";
    bool ok;
    RL_LOCK("calibration_post");
    if (!cJSON_IsTrue(m ? cJSON_GetObjectItem(m, "operator_at_leg") : NULL)) {
        ok = false;
        snprintf(why, sizeof why, "teaching needs operator_at_leg: true (someone at the leg sets each pose where the motor is)");
    } else if (!cJSON_IsString(action) || !cJSON_IsNumber(id)) {
        ok = false;
        snprintf(why, sizeof why, "needs action (set_lo, set_hi, open_lo, open_hi, revert) and id");
    } else ok = travel_action((uint8_t)id->valueint, action->valuestring, why, sizeof why);
    cJSON *o = state_json(&g_rl, -1);
    xSemaphoreGive(g_rl_lock);
    cJSON_Delete(m);
    if (!ok) {
        cJSON_AddStringToObject(o, "refused", why);
        httpd_resp_set_status(req, "409 Conflict");
    }
    return send_json(req, o);
}

/* ---- the robot configuration ---------------------------------------------- */

static bool busy_for_edit(char *why, size_t why_len)
{
    const char *no = g_rl.status_valid && g_rl.status.armed_mask ? "stop first: a motor is armed"
                   : rl_gait_active(&g_rl) ? "stop the gait first"
                   : g_rl.arm_job.axis >= 0 ? "a motor is being armed"
                   : rl_readdress_active(&g_rl) ? "a servo is being re-addressed" : NULL;
    if (no) snprintf(why, why_len, "%s", no);
    return no != NULL;
}

/* Replace the configuration with `j` after validating it whole; applied and saved. Lock held. */
static bool robot_replace(const cJSON *j, char *why, size_t why_len)
{
    rc_robot_t r;
    if (!robot_cfg_from_json(j, &r, why, why_len)) return false;
    r.revision = g_robot.revision;
    g_robot = r;
    robot_cfg_apply();
    esp_err_t e = robot_cfg_save();
    if (e != ESP_OK) return snprintf(why, why_len, "applied, but not saved: %s", esp_err_to_name(e)), false;
    return true;
}

static cJSON *slot_of_json(cJSON *cfg, int k)
{
    return cJSON_GetArrayItem(cJSON_GetObjectItem(cfg, "slots"), k);
}

/* One robot action by name (the page's WebSocket and POST /api/robot). Lock held. */
static bool robot_action(const cJSON *m, char *why, size_t why_len)
{
    const cJSON *action = cJSON_GetObjectItem(m, "action");
    const char *a = cJSON_IsString(action) ? action->valuestring : "";
    uint32_t now = rl_now_ms();
    if (!strcmp(a, "scan")) return link_scan_request(why, why_len);
    if (!strcmp(a, "readdress")) {
        const cJSON *from = cJSON_GetObjectItem(m, "from"), *to = cJSON_GetObjectItem(m, "to");
        if (!cJSON_IsNumber(from) || !cJSON_IsNumber(to)) return snprintf(why, why_len, "readdress needs from and to"), false;
        if (!rl_readdress(&g_rl, (uint8_t)from->valueint, (uint8_t)to->valueint, now)) return snprintf(why, why_len, "%s", g_rl.note), false;
        return true;
    }
    if (busy_for_edit(why, why_len)) return false;
    const cJSON *slot = cJSON_GetObjectItem(m, "slot");
    int k = cJSON_IsNumber(slot) ? slot->valueint : -1;
    if (!strcmp(a, "replace")) return robot_replace(cJSON_GetObjectItem(m, "config"), why, why_len);
    cJSON *cfg = robot_cfg_json(&g_robot);
    cJSON *s = k >= 0 && k < g_robot.slots ? slot_of_json(cfg, k) : NULL;
    bool ok = true;
    if (!strcmp(a, "assign")) {
        /* A different motor in this slot: its alignment and poses are the old motor's, so they go. */
        const cJSON *id = cJSON_GetObjectItem(m, "id");
        if (!s || !cJSON_IsNumber(id)) { snprintf(why, why_len, "assign needs slot and id"); ok = false; }
        else {
            int was = g_robot.slot[k].id;
            cJSON_ReplaceItemInObject(s, "id", cJSON_CreateNumber(id->valueint));
            if (was != id->valueint && !cJSON_IsTrue(cJSON_GetObjectItem(m, "keep_calibration"))) {
                cJSON_ReplaceItemInObject(s, "alignment", cJSON_CreateNull());
                cJSON_ReplaceItemInObject(s, "poses", cJSON_CreateNull());
            }
        }
    } else if (!strcmp(a, "polarity")) {
        const cJSON *v = cJSON_GetObjectItem(m, "polarity");
        if (!s || !cJSON_IsNumber(v)) { snprintf(why, why_len, "polarity needs slot and polarity"); ok = false; }
        else cJSON_ReplaceItemInObject(s, "polarity", cJSON_CreateNumber(v->valueint));
    } else if (!strcmp(a, "enable")) {
        const cJSON *v = cJSON_GetObjectItem(m, "enabled");
        if (!s || !cJSON_IsBool(v)) { snprintf(why, why_len, "enable needs slot and enabled"); ok = false; }
        else cJSON_ReplaceItemInObject(s, "enabled", cJSON_CreateBool(cJSON_IsTrue(v)));
    } else if (!strcmp(a, "align")) {
        /* The motor's reading now is the joint angle `rad` (the CAD pose the leg is held in). */
        const cJSON *rad = cJSON_GetObjectItem(m, "rad");
        int i = s ? rl_axis(&g_rl, g_robot.slot[k].id) : -1;
        const rl_servo_t *sv = i >= 0 ? &g_rl.servo[i] : NULL;
        if (!s || !cJSON_IsNumber(rad)) { snprintf(why, why_len, "align needs slot and rad"); ok = false; }
        else if (!sv || !sv->valid || now - sv->at_ms > 300) { snprintf(why, why_len, "the slot's motor has no fresh reading"); ok = false; }
        else if (g_robot.slot[k].multi_turn && !sv->turn_known) { snprintf(why, why_len, "confirm the motor's turn first"); ok = false; }
        else {
            cJSON *al = cJSON_CreateObject();
            cJSON_AddNumberToObject(al, "counts", rl_position(&g_rl, g_robot.slot[k].id));
            cJSON_AddNumberToObject(al, "rad", rad->valuedouble);
            cJSON_ReplaceItemInObject(s, "alignment", al);
        }
    } else if (!strcmp(a, "limits")) {
        const cJSON *v = cJSON_GetObjectItem(m, "limits");
        if (!cJSON_IsObject(v)) { snprintf(why, why_len, "limits needs limits"); ok = false; }
        else cJSON_ReplaceItemInObject(cfg, "limits", cJSON_Duplicate(v, 1));
    } else if (!strcmp(a, "name")) {
        const cJSON *v = cJSON_GetObjectItem(m, "name");
        if (!cJSON_IsString(v)) { snprintf(why, why_len, "name needs name"); ok = false; }
        else cJSON_ReplaceItemInObject(cfg, "name", cJSON_CreateString(v->valuestring));
    } else {
        snprintf(why, why_len, "action must be scan, readdress, assign, polarity, enable, align, limits, name or replace");
        ok = false;
    }
    if (ok) {
        ok = robot_replace(cfg, why, why_len);
        if (ok) ESP_LOGW(TAG, "robot configuration: %s (revision %lu)", a, (unsigned long)g_robot.revision);
    }
    cJSON_Delete(cfg);
    return ok;
}

/* GET /api/robot: the configuration, the FPGA profile, the last bus scan and every motor seen. */
static cJSON *robot_json(void)
{
    uint32_t now = rl_now_ms();
    cJSON *o = cJSON_CreateObject();
    cJSON_AddItemToObject(o, "config", robot_cfg_json(&g_robot));
    cJSON *prof = cJSON_AddObjectToObject(o, "fpga");
    int v = g_rl.status_valid ? g_rl.status.version : 0;
    cJSON_AddNumberToObject(prof, "version", v);
    cJSON_AddStringToObject(prof, "image", v == 9 ? "robot (IDs 1-16, re-addressing)" : v == 8 ? "leg calibration (IDs 1-3)" : v == 1 ? "bench (IDs 4-12)" : v ? "older calibration" : "not answering");
    cJSON_AddNumberToObject(prof, "first_id", 1);
    cJSON_AddNumberToObject(prof, "count", v == 9 ? 16 : v == 1 ? 9 : 3);
    cJSON_AddNumberToObject(o, "link_baud", g_link_baud);
    cJSON *scan = cJSON_AddObjectToObject(o, "scan");
    cJSON_AddBoolToObject(scan, "running", g_scan.running);
    cJSON_AddBoolToObject(scan, "done", g_scan.done);
    if (g_scan.done) {
        cJSON_AddNumberToObject(scan, "age_ms", now - g_scan.at_ms);
        cJSON_AddNumberToObject(scan, "took_ms", g_scan.took_ms);
    }
    cJSON *ids = cJSON_AddArrayToObject(scan, "ids"), *errs = cJSON_AddArrayToObject(scan, "errors");
    for (int id = 0; id < 254; id++) {
        if (g_scan.answered[id / 32] >> (id % 32) & 1) cJSON_AddItemToArray(ids, cJSON_CreateNumber(id));
        if (g_scan.errors[id / 32] >> (id % 32) & 1) cJSON_AddItemToArray(errs, cJSON_CreateNumber(id));
    }
    cJSON *rd = cJSON_AddObjectToObject(o, "readdress");
    cJSON_AddBoolToObject(rd, "active", g_rl.readdress.active);
    cJSON_AddStringToObject(rd, "result", g_rl.readdress.result);
    cJSON_AddBoolToObject(rd, "ok", g_rl.readdress.ok);
    return o;
}

static esp_err_t robot_get(httpd_req_t *req)
{
    RL_LOCK("robot_get");
    cJSON *o = robot_json();
    xSemaphoreGive(g_rl_lock);
    return send_json(req, o);
}

/* POST /api/robot {"action": ..., ...}: as the page's {"t":"robot"}; edits need operator_at_robot. */
static esp_err_t robot_post(httpd_req_t *req)
{
    size_t cap = req->content_len + 1;
    if (cap > 16384) return httpd_resp_send_err(req, HTTPD_400_BAD_REQUEST, "too large");
    char *body = calloc(1, cap);
    int got = 0;
    while (body && got < (int)req->content_len) {
        int r = httpd_req_recv(req, body + got, req->content_len - got);
        if (r <= 0) break;
        got += r;
    }
    cJSON *m = body ? cJSON_ParseWithLength(body, (size_t)got) : NULL;
    free(body);
    const cJSON *action = m ? cJSON_GetObjectItem(m, "action") : NULL;
    char why[200] = "";
    bool ok;
    RL_LOCK("robot_post");
    bool harmless = cJSON_IsString(action) && !strcmp(action->valuestring, "scan");
    if (!m) { ok = false; snprintf(why, sizeof why, "the body must be JSON"); }
    else if (!harmless && !cJSON_IsTrue(cJSON_GetObjectItem(m, "operator_at_robot"))) {
        ok = false;
        snprintf(why, sizeof why, "configuration changes need operator_at_robot: true (someone at the robot)");
    } else ok = robot_action(m, why, sizeof why);
    cJSON *o = robot_json();
    xSemaphoreGive(g_rl_lock);
    cJSON_Delete(m);
    if (!ok) {
        cJSON_AddStringToObject(o, "refused", why);
        httpd_resp_set_status(req, "409 Conflict");
    }
    return send_json(req, o);
}

/* GET /api/gait_log: the last run's samples, oldest first, for the Mac to keep. */
static esp_err_t gait_log_get(httpd_req_t *req)
{
    RL_LOCK("gait_log_get");
    const rl_gait_t *g = &g_rl.gait;
    uint32_t total = g->log_count, n = total < g->log_cap ? total : g->log_cap, first = total - n;
    rl_gait_sample_t *copy = malloc(n ? n * sizeof *copy : 1);
    for (uint32_t k = 0; copy && k < n; k++) copy[k] = g->log[(first + k) % g->log_cap];
    char head[640];
    snprintf(head, sizeof head, "{\"gait\":\"%s\",\"phase\":\"%s\",\"result\":\"%s\",\"stop_verified\":%s,\"speed\":%.3f,\"samples_total\":%lu,"
             "\"columns\":[\"ms\",\"gait_t\",\"id\",\"command_counts\",\"desired_counts\",\"actual_counts\"],\"samples\":[",
             gait_name, rl_gait_phase_name(g->phase), g->result, g->stop_verified ? "true" : "false", g->speed_scale, (unsigned long)total);
    xSemaphoreGive(g_rl_lock);
    if (!copy) return httpd_resp_send_500(req);
    httpd_resp_set_type(req, "application/json");
    httpd_resp_send_chunk(req, head, HTTPD_RESP_USE_STRLEN);
    char row[96];
    for (uint32_t k = 0; k < n; k++) {
        const rl_gait_sample_t *s = &copy[k];
        snprintf(row, sizeof row, "%s[%lu,%.4f,%u,%.1f,%.1f,%ld]", k ? "," : "", (unsigned long)s->ms, s->gait_t, s->id, s->command, s->desired, (long)s->actual);
        if (httpd_resp_send_chunk(req, row, HTTPD_RESP_USE_STRLEN) != ESP_OK) { free(copy); return ESP_FAIL; }
    }
    free(copy);
    httpd_resp_send_chunk(req, "]}", 2);
    return httpd_resp_send_chunk(req, NULL, 0);
}

/* The owner's socket closed: its session ends, whatever it held stops. */
static void on_session_closed(void *ctx)
{
    int fd = (int)(intptr_t)ctx;
    RL_LOCK("on_session_closed");
    if (g_owner_kind == OWNER_PAGE && g_owner_fd == fd) {
        rl_session_end(&g_rl);
        g_owner_kind = OWNER_NONE;
        g_owner_fd = -1;
        ESP_LOGW(TAG, "owner socket %d closed: session ended", fd);
    }
    xSemaphoreGive(g_rl_lock);
}

static void ws_reply(httpd_req_t *req, cJSON *o)
{
    char *text = cJSON_PrintUnformatted(o);
    cJSON_Delete(o);
    httpd_ws_frame_t frame = {.type = HTTPD_WS_TYPE_TEXT, .payload = (uint8_t *)text, .len = strlen(text)};
    httpd_ws_send_frame(req, &frame);
    free(text);
}

static void refused(httpd_req_t *req, const char *why)
{
    cJSON *o = cJSON_CreateObject();
    cJSON_AddStringToObject(o, "t", "refused");
    cJSON_AddStringToObject(o, "why", why);
    ws_reply(req, o);
}

static esp_err_t ws_handler(httpd_req_t *req)
{
    int fd = httpd_req_to_sockfd(req);
    if (req->method == HTTP_GET) {
        /* Handshake: remember the socket so its close is seen. */
        httpd_sess_set_ctx(req->handle, fd, (void *)(intptr_t)fd, on_session_closed);
        return ESP_OK;
    }
    httpd_ws_frame_t frame = {.type = HTTPD_WS_TYPE_TEXT};
    esp_err_t r = httpd_ws_recv_frame(req, &frame, 0);
    if (r != ESP_OK) return r;
    if (frame.len == 0 || frame.len > 8192) return ESP_OK; /* a whole robot configuration fits */
    uint8_t *buf = calloc(1, frame.len + 1);
    frame.payload = buf;
    r = httpd_ws_recv_frame(req, &frame, frame.len);
    if (r != ESP_OK) { free(buf); return r; }
    cJSON *m = cJSON_ParseWithLength((char *)buf, frame.len);
    free(buf);
    if (!m) return ESP_OK;
    const cJSON *t = cJSON_GetObjectItem(m, "t");
    const char *type = cJSON_IsString(t) ? t->valuestring : "";
    const cJSON *idj = cJSON_GetObjectItem(m, "id");
    const cJSON *dutyj = cJSON_GetObjectItem(m, "duty");
    int id = cJSON_IsNumber(idj) ? idj->valueint : 0;
    int duty = cJSON_IsNumber(dutyj) ? dutyj->valueint : 0;
    uint32_t now = rl_now_ms();
    char why[200] = "";
    bool ok = true;

    /* Flash reads outside the lock: the link task must not wait on them. */
    bool staged_ok = !strcmp(type, "drive_select") && drive_stage(m, why, sizeof why);

    RL_LOCK("ws_handler");
    bool owner = g_owner_kind == OWNER_PAGE && g_owner_fd == fd;
    if (!strcmp(type, "stop") || !strcmp(type, "gait_stop")) {
        if (!strcmp(type, "gait_stop")) rl_gait_stop(&g_rl, "stopped from the page");
        link_stop_now();
        ESP_LOGW(TAG, "STOP from socket %d", fd);
    } else if (!strcmp(type, "claim")) {
        if (g_owner_kind == OWNER_NONE || owner) {
            g_owner_kind = OWNER_PAGE;
            g_owner_fd = fd;
            rl_session_alive(&g_rl, now);
            ESP_LOGI(TAG, "socket %d owns the link", fd);
        } else {
            ok = false;
            snprintf(why, sizeof why, "the %s already owns the link", g_owner_kind == OWNER_TUNNEL ? "tunnel" : g_owner_kind == OWNER_SCAN ? "bus scan (a few seconds)" : "other page");
        }
    } else if (!owner) {
        ok = false;
        snprintf(why, sizeof why, "claim the link first");
    } else if (!strcmp(type, "ping")) {
        rl_session_alive(&g_rl, now);
    } else if (!strcmp(type, "arm")) {
        rl_session_alive(&g_rl, now);
        ok = rl_arm(&g_rl, (uint8_t)id, now);
    } else if (!strcmp(type, "disarm")) {
        rl_session_alive(&g_rl, now);
        ok = rl_disarm(&g_rl, (uint8_t)id, now);
    } else if (!strcmp(type, "hold")) {
        rl_session_alive(&g_rl, now);
        ok = rl_hold(&g_rl, (uint8_t)id, duty, now);
    } else if (!strcmp(type, "release")) {
        rl_session_alive(&g_rl, now);
        rl_release(&g_rl);
    } else if (!strcmp(type, "gait_select")) {
        rl_session_alive(&g_rl, now);
        const cJSON *off = cJSON_GetObjectItem(m, "offset"), *len = cJSON_GetObjectItem(m, "length"), *name = cJSON_GetObjectItem(m, "name");
        ok = cJSON_IsNumber(off) && cJSON_IsNumber(len)
             && select_gait((uint32_t)off->valuedouble, (uint32_t)len->valuedouble, cJSON_IsString(name) ? name->valuestring : "", why, sizeof why);
    } else if (!strcmp(type, "gait_play")) {
        rl_session_alive(&g_rl, now);
        const cJSON *sp = cJSON_GetObjectItem(m, "speed"), *sup = cJSON_GetObjectItem(m, "supported");
        if (!cJSON_IsTrue(sup)) {
            ok = false;
            snprintf(why, sizeof why, "confirm the leg is suspended with clear space around every joint");
        } else {
            ok = rl_gait_start(&g_rl, cJSON_IsNumber(sp) ? (float)sp->valuedouble : 1.f, now);
            if (ok) ESP_LOGW(TAG, "gait %s started from socket %d", gait_name, fd);
        }
    } else if (!strcmp(type, "travel")) {
        rl_session_alive(&g_rl, now);
        const cJSON *action = cJSON_GetObjectItem(m, "action");
        ok = cJSON_IsString(action) && travel_action((uint8_t)id, action->valuestring, why, sizeof why);
        if (!cJSON_IsString(action)) snprintf(why, sizeof why, "travel needs an action");
    } else if (!strcmp(type, "network")) {
        rl_session_alive(&g_rl, now);
        ok = net_action(m, why, sizeof why);
    } else if (!strcmp(type, "robot")) {
        rl_session_alive(&g_rl, now);
        ok = robot_action(m, why, sizeof why);
    } else if (!strcmp(type, "turn_confirm")) {
        rl_session_alive(&g_rl, now);
        const cJSON *counts = cJSON_GetObjectItem(m, "counts");
        ok = cJSON_IsNumber(counts) && rl_turn_confirm(&g_rl, (uint8_t)id, (int32_t)counts->valuedouble, now);
        if (ok) ESP_LOGW(TAG, "motor %d's turn confirmed at %ld from socket %d", id, (long)g_rl.servo[rl_axis(&g_rl, (uint8_t)id)].counts, fd);
    } else if (!strcmp(type, "drive_select")) {
        rl_session_alive(&g_rl, now);
        ok = staged_ok && drive_install(why, sizeof why);
    } else if (!strcmp(type, "drive")) {
        /* The held keys, renewed every <= 200 ms while held; none steps in place. */
        rl_session_alive(&g_rl, now);
        const cJSON *keys = cJSON_GetObjectItem(m, "keys");
        if (!g_rl.gait.drive) {
            ok = false;
            snprintf(why, sizeof why, "load keyboard driving first (drive_select)");
        } else {
            rl_drive_command(&g_rl, drive_variant(cJSON_IsString(keys) ? keys->valuestring : ""), now);
        }
    } else if (!strcmp(type, "gait_control")) {
        rl_session_alive(&g_rl, now);
        const cJSON *pl = cJSON_GetObjectItem(m, "playing"), *sp = cJSON_GetObjectItem(m, "speed");
        rl_gait_control(&g_rl, cJSON_IsBool(pl) ? cJSON_IsTrue(pl) : g_rl.gait.playing, cJSON_IsNumber(sp) ? (float)sp->valuedouble : 0.f);
    } else {
        ok = false;
        snprintf(why, sizeof why, "unknown message `%s`", type);
    }
    if (!ok && !why[0]) strncpy(why, g_rl.note, sizeof why - 1);
    xSemaphoreGive(g_rl_lock);
    cJSON_Delete(m);
    if (!ok) refused(req, why);
    return ESP_OK;
}

/* Ten times a second: the state to every WebSocket. */
static void broadcast(void)
{
    size_t n = 16;
    int fds[16];
    if (httpd_get_client_list(server, &n, fds) != ESP_OK) return;
    for (size_t i = 0; i < n; i++) {
        if (httpd_ws_get_fd_info(server, fds[i]) != HTTPD_WS_CLIENT_WEBSOCKET) continue;
        /* A snapshot under the lock, the JSON built from it after: this task shares a core with
         * the Wi-Fi driver's higher-priority tasks, and holding the lock while they preempt it
         * kept the link task from its replies for 35 ms. */
        static rl_t *snapshot; /* the heap, not .bss: internal RAM the Wi-Fi link needs at boot */
        if (!snapshot && !(snapshot = malloc(sizeof *snapshot))) return;
        RL_LOCK("broadcast");
        memcpy(snapshot, &g_rl, sizeof *snapshot);
        xSemaphoreGive(g_rl_lock);
        cJSON *o = state_json(snapshot, fds[i]);
        char *text = cJSON_PrintUnformatted(o);
        cJSON_Delete(o);
        httpd_ws_frame_t frame = {.type = HTTPD_WS_TYPE_TEXT, .payload = (uint8_t *)text, .len = strlen(text)};
        httpd_ws_send_frame_async(server, fds[i], &frame);
        free(text);
    }
}

/* The page's state, ten times a second, from a task of its own at low priority. It used to be an
 * esp_timer callback: that task runs at priority 22 on the Wi-Fi driver's core, and once a pass
 * (JSON, socket sends) overran its period it ran back to back, starved the idle task and the
 * network with it (task watchdog, the page unreachable). Here an overrun only delays the next pass. */
static void broadcast_task(void *arg)
{
    (void)arg;
    TickType_t wake = xTaskGetTickCount();
    uint32_t slow_logged = 0;
    for (;;) {
        vTaskDelayUntil(&wake, pdMS_TO_TICKS(100));
        uint32_t t0 = rl_now_ms();
        broadcast();
        uint32_t took = rl_now_ms() - t0;
        if (took > 80 && rl_now_ms() - slow_logged > 10000) {
            slow_logged = rl_now_ms();
            ESP_LOGW(TAG, "a state broadcast took %lu ms", (unsigned long)took);
        }
        if (took >= 100) wake = xTaskGetTickCount(); /* no catching up */
    }
}

void web_start(void)
{
    plan_buf = malloc(PLAN_BUF);
    if (!plan_buf) ESP_LOGE(TAG, "no memory for the gait plan: gaits cannot be selected");
    httpd_config_t cfg = HTTPD_DEFAULT_CONFIG();
    cfg.server_port = CONFIG_RL_HTTP_PORT;
    cfg.max_open_sockets = 7;
    cfg.max_uri_handlers = 18;
    cfg.recv_wait_timeout = 20;
    cfg.lru_purge_enable = true;
    ESP_ERROR_CHECK(httpd_start(&server, &cfg));
    httpd_uri_t index = {.uri = "/", .method = HTTP_GET, .handler = index_get};
    httpd_uri_t state = {.uri = "/api/state", .method = HTTP_GET, .handler = state_get};
    httpd_uri_t stop = {.uri = "/api/stop", .method = HTTP_POST, .handler = stop_post};
    httpd_uri_t ws = {.uri = "/ws", .method = HTTP_GET, .handler = ws_handler, .is_websocket = true};
    httpd_register_uri_handler(server, &index);
    httpd_register_uri_handler(server, &state);
    httpd_register_uri_handler(server, &stop);
    httpd_register_uri_handler(server, &ws);
    httpd_uri_t gaits = {.uri = "/api/gaits", .method = HTTP_GET, .handler = gaits_get};
    httpd_uri_t upload = {.uri = "/api/gaits", .method = HTTP_POST, .handler = gaits_post};
    httpd_uri_t gait = {.uri = "/api/gait", .method = HTTP_POST, .handler = gait_post};
    httpd_uri_t gait_log = {.uri = "/api/gait_log", .method = HTTP_GET, .handler = gait_log_get};
    httpd_register_uri_handler(server, &gaits);
    httpd_register_uri_handler(server, &upload);
    httpd_register_uri_handler(server, &gait);
    httpd_register_uri_handler(server, &gait_log);
    httpd_uri_t cal_get = {.uri = "/api/calibration", .method = HTTP_GET, .handler = calibration_get};
    httpd_uri_t cal_post = {.uri = "/api/calibration", .method = HTTP_POST, .handler = calibration_post};
    httpd_register_uri_handler(server, &cal_get);
    httpd_register_uri_handler(server, &cal_post);
    httpd_uri_t rob_get = {.uri = "/api/robot", .method = HTTP_GET, .handler = robot_get};
    httpd_uri_t rob_post = {.uri = "/api/robot", .method = HTTP_POST, .handler = robot_post};
    httpd_uri_t hist_get = {.uri = "/api/history", .method = HTTP_GET, .handler = history_get};
    httpd_register_uri_handler(server, &rob_get);
    httpd_register_uri_handler(server, &rob_post);
    httpd_register_uri_handler(server, &hist_get);
    httpd_uri_t net_g = {.uri = "/api/network", .method = HTTP_GET, .handler = net_get};
    httpd_uri_t net_p = {.uri = "/api/network", .method = HTTP_POST, .handler = net_post};
    httpd_register_uri_handler(server, &net_g);
    httpd_register_uri_handler(server, &net_p);
    xTaskCreate(broadcast_task, "ws-state", 6144, NULL, 2, NULL);
    ESP_LOGI(TAG, "page on port %d", CONFIG_RL_HTTP_PORT);
}
