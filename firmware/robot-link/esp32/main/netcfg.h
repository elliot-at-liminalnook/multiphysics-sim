/*
 * The board's Wi-Fi in the field: up to NET_MAX saved networks for the
 * station (tried in turn, the next after repeated failures) and the board's
 * own access point, on by default so the page is reachable at 192.168.4.1
 * with no network around. Kept in NVS ("rlnet"/"net"); passwords are written,
 * never read back by the page.
 */
#ifndef NETCFG_H
#define NETCFG_H
#include <stdbool.h>
#include "cJSON.h"
#include "esp_http_server.h"
#define NET_MAX 4
void net_start(void);
/* GET /api/network, POST /api/network {"action": "add"|"remove"|"ap", ..., "operator_at_robot": true} */
esp_err_t net_get(httpd_req_t *req);
esp_err_t net_post(httpd_req_t *req);
/* For the page's WebSocket {"t":"network", ...} (owner only); false with `why` on refusal. */
bool net_action(const cJSON *m, char *why, size_t why_len);
cJSON *net_json(void);
#endif
