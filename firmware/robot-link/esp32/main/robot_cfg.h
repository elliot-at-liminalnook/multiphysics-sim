/*
 * The robot's configuration, kept on the ESP32 so the robot can be set up in
 * the field without the Mac or a new FPGA image: which servo ID drives which
 * CAD joint (a slot: leg x role), each motor's direction, alignment and taught
 * poses, and host-side limits that may only be tighter than the FPGA's. It is
 * JSON in NVS ("rlcfg"/"robot"), edited by the page and REST, and pulled into
 * the repository by the Mac (leg_gait_pack --pull-robot), where the gait packs
 * are built from it.
 */
#ifndef ROBOT_CFG_H
#define ROBOT_CFG_H
#include <stdbool.h>
#include <stdint.h>
#include "cJSON.h"
#include "esp_err.h"

#define RC_SLOTS 16

typedef struct {
    char     leg[8];         /* "+X", "+Y", "-X", "-Y": the CAD leg */
    char     role[8];        /* "hip" (belt swing), "worm" (thigh), "foot" (slide) */
    char     joint[40];      /* the CAD motor joint, "+X | Foot servo output" */
    uint8_t  id;             /* the servo's bus ID; 0: no motor in this slot */
    bool     enabled;        /* polled and drivable */
    int8_t   polarity;       /* joint angle per encoder count: +1 or -1 */
    bool     aligned;        /* reference counts <-> reference_rad saved */
    int32_t  reference;
    float    reference_rad;
    bool     taught;         /* both poses known (an open end still counts as taught) */
    int32_t  lower, upper;   /* the calibration's poses: reversed when lower > upper */
    bool     open_lower, open_upper;
    bool     multi_turn;
    int32_t  turn_margin;
} rc_slot_t;

typedef struct {
    uint32_t  revision;      /* every saved change */
    char      name[24];
    uint8_t   slots;
    rc_slot_t slot[RC_SLOTS];
    uint8_t   temp_stop_c;   /* a playing gait stops at or above (FPGA: 60) */
    float     volt_min, volt_max; /* ... or outside (FPGA: 9.0-12.6 V) */
    int       duty_cap;      /* hold-to-move drive, 0..1000 */
    uint32_t  link_baud;     /* the FPGA link's last good baud rate */
} rc_robot_t;

extern rc_robot_t g_robot;

/* Load from NVS, or the CAD layout's 12 slots with the bench leg's IDs (call before the engine runs). */
void robot_cfg_init(void);
esp_err_t robot_cfg_save(void);
/* The configuration as JSON (the caller frees it). */
cJSON *robot_cfg_json(const rc_robot_t *r);
/* Parse and validate; on failure `why` names the field. */
bool robot_cfg_from_json(const cJSON *j, rc_robot_t *out, char *why, size_t why_len);
/* Push it into the engine (lock held): present servos, travel, limits, duty cap. */
void robot_cfg_apply(void);
/* Slots with a motor but no taught poses take the pack's travel for it (lock held); true if any did. */
bool robot_cfg_seed_travel(void);
/* After the engine changed a motor's taught travel (page teaching), copy it back (lock held). */
void robot_cfg_take_travel(void);
/* The slot a servo ID is in, or -1. */
int robot_cfg_slot_of(uint8_t id);
#endif
