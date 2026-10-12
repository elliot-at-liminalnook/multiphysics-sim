/* The robot's configuration: JSON in NVS, validated, applied to the engine. See robot_cfg.h. */
#include "robot_cfg.h"
#include "link_shared.h"
#include <limits.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_log.h"
#include "nvs.h"

static const char *TAG = "robot-cfg";
rc_robot_t g_robot;

static const char *const LEGS[] = {"+X", "+Y", "-X", "-Y"};
static const struct { const char *role, *joint; } ROLES[] = {
    {"hip", "Hip servo output"},   /* hip swing, belt driven */
    {"worm", "Worm servo output"}, /* thigh elevation through the worm */
    {"foot", "Foot servo output"}, /* the foot slide (the panel's "knee") */
};

/* The CAD layout's 12 slots; the bench leg (+X) on IDs 1-3 as the calibration panel taught it. */
static void defaults(rc_robot_t *r)
{
    memset(r, 0, sizeof *r);
    snprintf(r->name, sizeof r->name, "quadruped");
    for (int l = 0; l < 4; l++)
        for (int k = 0; k < 3; k++) {
            rc_slot_t *s = &r->slot[r->slots++];
            snprintf(s->leg, sizeof s->leg, "%s", LEGS[l]);
            snprintf(s->role, sizeof s->role, "%s", ROLES[k].role);
            snprintf(s->joint, sizeof s->joint, "%s | %s", LEGS[l], ROLES[k].joint);
            s->polarity = 1;
            s->enabled = true;
        }
    /* +X: foot 1, worm 2, hip 3 (calibration.json roles knee, worm, belt/hip). */
    r->slot[2].id = 1;
    r->slot[1].id = 2;
    r->slot[0].id = 3;
    r->slot[2].polarity = -1;
    r->slot[1].multi_turn = true;
    r->slot[1].turn_margin = 256;
    r->temp_stop_c = 55;
    r->volt_min = 9.0f;
    r->volt_max = 12.6f;
    r->duty_cap = CONFIG_RL_DUTY_CAP;
    r->link_baud = CONFIG_RL_UART_BAUD;
}

int robot_cfg_slot_of(uint8_t id)
{
    for (int k = 0; id && k < g_robot.slots; k++)
        if (g_robot.slot[k].id == id) return k;
    return -1;
}

/* ---- JSON ----------------------------------------------------------------- */

cJSON *robot_cfg_json(const rc_robot_t *r)
{
    cJSON *o = cJSON_CreateObject();
    cJSON_AddNumberToObject(o, "revision", r->revision);
    cJSON_AddStringToObject(o, "name", r->name);
    cJSON *lim = cJSON_AddObjectToObject(o, "limits");
    cJSON_AddNumberToObject(lim, "temp_stop_c", r->temp_stop_c);
    /* In tenths of a volt, as the servos report it: a float's 12.6000004 would fail its own range check. */
    cJSON_AddNumberToObject(lim, "volt_min_v", lroundf(r->volt_min * 10.f) / 10.0);
    cJSON_AddNumberToObject(lim, "volt_max_v", lroundf(r->volt_max * 10.f) / 10.0);
    cJSON_AddNumberToObject(lim, "duty_cap", r->duty_cap);
    cJSON_AddNumberToObject(o, "link_baud", r->link_baud);
    cJSON *slots = cJSON_AddArrayToObject(o, "slots");
    for (int k = 0; k < r->slots; k++) {
        const rc_slot_t *s = &r->slot[k];
        cJSON *j = cJSON_CreateObject();
        cJSON_AddStringToObject(j, "leg", s->leg);
        cJSON_AddStringToObject(j, "role", s->role);
        cJSON_AddStringToObject(j, "joint", s->joint);
        cJSON_AddNumberToObject(j, "id", s->id);
        cJSON_AddBoolToObject(j, "enabled", s->enabled);
        cJSON_AddNumberToObject(j, "polarity", s->polarity);
        if (s->aligned) {
            cJSON *a = cJSON_AddObjectToObject(j, "alignment");
            cJSON_AddNumberToObject(a, "counts", s->reference);
            cJSON_AddNumberToObject(a, "rad", s->reference_rad);
        } else cJSON_AddNullToObject(j, "alignment");
        if (s->taught) {
            cJSON *p = cJSON_AddObjectToObject(j, "poses");
            cJSON_AddNumberToObject(p, "lower", s->lower);
            cJSON_AddNumberToObject(p, "upper", s->upper);
            cJSON *open = cJSON_AddArrayToObject(p, "open");
            cJSON_AddItemToArray(open, cJSON_CreateBool(s->open_lower));
            cJSON_AddItemToArray(open, cJSON_CreateBool(s->open_upper));
        } else cJSON_AddNullToObject(j, "poses");
        cJSON_AddBoolToObject(j, "multi_turn", s->multi_turn);
        cJSON_AddNumberToObject(j, "turn_margin", s->turn_margin);
        cJSON_AddItemToArray(slots, j);
    }
    return o;
}

