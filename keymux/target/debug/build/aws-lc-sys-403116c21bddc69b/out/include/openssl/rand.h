/* Copyright (c) 2014, Google Inc.
 *
 * Permission to use, copy, modify, and/or distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
 * SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
 * OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
 * CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE. */

#ifndef OPENSSL_HEADER_RAND_H
#define OPENSSL_HEADER_RAND_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Random number generation.


#define RAND_PRED_RESISTANCE_LEN (32)

// RAND_bytes writes |len| bytes of random data to |buf| and returns one. In the
// event that sufficient random data can not be obtained, |abort| is called.
OPENSSL_EXPORT int RAND_bytes(uint8_t *buf, size_t len);

// RAND_priv_bytes is a wrapper around |RAND_bytes| provided for compatibility.
// Consumers should call |RAND_bytes| directly.
OPENSSL_EXPORT int RAND_priv_bytes(uint8_t *buf, size_t len);

// RAND_bytes_with_user_prediction_resistance is functionally equivalent to
// |RAND_bytes| but also provides a way for the caller to inject prediction
// resistance material using the argument |user_pred_resistance|.
// |user_pred_resistance| must not be NULL and |user_pred_resistance| must be
// filled with |RAND_PRED_RESISTANCE_LEN| bytes.
OPENSSL_EXPORT int RAND_bytes_with_user_prediction_resistance(uint8_t *out,
  size_t out_len, const uint8_t user_pred_resistance[RAND_PRED_RESISTANCE_LEN]);

// Obscure functions.

#if defined(BORINGSSL_UNSAFE_DETERMINISTIC_MODE)
// RAND_reset_for_fuzzing resets the fuzzer-only deterministic RNG. This
// function is only defined in the fuzzer-only build configuration.
OPENSSL_EXPORT void RAND_reset_for_fuzzing(void);
#endif

// RAND_get_system_entropy_for_custom_prng writes |len| bytes of random data
// from a system entropy source to |buf|. The maximum length of entropy which
// may be requested is 256 bytes. If more than 256 bytes of data is requested,
// or if sufficient random data can not be obtained, |abort| is called.
// |RAND_bytes| should normally be used instead of this function. This function
// should only be used for seed values or where |malloc| should not be called
// from BoringSSL. This function is not FIPS compliant.
OPENSSL_EXPORT void RAND_get_system_entropy_for_custom_prng(uint8_t *buf,
                                                            size_t len);


// Deprecated functions

// RAND_pseudo_bytes is a wrapper around |RAND_bytes|.
OPENSSL_EXPORT int RAND_pseudo_bytes(uint8_t *buf, size_t len);

// RAND_seed reads a single byte of random data to ensure that any file
// descriptors etc are opened.
OPENSSL_EXPORT void RAND_seed(const void *buf, int num);


// General No-op Functions [Deprecated].
//
// OpenSSL historically allowed applications to do various operations to gather
// entropy and mix them into the entropy pool. AWS-LC sources entropy for the
// consuming application and the following functions have been deprecated as
// no-ops. Consumers should call |RAND_bytes| directly.
//
// TODO (CryptoAlg-2398): Add |OPENSSL_DEPRECATED| to the ones that are missing.
// curl and tpm2-tss defines -Wnerror and depend on them.

// RAND_load_file returns a nonnegative number.
OPENSSL_EXPORT OPENSSL_DEPRECATED int RAND_load_file(const char *path,
                                                     long num);

// RAND_write_file does nothing and returns negative 1.
OPENSSL_EXPORT OPENSSL_DEPRECATED int RAND_write_file(const char *file);

// RAND_file_name returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED const char *RAND_file_name(char *buf,
                                                             size_t num);

// RAND_add does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void RAND_add(const void *buf, int num,
                                                double entropy);

// RAND_egd returns 255.
OPENSSL_EXPORT OPENSSL_DEPRECATED int RAND_egd(const char *);

// RAND_egd_bytes returns |bytes|.
OPENSSL_EXPORT OPENSSL_DEPRECATED int RAND_egd_bytes(const char *, int bytes);

// RAND_poll returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int RAND_poll(void);

// RAND_status returns one.
OPENSSL_EXPORT int RAND_status(void);

// RAND_cleanup does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void RAND_cleanup(void);

// rand_meth_st is typedefed to |RAND_METHOD| in base.h. It isn't used; it
// exists only to be the return type of |RAND_SSLeay|. It's
// external so that variables of this type can be initialized.
struct rand_meth_st {
  void (*seed)(const void *buf, int num);
  int (*bytes)(uint8_t *buf, size_t num);
  void (*cleanup)(void);
  void (*add)(const void *buf, int num, double entropy);
  int (*pseudorand)(uint8_t *buf, size_t num);
  int (*status)(void);
};

// RAND_SSLeay returns a pointer to a dummy |RAND_METHOD|.
OPENSSL_EXPORT OPENSSL_DEPRECATED RAND_METHOD *RAND_SSLeay(void);

// RAND_OpenSSL returns a pointer to a dummy |RAND_METHOD|.
OPENSSL_EXPORT RAND_METHOD *RAND_OpenSSL(void);

// RAND_get_rand_method returns |RAND_SSLeay()|.
OPENSSL_EXPORT const RAND_METHOD *RAND_get_rand_method(void);

// RAND_set_rand_method returns one.
OPENSSL_EXPORT int RAND_set_rand_method(const RAND_METHOD *);

// RAND_keep_random_devices_open does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void RAND_keep_random_devices_open(int a);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_RAND_H
