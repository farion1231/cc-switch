/* Copyright (C) 1995-1998 Eric Young (eay@cryptsoft.com)
 * All rights reserved.
 *
 * This package is an SSL implementation written
 * by Eric Young (eay@cryptsoft.com).
 * The implementation was written so as to conform with Netscapes SSL.
 *
 * This library is free for commercial and non-commercial use as long as
 * the following conditions are aheared to.  The following conditions
 * apply to all code found in this distribution, be it the RC4, RSA,
 * lhash, DES, etc., code; not just the SSL code.  The SSL documentation
 * included with this distribution is covered by the same copyright terms
 * except that the holder is Tim Hudson (tjh@cryptsoft.com).
 *
 * Copyright remains Eric Young's, and as such any Copyright notices in
 * the code are not to be removed.
 * If this package is used in a product, Eric Young should be given attribution
 * as the author of the parts of the library used.
 * This can be in the form of a textual message at program startup or
 * in documentation (online or textual) provided with the package.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 * 1. Redistributions of source code must retain the copyright
 *    notice, this list of conditions and the following disclaimer.
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in the
 *    documentation and/or other materials provided with the distribution.
 * 3. All advertising materials mentioning features or use of this software
 *    must display the following acknowledgement:
 *    "This product includes cryptographic software written by
 *     Eric Young (eay@cryptsoft.com)"
 *    The word 'cryptographic' can be left out if the rouines from the library
 *    being used are not cryptographic related :-).
 * 4. If you include any Windows specific code (or a derivative thereof) from
 *    the apps directory (application code) you must include an acknowledgement:
 *    "This product includes software written by Tim Hudson (tjh@cryptsoft.com)"
 *
 * THIS SOFTWARE IS PROVIDED BY ERIC YOUNG ``AS IS'' AND
 * ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
 * ARE DISCLAIMED.  IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE
 * FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
 * DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
 * OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
 * LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
 * OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
 * SUCH DAMAGE.
 *
 * The licence and distribution terms for any publically available version or
 * derivative of this code cannot be changed.  i.e. this code cannot simply be
 * copied and put under another distribution licence
 * [including the GNU Public Licence.]
 */
/* ====================================================================
 * Copyright (c) 1998-2006 The OpenSSL Project.  All rights reserved.
 *
 * Redistribution and use in source and binary forms, with or without
 * modification, are permitted provided that the following conditions
 * are met:
 *
 * 1. Redistributions of source code must retain the above copyright
 *    notice, this list of conditions and the following disclaimer.
 *
 * 2. Redistributions in binary form must reproduce the above copyright
 *    notice, this list of conditions and the following disclaimer in
 *    the documentation and/or other materials provided with the
 *    distribution.
 *
 * 3. All advertising materials mentioning features or use of this
 *    software must display the following acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit. (http://www.openssl.org/)"
 *
 * 4. The names "OpenSSL Toolkit" and "OpenSSL Project" must not be used to
 *    endorse or promote products derived from this software without
 *    prior written permission. For written permission, please contact
 *    openssl-core@openssl.org.
 *
 * 5. Products derived from this software may not be called "OpenSSL"
 *    nor may "OpenSSL" appear in their names without prior written
 *    permission of the OpenSSL Project.
 *
 * 6. Redistributions of any form whatsoever must retain the following
 *    acknowledgment:
 *    "This product includes software developed by the OpenSSL Project
 *    for use in the OpenSSL Toolkit (http://www.openssl.org/)"
 *
 * THIS SOFTWARE IS PROVIDED BY THE OpenSSL PROJECT ``AS IS'' AND ANY
 * EXPRESSED OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
 * IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
 * PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE OpenSSL PROJECT OR
 * ITS CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
 * SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
 * NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
 * LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
 * HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
 * STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
 * ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED
 * OF THE POSSIBILITY OF SUCH DAMAGE.
 * ====================================================================
 *
 * This product includes cryptographic software written by Eric Young
 * (eay@cryptsoft.com).  This product includes software written by Tim
 * Hudson (tjh@cryptsoft.com). */

#ifndef OPENSSL_HEADER_ERR_H
#define OPENSSL_HEADER_ERR_H

#include <stdio.h>

