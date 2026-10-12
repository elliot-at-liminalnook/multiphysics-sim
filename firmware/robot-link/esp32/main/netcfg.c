/* See netcfg.h. */
#include "netcfg.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "esp_event.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "esp_random.h"
#include "esp_netif.h"
#include "esp_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "nvs.h"

static const char *TAG = "net";

typedef struct { char ssid[33], password[65]; } net_t;
static struct {
    int    count;
    net_t  net[NET_MAX];
    net_t  ap;
    bool   ap_on;
} cfg;
static SemaphoreHandle_t lock;
static int current, failures;
static bool connected;
static char ip[16] = "";
static int ap_clients;

static void save(void)
{
    cJSON *o = cJSON_CreateObject();
    cJSON *list = cJSON_AddArrayToObject(o, "networks");
    for (int k = 0; k < cfg.count; k++) {
        cJSON *n = cJSON_CreateObject();
        cJSON_AddStringToObject(n, "ssid", cfg.net[k].ssid);
        cJSON_AddStringToObject(n, "password", cfg.net[k].password);
        cJSON_AddItemToArray(list, n);
    }
    cJSON *ap = cJSON_AddObjectToObject(o, "ap");
    cJSON_AddStringToObject(ap, "ssid", cfg.ap.ssid);
    cJSON_AddStringToObject(ap, "password", cfg.ap.password);
    cJSON_AddBoolToObject(ap, "on", cfg.ap_on);
    char *text = cJSON_PrintUnformatted(o);
    cJSON_Delete(o);
    nvs_handle_t h;
    if (text && nvs_open("rlnet", NVS_READWRITE, &h) == ESP_OK) {
        nvs_set_str(h, "net", text);
        nvs_commit(h);
        nvs_close(h);
    }
    free(text);
}

static void load(void)
{
    memset(&cfg, 0, sizeof cfg);
#if CONFIG_RL_WIFI_STA
    snprintf(cfg.net[0].ssid, sizeof cfg.net[0].ssid, "%s", CONFIG_RL_WIFI_SSID);
    snprintf(cfg.net[0].password, sizeof cfg.net[0].password, "%s", CONFIG_RL_WIFI_PASSWORD);
    cfg.count = 1;
#endif
    /* The P4's radio is the C6's, so its own SoftAP MAC is not in eFuse: name the board by its base MAC. */
    uint8_t mac[6] = {0};
    esp_efuse_mac_get_default(mac);
    snprintf(cfg.ap.ssid, sizeof cfg.ap.ssid, "robot-link-%02X%02X", mac[4], mac[5]);
    /* The password is random, made once and kept: the MAC is broadcast, so nothing derived from it is secret. */
    static const char alphabet[] = "abcdefghjkmnpqrstuvwxyz23456789";
    for (int k = 0; k < 12; k++) cfg.ap.password[k] = alphabet[esp_random() % (sizeof alphabet - 1)];
    cfg.ap.password[12] = 0;
    cfg.ap_on = true;
    nvs_handle_t h;
    size_t len = 0;
    if (nvs_open("rlnet", NVS_READONLY, &h) != ESP_OK) { save(); return; }
    char *text = NULL;
    if (nvs_get_str(h, "net", NULL, &len) == ESP_OK && (text = malloc(len)) && nvs_get_str(h, "net", text, &len) == ESP_OK) {
        cJSON *o = cJSON_Parse(text);
        const cJSON *list = cJSON_GetObjectItem(o, "networks"), *n;
        int k = 0;
        cJSON_ArrayForEach(n, list) {
            const cJSON *s = cJSON_GetObjectItem(n, "ssid"), *p = cJSON_GetObjectItem(n, "password");
            if (k < NET_MAX && cJSON_IsString(s) && cJSON_IsString(p)) {
                snprintf(cfg.net[k].ssid, sizeof cfg.net[k].ssid, "%s", s->valuestring);
                snprintf(cfg.net[k].password, sizeof cfg.net[k].password, "%s", p->valuestring);
                k++;
            }
        }
        if (cJSON_IsArray(list)) cfg.count = k;
        const cJSON *ap = cJSON_GetObjectItem(o, "ap");
        const cJSON *s = cJSON_GetObjectItem(ap, "ssid"), *p = cJSON_GetObjectItem(ap, "password");
        if (cJSON_IsString(s) && cJSON_IsString(p) && strlen(p->valuestring) >= 8) {
            snprintf(cfg.ap.ssid, sizeof cfg.ap.ssid, "%s", s->valuestring);
            snprintf(cfg.ap.password, sizeof cfg.ap.password, "%s", p->valuestring);
        }
        cfg.ap_on = !cJSON_IsFalse(cJSON_GetObjectItem(ap, "on"));
        cJSON_Delete(o);
    }
    bool stored = text != NULL;
    free(text);
    nvs_close(h);
    if (!stored) save();
}

