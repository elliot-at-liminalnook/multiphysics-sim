idf_component_register(
    SRCS "main.c" "web.c" "gait_store.c" "../../core/hx.c" "../../core/robot_link.c" "../../core/gait.c"
    INCLUDE_DIRS "." "../../core"
    EMBED_TXTFILES "index.html"
    REQUIRES esp_wifi esp_netif esp_event nvs_flash lwip esp_http_server json driver esp_timer esp_partition mbedtls
)