#define FAIL(...) do { snprintf(why, why_len, __VA_ARGS__); return false; } while (0)

static bool number(const cJSON *o, const char *key, double lo, double hi, double *out, char *why, size_t why_len, const char *where)
{
    const cJSON *v = cJSON_GetObjectItem(o, key);
    if (!cJSON_IsNumber(v) || !(v->valuedouble >= lo && v->valuedouble <= hi)) FAIL("%s.%s must be a number in %g..%g", where, key, lo, hi);
    *out = v->valuedouble;
    return true;
}

bool robot_cfg_from_json(const cJSON *j, rc_robot_t *out, char *why, size_t why_len)
{
    rc_robot_t r;
    memset(&r, 0, sizeof r);
    double v;
    const cJSON *name = cJSON_GetObjectItem(j, "name");
    snprintf(r.name, sizeof r.name, "%s", cJSON_IsString(name) ? name->valuestring : "robot");
    const cJSON *lim = cJSON_GetObjectItem(j, "limits");
    if (!cJSON_IsObject(lim)) FAIL("limits is missing");
    /* Only tighter than the FPGA's own trips (60 C, 9.0-12.6 V). */
    if (!number(lim, "temp_stop_c", 20, 59, &v, why, why_len, "limits")) return false;
    r.temp_stop_c = (uint8_t)v;
    if (!number(lim, "volt_min_v", 9.0, 12.6, &v, why, why_len, "limits")) return false;
    r.volt_min = lround(v * 10.0) / 10.f;
    if (!number(lim, "volt_max_v", 9.0, 12.6, &v, why, why_len, "limits")) return false;
    r.volt_max = lround(v * 10.0) / 10.f;
    if (r.volt_min >= r.volt_max) FAIL("limits.volt_min_v must be below volt_max_v");
    if (!number(lim, "duty_cap", 0, 1000, &v, why, why_len, "limits")) return false;
    r.duty_cap = (int)v;
    const cJSON *baud = cJSON_GetObjectItem(j, "link_baud");
    r.link_baud = cJSON_IsNumber(baud) && (baud->valueint == 115200 || baud->valueint == 1000000) ? (uint32_t)baud->valueint : CONFIG_RL_UART_BAUD;
    const cJSON *slots = cJSON_GetObjectItem(j, "slots");
    if (!cJSON_IsArray(slots) || cJSON_GetArraySize(slots) > RC_SLOTS) FAIL("slots must be a list of at most %d", RC_SLOTS);
    const cJSON *e;
    cJSON_ArrayForEach(e, slots) {
        rc_slot_t *s = &r.slot[r.slots];
        char where[24];
        snprintf(where, sizeof where, "slots[%u]", r.slots);
        const cJSON *leg = cJSON_GetObjectItem(e, "leg"), *role = cJSON_GetObjectItem(e, "role"), *joint = cJSON_GetObjectItem(e, "joint");
        if (!cJSON_IsString(leg) || !cJSON_IsString(role) || !cJSON_IsString(joint)) FAIL("%s needs leg, role and joint", where);
        snprintf(s->leg, sizeof s->leg, "%s", leg->valuestring);
        snprintf(s->role, sizeof s->role, "%s", role->valuestring);
        snprintf(s->joint, sizeof s->joint, "%s", joint->valuestring);
        if (!number(e, "id", 0, RL_MAX_SERVOS, &v, why, why_len, where)) return false;
        s->id = (uint8_t)v;
        for (int k = 0; s->id && k < r.slots; k++)
            if (r.slot[k].id == s->id) FAIL("ID %u is in two slots (%s %s and %s %s)", s->id, r.slot[k].leg, r.slot[k].role, s->leg, s->role);
        s->enabled = !cJSON_IsFalse(cJSON_GetObjectItem(e, "enabled"));
        if (!number(e, "polarity", -1, 1, &v, why, why_len, where)) return false;
        if (v != 1 && v != -1) FAIL("%s.polarity must be 1 or -1", where);
        s->polarity = (int8_t)v;
        s->multi_turn = cJSON_IsTrue(cJSON_GetObjectItem(e, "multi_turn"));
        const cJSON *margin = cJSON_GetObjectItem(e, "turn_margin");
        s->turn_margin = cJSON_IsNumber(margin) ? margin->valueint : 0;
        if (s->turn_margin < 0 || s->turn_margin > 1024) FAIL("%s.turn_margin must be 0..1024", where);
        const cJSON *a = cJSON_GetObjectItem(e, "alignment");
        if (cJSON_IsObject(a)) {
            if (!number(a, "counts", -8000000, 8000000, &v, why, why_len, where)) return false;
            s->reference = (int32_t)v;
            if (!number(a, "rad", -100, 100, &v, why, why_len, where)) return false;
            s->reference_rad = (float)v;
            s->aligned = true;
        }
        const cJSON *p = cJSON_GetObjectItem(e, "poses");
        if (cJSON_IsObject(p)) {
            if (!number(p, "lower", -8000000, 8000000, &v, why, why_len, where)) return false;
            s->lower = (int32_t)v;
            if (!number(p, "upper", -8000000, 8000000, &v, why, why_len, where)) return false;
            s->upper = (int32_t)v;
            if (abs(s->upper - s->lower) < RL_TRAVEL_MIN) FAIL("%s: its poses must be at least %d counts apart", where, RL_TRAVEL_MIN);
            const cJSON *open = cJSON_GetObjectItem(p, "open");
            s->open_lower = cJSON_IsArray(open) && cJSON_IsTrue(cJSON_GetArrayItem(open, 0));
            s->open_upper = cJSON_IsArray(open) && cJSON_IsTrue(cJSON_GetArrayItem(open, 1));
            if (s->open_lower && s->open_upper) FAIL("%s: both poses are open", where);
            int32_t lo = s->lower < s->upper ? s->lower : s->upper, hi = s->lower < s->upper ? s->upper : s->lower;
            if (!s->multi_turn && (lo < 0 || hi > 4095)) FAIL("%s: poses beyond one turn need multi_turn", where);
            if (s->multi_turn && (int64_t)hi - lo + 2 * s->turn_margin >= 2 * 4096) FAIL("%s: travel spans two turns", where);
            s->taught = true;
        }
        r.slots++;
    }
    const cJSON *rev = cJSON_GetObjectItem(j, "revision");
    r.revision = cJSON_IsNumber(rev) ? (uint32_t)rev->valuedouble : 0;
    *out = r;
    return true;
}