static void join(int k)
{
    if (!cfg.count) return;
    current = k % cfg.count;
    wifi_config_t wc = {0};
    strncpy((char *)wc.sta.ssid, cfg.net[current].ssid, sizeof wc.sta.ssid - 1);
    strncpy((char *)wc.sta.password, cfg.net[current].password, sizeof wc.sta.password - 1);
    esp_wifi_set_config(WIFI_IF_STA, &wc);
    esp_wifi_connect();
    ESP_LOGI(TAG, "joining \"%s\"", cfg.net[current].ssid);
}

static void apply_ap(void)
{
    wifi_config_t wc = {0};
    strncpy((char *)wc.ap.ssid, cfg.ap.ssid, sizeof wc.ap.ssid - 1);
    wc.ap.ssid_len = strlen(cfg.ap.ssid);
    strncpy((char *)wc.ap.password, cfg.ap.password, sizeof wc.ap.password - 1);
    wc.ap.max_connection = 4;
    wc.ap.authmode = WIFI_AUTH_WPA2_PSK;
    wc.ap.channel = 6;
    esp_err_t e = esp_wifi_set_config(WIFI_IF_AP, &wc);
    if (e != ESP_OK) ESP_LOGE(TAG, "access point config: %s", esp_err_to_name(e));
}

static void on_wifi(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg;
    if (base == WIFI_EVENT && id == WIFI_EVENT_STA_DISCONNECTED) {
        connected = false;
        ip[0] = 0;
        /* Three tries each, then the next saved network. */
        if (++failures >= 3) { failures = 0; join(current + 1); }
        else esp_wifi_connect();
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP) {
        ip_event_got_ip_t *e = data;
        snprintf(ip, sizeof ip, IPSTR, IP2STR(&e->ip_info.ip));
        connected = true;
        failures = 0;
        ESP_LOGI(TAG, "station address %s on \"%s\"", ip, cfg.net[current].ssid);
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STACONNECTED) {
        ap_clients++;
        ESP_LOGI(TAG, "a device joined the access point");
    } else if (base == WIFI_EVENT && id == WIFI_EVENT_AP_STADISCONNECTED) {
        if (ap_clients) ap_clients--;
    }
}

void net_start(void)
{
    lock = xSemaphoreCreateMutex();
    load();
    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    ESP_ERROR_CHECK(esp_event_handler_register(WIFI_EVENT, ESP_EVENT_ANY_ID, on_wifi, NULL));
    ESP_ERROR_CHECK(esp_event_handler_register(IP_EVENT, IP_EVENT_STA_GOT_IP, on_wifi, NULL));
    esp_netif_create_default_wifi_sta();
    esp_netif_create_default_wifi_ap();
    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&init));
    /* The access point and the station together; the station alone if the AP is switched off. */
    esp_err_t e = esp_wifi_set_mode(cfg.ap_on || !cfg.count ? WIFI_MODE_APSTA : WIFI_MODE_STA);
    if (e != ESP_OK) {
        ESP_LOGE(TAG, "access point and station together refused (%s): station only", esp_err_to_name(e));
        ESP_ERROR_CHECK(esp_wifi_set_mode(WIFI_MODE_STA));
    } else if (cfg.ap_on || !cfg.count) apply_ap();
    ESP_ERROR_CHECK(esp_wifi_start());
    esp_wifi_set_ps(WIFI_PS_NONE);
    if (cfg.ap_on || !cfg.count) ESP_LOGW(TAG, "access point \"%s\" (password %s): http://192.168.4.1/", cfg.ap.ssid, cfg.ap.password);
    join(0);
}

cJSON *net_json(void)
{
    cJSON *o = cJSON_CreateObject();
    cJSON *st = cJSON_AddObjectToObject(o, "station");
    cJSON_AddBoolToObject(st, "connected", connected);
    cJSON_AddStringToObject(st, "ssid", cfg.count ? cfg.net[current].ssid : "");
    cJSON_AddStringToObject(st, "ip", ip);
    wifi_ap_record_t rec;
    if (connected && esp_wifi_sta_get_ap_info(&rec) == ESP_OK) cJSON_AddNumberToObject(st, "rssi_dbm", rec.rssi);
    cJSON *list = cJSON_AddArrayToObject(o, "networks");
    for (int k = 0; k < cfg.count; k++) cJSON_AddItemToArray(list, cJSON_CreateString(cfg.net[k].ssid));
    cJSON *ap = cJSON_AddObjectToObject(o, "ap");
    cJSON_AddBoolToObject(ap, "on", cfg.ap_on || !cfg.count);
    cJSON_AddStringToObject(ap, "ssid", cfg.ap.ssid);
    /* The access point's own password is shown: it is how a phone joins in the field. */
    cJSON_AddStringToObject(ap, "password", cfg.ap.password);
    cJSON_AddStringToObject(ap, "address", "192.168.4.1");
    cJSON_AddNumberToObject(ap, "clients", ap_clients);
    return o;
}

