/* Minimal shim to satisfy glslopt's os_misc.c on Termux, where the Android
 * NDK ships android/log.h rather than the NDK-conventional log/log.h.
 *
 * os_misc.c uses two things from <log/log.h>:
 *   1. androidx constants ANDROID_LOG_* (an enum here)
 *   2. the LOG_PRI(priority, tag, fmt, ...) macro
 * The log never actually leaves stdout/stderr (os_log_message() already
 * writes to a FILE*), so these are safe no-op definitions for a headless,
 * non-embedded build.
 */
#ifndef SERVERBROWSER_LOG_H
#define SERVERBROWSER_LOG_H

#include <stdarg.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum android_LogPriority {
    ANDROID_LOG_UNKNOWN = 0,
    ANDROID_LOG_DEFAULT,
    ANDROID_LOG_VERBOSE,
    ANDROID_LOG_DEBUG,
    ANDROID_LOG_INFO,
    ANDROID_LOG_WARN,
    ANDROID_LOG_ERROR,
    ANDROID_LOG_FATAL,
    ANDROID_LOG_SILENT,
} android_LogPriority;

static inline int __android_log_vprint(int prio, const char *tag,
                                       const char *fmt, va_list ap) {
    (void)prio; (void)tag; (void)fmt; (void)ap;
    return 0;
}

static inline int __android_log_print(int prio, const char *tag,
                                      const char *fmt, ...) {
    (void)prio; (void)tag; (void)fmt;
    return 0;
}

/* LOG_PRI is what os_misc.c actually invokes. */
#define LOG_PRI(priority, tag, ...) __android_log_print(priority, tag, __VA_ARGS__)

#ifdef __cplusplus
}
#endif

#endif /* SERVERBROWSER_LOG_H */