#include <openssl/base.h>
#include <openssl/crypto.h>
#include <errno.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Error queue handling functions.
//
// Errors in OpenSSL are generally signaled by the return value of a function.
// When a function fails it may add an entry to a per-thread error queue,
// which is managed by the functions in this header.
//
// Each error contains:
//   1) The library (i.e. ec, pem, rsa) which created it.
//   2) The file and line number of the call that added the error.
//   3) A pointer to some error specific data, which may be NULL.
//
// The library identifier and reason code are packed in a uint32_t and there
// exist various functions for unpacking it.
//
// The typical behaviour is that an error will occur deep in a call queue and
// that code will push an error onto the error queue. As the error queue
// unwinds, other functions will push their own errors. Thus, the "least
// recent" error is the most specific and the other errors will provide a
// backtrace of sorts.


// Startup and shutdown.

// ERR_load_BIO_strings does nothing.
//
// TODO(fork): remove. libjingle calls this.
OPENSSL_EXPORT void ERR_load_BIO_strings(void);

// ERR_load_ERR_strings does nothing.
OPENSSL_EXPORT void ERR_load_ERR_strings(void);

// ERR_load_CRYPTO_strings does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void ERR_load_CRYPTO_strings(void);

// ERR_load_crypto_strings does nothing.
OPENSSL_EXPORT void ERR_load_crypto_strings(void);

// ERR_load_RAND_strings does nothing.
OPENSSL_EXPORT void ERR_load_RAND_strings(void);

// ERR_free_strings does nothing.
OPENSSL_EXPORT void ERR_free_strings(void);


// Reading and formatting errors.

// ERR_GET_LIB returns the library code for the error. This is one of
// the |ERR_LIB_*| values.
OPENSSL_INLINE int ERR_GET_LIB(uint32_t packed_error) {
  return (int)((packed_error >> 24) & 0xff);
}

// ERR_GET_REASON returns the reason code for the error. This is one of
// library-specific |LIB_R_*| values where |LIB| is the library (see
// |ERR_GET_LIB|). Note that reason codes are specific to the library.
OPENSSL_INLINE int ERR_GET_REASON(uint32_t packed_error) {
  return (int)(packed_error & 0xfff);
}

// ERR_get_error gets the packed error code for the least recent error and
// removes that error from the queue. If there are no errors in the queue then
// it returns zero.
OPENSSL_EXPORT uint32_t ERR_get_error(void);

// ERR_get_error_line acts like |ERR_get_error|, except that the file and line
// number of the call that added the error are also returned.
OPENSSL_EXPORT uint32_t ERR_get_error_line(const char **file, int *line);

// ERR_FLAG_STRING means that the |data| member is a NUL-terminated string that
// can be printed. This is always set if |data| is non-NULL.
#define ERR_FLAG_STRING 1

// ERR_FLAG_MALLOCED is passed into |ERR_set_error_data| to indicate that |data|
// was allocated with |OPENSSL_malloc|.
//
// It is, separately, returned in |*flags| from |ERR_get_error_line_data| to
// indicate that |*data| has a non-static lifetime, but this lifetime is still
// managed by the library. The caller must not call |OPENSSL_free| or |free| on
// |data|.
#define ERR_FLAG_MALLOCED 2

// ERR_get_error_line_data acts like |ERR_get_error_line|, but also returns the
// error-specific data pointer and flags. The flags are a bitwise-OR of
// |ERR_FLAG_*| values. The error-specific data is owned by the error queue
// and the pointer becomes invalid after the next call that affects the same
// thread's error queue. If |*flags| contains |ERR_FLAG_STRING| then |*data| is
// human-readable.
OPENSSL_EXPORT uint32_t ERR_get_error_line_data(const char **file, int *line,
                                                const char **data, int *flags);

// The "peek" functions act like the |ERR_get_error| functions, above, but they
// do not remove the error from the queue.
OPENSSL_EXPORT uint32_t ERR_peek_error(void);
OPENSSL_EXPORT uint32_t ERR_peek_error_line(const char **file, int *line);
OPENSSL_EXPORT uint32_t ERR_peek_error_line_data(const char **file, int *line,
                                                 const char **data, int *flags);

// The "peek last" functions act like the "peek" functions, above, except that
// they return the most recent error.
OPENSSL_EXPORT uint32_t ERR_peek_last_error(void);
OPENSSL_EXPORT uint32_t ERR_peek_last_error_line(const char **file, int *line);
OPENSSL_EXPORT uint32_t ERR_peek_last_error_line_data(const char **file,
                                                      int *line,
                                                      const char **data,
                                                      int *flags);