bool net_action(const cJSON *m, char *why, size_t why_len)
{
    const cJSON *action = cJSON_GetObjectItem(m, "action"), *ssid = cJSON_GetObjectItem(m, "ssid"), *pw = cJSON_GetObjectItem(m, "password");
    const char *a = cJSON_IsString(action) ? action->valuestring : "";
    bool ok = true;
    xSemaphoreTake(lock, portMAX_DELAY);
    if (!strcmp(a, "add")) {
        if (!cJSON_IsString(ssid) || !*ssid->valuestring || strlen(ssid->valuestring) > 32 || !cJSON_IsString(pw) || strlen(pw->valuestring) > 64)
            ok = false, snprintf(why, why_len, "add needs ssid (1-32 characters) and password (up to 64)");
        else {
            int k = 0;
            while (k < cfg.count && strcmp(cfg.net[k].ssid, ssid->valuestring)) k++;
            if (k == cfg.count && cfg.count == NET_MAX) ok = false, snprintf(why, why_len, "at most %d networks: remove one first", NET_MAX);
            else {
                if (k == cfg.count) cfg.count++;
                snprintf(cfg.net[k].ssid, sizeof cfg.net[k].ssid, "%s", ssid->valuestring);
                snprintf(cfg.net[k].password, sizeof cfg.net[k].password, "%s", pw->valuestring);
                save();
                if (!connected) { failures = 0; join(k); }
            }
        }
    } else if (!strcmp(a, "remove")) {
        int k = 0;
        while (cJSON_IsString(ssid) && k < cfg.count && strcmp(cfg.net[k].ssid, ssid->valuestring)) k++;
        if (!cJSON_IsString(ssid) || k == cfg.count) ok = false, snprintf(why, why_len, "no saved network by that name");
        else {
            memmove(&cfg.net[k], &cfg.net[k + 1], (size_t)(cfg.count - k - 1) * sizeof cfg.net[0]);
            cfg.count--;
            save();
        }
    } else if (!strcmp(a, "ap")) {
        const cJSON *on = cJSON_GetObjectItem(m, "on");
        if (cJSON_IsString(ssid) && (!*ssid->valuestring || strlen(ssid->valuestring) > 32)) ok = false, snprintf(why, why_len, "the access point name must be 1-32 characters");
        else if (cJSON_IsString(pw) && (strlen(pw->valuestring) < 8 || strlen(pw->valuestring) > 63)) ok = false, snprintf(why, why_len, "the access point password must be 8-63 characters");
        else if (cJSON_IsFalse(on) && !cfg.count) ok = false, snprintf(why, why_len, "with no saved network the access point is the only way in: it stays on");
        else {
            if (cJSON_IsString(ssid)) snprintf(cfg.ap.ssid, sizeof cfg.ap.ssid, "%s", ssid->valuestring);
            if (cJSON_IsString(pw)) snprintf(cfg.ap.password, sizeof cfg.ap.password, "%s", pw->valuestring);
            if (cJSON_IsBool(on)) cfg.ap_on = cJSON_IsTrue(on);
            save();
        }
    } else ok = false, snprintf(why, why_len, "action must be add, remove or ap");
    xSemaphoreGive(lock);
    return ok;
}

static esp_err_t send_obj(httpd_req_t *req, cJSON *o, const char *refused)
{
    if (refused) {
        cJSON_AddStringToObject(o, "refused", refused);
        httpd_resp_set_status(req, "409 Conflict");
    }
    char *text = cJSON_PrintUnformatted(o);
    cJSON_Delete(o);
    httpd_resp_set_type(req, "application/json");
    esp_err_t r = httpd_resp_send(req, text, HTTPD_RESP_USE_STRLEN);
    free(text);
    return r;
}

esp_err_t net_get(httpd_req_t *req)
{
    return send_obj(req, net_json(), NULL);
}

esp_err_t net_post(httpd_req_t *req)
{
    char body[256] = "";
    int n = req->content_len < sizeof body - 1 ? (int)req->content_len : (int)sizeof body - 1;
    int got = n > 0 ? httpd_req_recv(req, body, n) : 0;
    cJSON *m = got > 0 ? cJSON_ParseWithLength(body, (size_t)got) : NULL;
    char why[160] = "";
    bool ok = m && cJSON_IsTrue(cJSON_GetObjectItem(m, "operator_at_robot"));
    if (!ok) snprintf(why, sizeof why, "network changes need operator_at_robot: true");
    else ok = net_action(m, why, sizeof why);
    cJSON_Delete(m);
    return send_obj(req, net_json(), ok ? NULL : why);
}
