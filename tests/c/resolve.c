/*
 * Resolves one envelope through the C API the way an app does, and prints the resolved tree and a
 * newline, the form of the goldens in fixtures/expected/resolved. scripts/check-prebuild.sh builds
 * it with include/twidget.h against the macOS library in prebuild/.
 *
 * Usage: resolve ENVELOPE TEMPLATES NOW_MS TZ_OFFSET_MINUTES light|dark ltr|rtl
 *
 * TEMPLATES is the directory of bundled templates, each named <id>@<version>.json.
 */

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "twidget.h"

/* Prints the library's description of the call that failed with status, and exits. */
_Noreturn static void fail(const char *call, int32_t status) {
    uint8_t message[1024];
    size_t len = twidget_last_error(message, sizeof message);
    if (len > sizeof message) {
        len = sizeof message;
    }
    fprintf(stderr, "resolve: %s returned %" PRId32 ": %.*s\n", call, status, (int)len,
            (const char *)message);
    exit(1);
}

/* Reads the whole file at path, or exits. */
static uint8_t *read_file(const char *path, size_t *len) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        perror(path);
        exit(1);
    }
    uint8_t *bytes = NULL;
    size_t cap = 0;
    *len = 0;
    do {
        cap = cap == 0 ? 65536 : cap * 2;
        uint8_t *grown = realloc(bytes, cap);
        if (grown == NULL) {
            fputs("resolve: out of memory\n", stderr);
            exit(1);
        }
        bytes = grown;
        *len += fread(bytes + *len, 1, cap - *len, file);
    } while (*len == cap);
    if (ferror(file)) {
        perror(path);
        exit(1);
    }
    fclose(file);
    return bytes;
}

/* Parses a decimal integer argument, or exits. */
static long long parse_integer(const char *name, const char *text) {
    char *end = NULL;
    errno = 0;
    long long value = strtoll(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0') {
        fprintf(stderr, "resolve: %s is not an integer: %s\n", name, text);
        exit(2);
    }
    return value;
}

/* Parses an argument that must be one of two words: 0 for the first, 1 for the second. */
static uint32_t parse_choice(const char *name, const char *text, const char *zero,
                             const char *one) {
    if (strcmp(text, zero) == 0) {
        return 0;
    }
    if (strcmp(text, one) == 0) {
        return 1;
    }
    fprintf(stderr, "resolve: %s must be %s or %s: %s\n", name, zero, one, text);
    exit(2);
}

int main(int argc, char **argv) {
    if (argc != 7) {
        fputs("usage: resolve ENVELOPE TEMPLATES NOW_MS TZ_OFFSET_MINUTES light|dark ltr|rtl\n",
              stderr);
        return 2;
    }
    long long tz_offset_minutes = parse_integer("TZ_OFFSET_MINUTES", argv[4]);
    if (tz_offset_minutes < INT32_MIN || tz_offset_minutes > INT32_MAX) {
        fprintf(stderr, "resolve: TZ_OFFSET_MINUTES is out of range: %s\n", argv[4]);
        return 2;
    }
    TWidgetEnv env = {
        .struct_size = (uint32_t)sizeof env,
        .now_ms = (int64_t)parse_integer("NOW_MS", argv[3]),
        .tz_offset_minutes = (int32_t)tz_offset_minutes,
        .dark = parse_choice("the theme", argv[5], "light", "dark"),
        .rtl = parse_choice("the direction", argv[6], "ltr", "rtl"),
    };

    /* A library built from another header would misread every call. */
    if (twidget_abi_version() != TWIDGET_ABI_VERSION) {
        fprintf(stderr, "resolve: the library has ABI %" PRIu32 " and the header %u\n",
                twidget_abi_version(), TWIDGET_ABI_VERSION);
        return 1;
    }

    size_t envelope_len = 0;
    uint8_t *envelope = read_file(argv[1], &envelope_len);

    /* An envelope that names a bundled template needs that template loaded first. */
    TWidgetTemplate *bundled = NULL;
    uint8_t id[256];
    size_t id_len = 0;
    uint32_t version = 0;
    int32_t status = twidget_envelope_template_ref(envelope, envelope_len, id, sizeof id, &id_len,
                                                   &version);
    if (status == TWIDGET_OK) {
        char path[4096];
        int written = snprintf(path, sizeof path, "%s/%.*s@%" PRIu32 ".json", argv[2],
                               (int)id_len, (const char *)id, version);
        if (written < 0 || (size_t)written >= sizeof path) {
            fputs("resolve: the template's path is too long\n", stderr);
            return 1;
        }
        size_t json_len = 0;
        uint8_t *json = read_file(path, &json_len);
        bundled = twidget_template_new(json, json_len, &status);
        free(json);
        if (bundled == NULL) {
            fail("twidget_template_new", status);
        }
    } else if (status != TWIDGET_ERROR_NOT_FOUND) {
        fail("twidget_envelope_template_ref", status);
    }

    TWidgetInstance *instance = twidget_instance_new(envelope, envelope_len, bundled, &status);
    if (instance == NULL) {
        fail("twidget_instance_new", status);
    }
    free(envelope);
    /* The instance holds its own reference, so the template can go now. */
    twidget_template_drop(bundled);

    /* The first call learns the size, and the second copies the tree the first one cached. */
    size_t out_len = 0;
    status = twidget_resolve(instance, &env, NULL, 0, &out_len, NULL);
    if (status != TWIDGET_ERROR_BUFFER_TOO_SMALL) {
        fail("twidget_resolve", status);
    }
    uint8_t *out = malloc(out_len);
    if (out == NULL) {
        fputs("resolve: out of memory\n", stderr);
        return 1;
    }
    status = twidget_resolve(instance, &env, out, out_len, &out_len, NULL);
    if (status != TWIDGET_OK) {
        fail("twidget_resolve", status);
    }
    twidget_instance_drop(instance);

    fwrite(out, 1, out_len, stdout);
    putchar('\n');
    free(out);
    return fflush(stdout) == 0 && !ferror(stdout) ? 0 : 1;
}
