// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR ISC

#ifndef UI_H
#define UI_H

#include "openssl/base.h"
struct ui_st {
  char _unused;
};

struct ui_method_st {
  char _unused;
};

typedef struct ui_st UI;
typedef struct ui_method_st UI_METHOD;

/// UI_new does nothing, always returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED UI *UI_new(void);

/// UI_free invokes OPENSSL_free on its parameter.
OPENSSL_EXPORT OPENSSL_DEPRECATED void UI_free(UI *ui);

/// UI_add_input_string does nothing, always returns 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int UI_add_input_string(UI *ui, const char *prompt, int flags,
        char *result_buf, int minsize, int maxsize);

/// UI_add_verify_string does nothing, always returns 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int UI_add_verify_string(UI *ui, const char *prompt, int flags,
        char *result_buf, int minsize, int maxsize, const char *test_buf);

/// UI_add_info_string does nothing, always returns 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int UI_add_info_string(UI *ui, const char *text);

/// UI_process does nothing, always returns 0.
OPENSSL_EXPORT OPENSSL_DEPRECATED int UI_process(UI *ui);

#endif //UI_H
