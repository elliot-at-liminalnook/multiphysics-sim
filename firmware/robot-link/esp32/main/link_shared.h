/* The link engine shared between the UART task and the web server. */
#ifndef LINK_SHARED_H
#define LINK_SHARED_H
#include "robot_link.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"

extern rl_t g_rl;
extern SemaphoreHandle_t g_rl_lock;
/* Who took the engine's lock last (a diagnostic: the link task reports who it waited for). */
extern const char *volatile g_rl_lock_holder;
#define RL_LOCK(who) do { xSemaphoreTake(g_rl_lock, portMAX_DELAY); g_rl_lock_holder = (who); } while (0)
uint32_t rl_now_ms(void);

/* Raw bytes to the FPGA (the tunnel). */
void link_uart_write(const uint8_t *bytes, size_t n);

/* STOP from the page, call with the lock held. When the tunnel or the bus scan
 * owns the UART the engine is paused, so this also writes STOP straight to the
 * FPGA rather than waiting for the owner to finish. Landing mid-packet, it
 * makes the bridge latch a bridge fault, which is a stop as well. */
void link_stop_now(void);

/* Who holds the FPGA's host UART: nobody, the page with this WebSocket fd, the tunnel, or the bus scan. */
enum { OWNER_NONE = 0, OWNER_PAGE, OWNER_TUNNEL, OWNER_SCAN };
extern int g_owner_kind, g_owner_fd;

/* The last bus scan: which IDs 0-253 answered a ping. */
typedef struct {
    bool     running, done;
    uint32_t at_ms, took_ms;
    uint32_t answered[8];   /* bit per ID */
    uint32_t errors[8];     /* answered with a device error */
} link_scan_t;
extern link_scan_t g_scan;
/* Ask for a scan (lock held): refused unless the FPGA is latched with nothing armed and no gait. */
bool link_scan_request(char *why, size_t why_len);
/* The FPGA link's baud rate now (it follows whichever image answers). */
extern volatile uint32_t g_link_baud;

void web_start(void);
/* The tunnel passes bytes it received from the FPGA to the web layer for counting only. */
#endif