// ERR_error_string_n generates a human-readable string representing
// |packed_error|, places it at |buf|, and returns |buf|. It writes at most
// |len| bytes (including the terminating NUL) and truncates the string if
// necessary. If |len| is greater than zero then |buf| is always NUL terminated.
//
// The string will have the following format:
//
//   error:[error code]:[library name]:OPENSSL_internal:[reason string]
//
// error code is an 8 digit hexadecimal number; library name and reason string
// are ASCII text.
OPENSSL_EXPORT char *ERR_error_string_n(uint32_t packed_error, char *buf,
                                        size_t len);

// ERR_lib_error_string returns a string representation of the library that
// generated |packed_error|, or a placeholder string is the library is
// unrecognized.
OPENSSL_EXPORT const char *ERR_lib_error_string(uint32_t packed_error);

// ERR_reason_error_string returns a string representation of the reason for
// |packed_error|, or a placeholder string if the reason is unrecognized.
OPENSSL_EXPORT const char *ERR_reason_error_string(uint32_t packed_error);

// ERR_print_errors_callback_t is the type of a function used by
// |ERR_print_errors_cb|. It takes a pointer to a human readable string (and
// its length) that describes an entry in the error queue. The |ctx| argument
// is an opaque pointer given to |ERR_print_errors_cb|.
//
// It should return one on success or zero on error, which will stop the
// iteration over the error queue.
typedef int (*ERR_print_errors_callback_t)(const char *str, size_t len,
                                           void *ctx);

// ERR_print_errors_cb clears the current thread's error queue, calling
// |callback| with a string representation of each error, from the least recent
// to the most recent error.
//
// The string will have the following format (which differs from
// |ERR_error_string|):
//
//   [thread id]:error:[error code]:[library name]:OPENSSL_internal:[reason string]:[file]:[line number]:[optional string data]
//
// The callback can return one to continue the iteration or zero to stop it.
// The |ctx| argument is an opaque value that is passed through to the
// callback.
OPENSSL_EXPORT void ERR_print_errors_cb(ERR_print_errors_callback_t callback,
                                        void *ctx);

// ERR_print_errors_fp clears the current thread's error queue, printing each
// error to |file|. See |ERR_print_errors_cb| for the format.
OPENSSL_EXPORT void ERR_print_errors_fp(FILE *file);


// Clearing errors.

// ERR_clear_error clears the error queue for the current thread.
OPENSSL_EXPORT void ERR_clear_error(void);

// ERR_set_mark "marks" the most recent error for use with |ERR_pop_to_mark|.
// It returns one if an error was marked and zero if there are no errors.
OPENSSL_EXPORT int ERR_set_mark(void);

// ERR_pop_to_mark removes errors from the most recent to the least recent
// until (and not including) a "marked" error. It returns zero if no marked
// error was found (and thus all errors were removed) and one otherwise. Errors
// are marked using |ERR_set_mark|.
OPENSSL_EXPORT int ERR_pop_to_mark(void);


// Custom errors.

// ERR_get_next_error_library returns a value suitable for passing as the
// |library| argument to |ERR_put_error|. This is intended for code that wishes
// to push its own, non-standard errors to the error queue.
OPENSSL_EXPORT int ERR_get_next_error_library(void);


// Built-in library and reason codes.

// The following values are built-in library codes.
#define ERR_LIB_NONE            1
#define ERR_LIB_SYS             2
#define ERR_LIB_BN              3
#define ERR_LIB_RSA             4
#define ERR_LIB_DH              5
#define ERR_LIB_EVP             6
#define ERR_LIB_BUF             7
#define ERR_LIB_OBJ             8
#define ERR_LIB_PEM             9
#define ERR_LIB_DSA             10
#define ERR_LIB_X509            11
#define ERR_LIB_ASN1            12
#define ERR_LIB_CONF            13
#define ERR_LIB_CRYPTO          14
#define ERR_LIB_EC              15
#define ERR_LIB_SSL             16
#define ERR_LIB_BIO             17
#define ERR_LIB_PKCS7           18
#define ERR_LIB_PKCS8           19
#define ERR_LIB_X509V3          20
#define ERR_LIB_RAND            21
#define ERR_LIB_ENGINE          22
#define ERR_LIB_OCSP            23
#define ERR_LIB_UI              24
#define ERR_LIB_COMP            25
#define ERR_LIB_ECDSA           26
#define ERR_LIB_ECDH            27
#define ERR_LIB_HMAC            28
#define ERR_LIB_DIGEST          29
#define ERR_LIB_CIPHER          30
#define ERR_LIB_HKDF            31
#define ERR_LIB_TRUST_TOKEN     32
#define ERR_LIB_USER            33
#define ERR_NUM_LIBS            34
#define ERR_LIB_PKCS12          35
#define ERR_LIB_DSO             36
#define ERR_LIB_OSSL_STORE      37
#define ERR_LIB_FIPS            38
#define ERR_LIB_CMS             39
#define ERR_LIB_TS              40
#define ERR_LIB_CT              41
#define ERR_LIB_ASYNC           42
#define ERR_LIB_KDF             43
#define ERR_LIB_SM2             44

