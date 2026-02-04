#ifndef PLAYIT_AGENT_H
#define PLAYIT_AGENT_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    PLAYIT_STATUS_STOPPED = 0,
    PLAYIT_STATUS_CONNECTING = 1,
    PLAYIT_STATUS_CONNECTED = 2,
    PLAYIT_STATUS_DISCONNECTED = 3,
    PLAYIT_STATUS_ERROR = 4,
} playit_status_code;

typedef struct {
    int32_t code;
    const char *last_address;
    const char *last_error;
} playit_status;

typedef void (*playit_log_callback)(int32_t level, const char *message, void *user_data);

// Log levels: -1=TRACE, 0=DEBUG, 1=INFO, 2=WARN, 3=ERROR
void playit_set_log_callback(playit_log_callback callback, void *user_data);

// Config JSON fields:
// - secret_key (string, required)
// - api_url (string, optional; default https://api.playit.gg)
// - poll_interval_ms (number, optional; default 3000)
int32_t playit_init(const char *config_json);

int32_t playit_start(void);
int32_t playit_stop(void);
playit_status playit_get_status(void);

#ifdef __cplusplus
}
#endif

#endif