/* ---- NVS ------------------------------------------------------------------ */

esp_err_t robot_cfg_save(void)
{
    g_robot.revision++;
    cJSON *j = robot_cfg_json(&g_robot);
    char *text = cJSON_PrintUnformatted(j);
    cJSON_Delete(j);
    if (!text) return ESP_ERR_NO_MEM;
    nvs_handle_t h;
    esp_err_t e = nvs_open("rlcfg", NVS_READWRITE, &h);
    if (e == ESP_OK) {
        e = nvs_set_str(h, "robot", text);
        if (e == ESP_OK) e = nvs_commit(h);
        nvs_close(h);
    }
    free(text);
    if (e != ESP_OK) ESP_LOGE(TAG, "configuration not saved: %s", esp_err_to_name(e));
    return e;
}

void robot_cfg_init(void)
{
    defaults(&g_robot);
    nvs_handle_t h;
    size_t len = 0;
    if (nvs_open("rlcfg", NVS_READONLY, &h) != ESP_OK) {
        ESP_LOGI(TAG, "no saved configuration: the CAD layout with the bench leg on IDs 1-3");
        return;
    }
    char *text = NULL;
    if (nvs_get_str(h, "robot", NULL, &len) == ESP_OK && len > 1 && (text = malloc(len)) && nvs_get_str(h, "robot", text, &len) == ESP_OK) {
        cJSON *j = cJSON_Parse(text);
        char why[128];
        rc_robot_t r;
        if (j && robot_cfg_from_json(j, &r, why, sizeof why)) {
            g_robot = r;
            ESP_LOGI(TAG, "configuration \"%s\" revision %lu, %u slots", g_robot.name, (unsigned long)g_robot.revision, g_robot.slots);
        } else ESP_LOGE(TAG, "the saved configuration is invalid (%s): using the defaults", j ? why : "not JSON");
        cJSON_Delete(j);
    }
    free(text);
    nvs_close(h);
}