// The following reason codes used to denote an error occuring in another
// library. They are sometimes used for a stack trace.
#define ERR_R_SYS_LIB ERR_LIB_SYS
#define ERR_R_BN_LIB ERR_LIB_BN
#define ERR_R_RSA_LIB ERR_LIB_RSA
#define ERR_R_DH_LIB ERR_LIB_DH
#define ERR_R_EVP_LIB ERR_LIB_EVP
#define ERR_R_BUF_LIB ERR_LIB_BUF
#define ERR_R_OBJ_LIB ERR_LIB_OBJ
#define ERR_R_PEM_LIB ERR_LIB_PEM
#define ERR_R_DSA_LIB ERR_LIB_DSA
#define ERR_R_X509_LIB ERR_LIB_X509
#define ERR_R_ASN1_LIB ERR_LIB_ASN1
#define ERR_R_CONF_LIB ERR_LIB_CONF
#define ERR_R_CRYPTO_LIB ERR_LIB_CRYPTO
#define ERR_R_EC_LIB ERR_LIB_EC
#define ERR_R_SSL_LIB ERR_LIB_SSL
#define ERR_R_BIO_LIB ERR_LIB_BIO
#define ERR_R_PKCS7_LIB ERR_LIB_PKCS7
#define ERR_R_PKCS8_LIB ERR_LIB_PKCS8
#define ERR_R_X509V3_LIB ERR_LIB_X509V3
#define ERR_R_RAND_LIB ERR_LIB_RAND
#define ERR_R_DSO_LIB ERR_LIB_DSO
#define ERR_R_ENGINE_LIB ERR_LIB_ENGINE
#define ERR_R_OCSP_LIB ERR_LIB_OCSP
#define ERR_R_UI_LIB ERR_LIB_UI
#define ERR_R_COMP_LIB ERR_LIB_COMP
#define ERR_R_ECDSA_LIB ERR_LIB_ECDSA
#define ERR_R_ECDH_LIB ERR_LIB_ECDH
#define ERR_R_STORE_LIB ERR_LIB_STORE
#define ERR_R_FIPS_LIB ERR_LIB_FIPS
#define ERR_R_CMS_LIB ERR_LIB_CMS
#define ERR_R_TS_LIB ERR_LIB_TS
#define ERR_R_HMAC_LIB ERR_LIB_HMAC
#define ERR_R_JPAKE_LIB ERR_LIB_JPAKE
#define ERR_R_USER_LIB ERR_LIB_USER
#define ERR_R_DIGEST_LIB ERR_LIB_DIGEST
#define ERR_R_CIPHER_LIB ERR_LIB_CIPHER
#define ERR_R_HKDF_LIB ERR_LIB_HKDF
#define ERR_R_TRUST_TOKEN_LIB ERR_LIB_TRUST_TOKEN

// The following values are global reason codes. They may occur in any library.
#define ERR_R_FATAL 64
#define ERR_R_MALLOC_FAILURE (1 | ERR_R_FATAL)
#define ERR_R_SHOULD_NOT_HAVE_BEEN_CALLED (2 | ERR_R_FATAL)
#define ERR_R_PASSED_NULL_PARAMETER (3 | ERR_R_FATAL)
#define ERR_R_INTERNAL_ERROR (4 | ERR_R_FATAL)
#define ERR_R_OVERFLOW (5 | ERR_R_FATAL)


// Deprecated functions.

// ERR_remove_state calls |ERR_clear_error|.
OPENSSL_EXPORT void ERR_remove_state(unsigned long pid);

// ERR_remove_thread_state clears the error queue for the current thread if
// |tid| is NULL. Otherwise it calls |assert(0)|, because it's no longer
// possible to delete the error queue for other threads.
//
// Use |ERR_clear_error| instead. Note error queues are deleted automatically on
// thread exit. You do not need to call this function to release memory.
OPENSSL_EXPORT void ERR_remove_thread_state(const CRYPTO_THREADID *tid);

// ERR_func_error_string returns the string "OPENSSL_internal".
OPENSSL_EXPORT const char *ERR_func_error_string(uint32_t packed_error);

// ERR_error_string behaves like |ERR_error_string_n| but |len| is implicitly
// |ERR_ERROR_STRING_BUF_LEN|.
//
// Additionally, if |buf| is NULL, the error string is placed in a static buffer
// which is returned. This is not thread-safe and only exists for backwards
// compatibility with legacy callers. The static buffer will be overridden by
// calls in other threads.
//
// Use |ERR_error_string_n| instead.
//
// TODO(fork): remove this function.
OPENSSL_EXPORT char *ERR_error_string(uint32_t packed_error, char *buf);
#define ERR_ERROR_STRING_BUF_LEN 120

// ERR_GET_FUNC returns zero. BoringSSL errors do not report a function code.
OPENSSL_INLINE int ERR_GET_FUNC(uint32_t packed_error) {
  (void)packed_error;
  return 0;
}

// ERR_TXT_* are provided for compatibility with code that assumes that it's
// using OpenSSL.
#define ERR_TXT_STRING ERR_FLAG_STRING
#define ERR_TXT_MALLOCED ERR_FLAG_MALLOCED


// Private functions.

// ERR_clear_system_error clears the system's error value (i.e. errno).
OPENSSL_EXPORT void ERR_clear_system_error(void);

// OPENSSL_PUT_ERROR is used by OpenSSL code to add an error to the error
// queue.
#define OPENSSL_PUT_ERROR(library, reason) \
  ERR_put_error(ERR_LIB_##library, 0, reason, __FILE__, __LINE__)

// OPENSSL_PUT_SYSTEM_ERROR is used by OpenSSL code to add an error from the
// operating system to the error queue.
// TODO(fork): include errno.
#define OPENSSL_PUT_SYSTEM_ERROR() \
  ERR_put_error(ERR_LIB_SYS, 0, 0, __FILE__, __LINE__);

// ERR_put_error adds an error to the error queue, dropping the least recent
// error if necessary for space reasons.
OPENSSL_EXPORT void ERR_put_error(int library, int unused, int reason,
                                  const char *file, unsigned line);

// ERR_add_error_data takes a variable number (|count|) of const char*
// pointers, concatenates them and sets the result as the data on the most
// recent error.
OPENSSL_EXPORT void ERR_add_error_data(unsigned count, ...);

// ERR_add_error_dataf takes a printf-style format and arguments, and sets the
// result as the data on the most recent error.
OPENSSL_EXPORT void ERR_add_error_dataf(const char *format, ...)
    OPENSSL_PRINTF_FORMAT_FUNC(1, 2);

// ERR_set_error_data sets the data on the most recent error to |data|, which
// must be a NUL-terminated string. |flags| must contain |ERR_FLAG_STRING|. If
// |flags| contains |ERR_FLAG_MALLOCED|, this function takes ownership of
// |data|, which must have been allocated with |OPENSSL_malloc|. Otherwise, it
// saves a copy of |data|.
//
// Note this differs from OpenSSL which, when |ERR_FLAG_MALLOCED| is unset,
// saves the pointer as-is and requires it remain valid for the lifetime of the
// address space.
OPENSSL_EXPORT void ERR_set_error_data(char *data, int flags);

// ERR_NUM_ERRORS is one more than the limit of the number of errors in the
// queue.
#define ERR_NUM_ERRORS 16

#define ERR_PACK(lib, reason)                                              \
  (((((uint32_t)(lib)) & 0xff) << 24) | ((((uint32_t)(reason)) & 0xfff)))

// OPENSSL_DECLARE_ERROR_REASON is used by util/make_errors.h (which generates
// the error defines) to recognise that an additional reason value is needed.
// This is needed when the reason value is used outside of an
// |OPENSSL_PUT_ERROR| macro. The resulting define will be
// ${lib}_R_${reason}.
#define OPENSSL_DECLARE_ERROR_REASON(lib, reason)


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_ERR_H