/* ---- the engine ----------------------------------------------------------- */

void robot_cfg_apply(void)
{
    uint16_t present = 0;
    for (int k = 0; k < g_robot.slots; k++) {
        const rc_slot_t *s = &g_robot.slot[k];
        int i = s->id ? (int)s->id - g_rl.cfg.first_id : -1;
        if (s->enabled && i >= 0 && i < RL_MAX_SERVOS) present |= (uint16_t)(1u << i);
    }
    rl_set_present(&g_rl, present);
    for (int i = 0; i < RL_MAX_SERVOS; i++) {
        uint8_t id = (uint8_t)(g_rl.cfg.first_id + i);
        int k = robot_cfg_slot_of(id);
        const rc_slot_t *s = k >= 0 ? &g_robot.slot[k] : NULL;
        if (!s || !s->enabled || !s->taught) {
            rl_travel_forget(&g_rl, id);
            continue;
        }
        bool reversed = s->lower > s->upper;
        int32_t lo = reversed ? s->upper : s->lower, hi = reversed ? s->lower : s->upper;
        bool open_lo = reversed ? s->open_upper : s->open_lower, open_hi = reversed ? s->open_lower : s->open_upper;
        rl_travel_configure(&g_rl, id, lo, hi, open_lo, open_hi, s->multi_turn, reversed, s->turn_margin);
    }
    g_rl.cfg.temp_stop_c = g_robot.temp_stop_c;
    g_rl.cfg.volt_min_raw = (uint8_t)lroundf(g_robot.volt_min * 10.f);
    g_rl.cfg.volt_max_raw = (uint8_t)lroundf(g_robot.volt_max * 10.f);
    g_rl.cfg.duty_cap = g_robot.duty_cap;
}

void robot_cfg_take_travel(void)
{
    for (int k = 0; k < g_robot.slots; k++) {
        rc_slot_t *s = &g_robot.slot[k];
        int i = s->id ? rl_axis(&g_rl, s->id) : -1;
        if (i < 0 || !g_rl.servo[i].travel.known) continue;
        const rl_travel_t *t = &g_rl.servo[i].travel;
        /* The calibration's names: a reversed motor's lower pose is its high-count end. */
        s->lower = t->reversed ? t->hi : t->lo;
        s->upper = t->reversed ? t->lo : t->hi;
        s->open_lower = t->reversed ? t->open[1] : t->open[0];
        s->open_upper = t->reversed ? t->open[0] : t->open[1];
        s->taught = true;
    }
}

bool robot_cfg_seed_travel(void)
{
    bool any = false;
    for (int k = 0; k < g_robot.slots; k++) {
        rc_slot_t *s = &g_robot.slot[k];
        int i = s->id ? rl_axis(&g_rl, s->id) : -1;
        if (s->taught || i < 0) continue;
        const rl_travel_t *t = &g_rl.servo[i].travel;
        if (!t->known || t->pack_lo == INT32_MIN) continue;
        s->lower = t->reversed ? t->pack_hi : t->pack_lo;
        s->upper = t->reversed ? t->pack_lo : t->pack_hi;
        s->multi_turn = t->multi_turn;
        s->turn_margin = t->margin;
        s->taught = true;
        any = true;
    }
    return any;
}
