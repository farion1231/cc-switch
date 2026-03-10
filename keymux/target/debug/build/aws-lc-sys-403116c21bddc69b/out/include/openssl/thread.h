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
 * [including the GNU Public Licence.] */

#ifndef OPENSSL_HEADER_THREAD_H
#define OPENSSL_HEADER_THREAD_H

#include <sys/types.h>

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


#if !defined(OPENSSL_THREADS)
typedef struct crypto_mutex_st {
  char padding;  // Empty structs have different sizes in C and C++.
} CRYPTO_MUTEX;
#elif defined(OPENSSL_WINDOWS)
// CRYPTO_MUTEX can appear in public header files so we really don't want to
// pull in windows.h. It's statically asserted that this structure is large
// enough to contain a Windows SRWLOCK by thread_win.c.
typedef union crypto_mutex_st {
  void *handle;
} CRYPTO_MUTEX;
#elif !defined(__GLIBC__)
#if defined(OPENSSL_OPENBSD)
#include <pthread.h>
#endif
typedef pthread_rwlock_t CRYPTO_MUTEX;
#else
// On glibc, |pthread_rwlock_t| is hidden under feature flags, and we can't
// ensure that we'll be able to get it from a public header. It's statically
// asserted that this structure is large enough to contain a |pthread_rwlock_t|
// by thread_pthread.c.
typedef union crypto_mutex_st {
  double alignment;
  uint8_t padding[3 * sizeof(int) + 5 * sizeof(unsigned) + 16 + 8];
} CRYPTO_MUTEX;
#endif

// CRYPTO_refcount_t is the type of a reference count.
//
// Since some platforms use C11 atomics to access this, it should have the
// _Atomic qualifier. However, this header is included by C++ programs as well
// as C code that might not set -std=c11. So, in practice, it's not possible to
// do that. Instead we statically assert that the size and native alignment of
// a plain uint32_t and an _Atomic uint32_t are equal in refcount_c11.c.
typedef uint32_t CRYPTO_refcount_t;

// AWSLC_thread_local_clear destructs AWS-LC-related thread-local data.
// If no other AWS-LC function is subsequently called on this thread prior to
// its termination, our internal thread-local destructor function will not be
// invoked. If performed on all active threads, this may allow a shared
// AWS-LC library to be unloaded safely via |dlclose|.
OPENSSL_EXPORT int AWSLC_thread_local_clear(void);

// AWSLC_thread_local_shutdown deletes the key used to track thread-local data.
// This function is not thread-safe. It is needed to avoid leaking resources in
// consumers that use |dlopen|/|dlclose| to access the AWS-LC shared library.
// It should be called prior to |dlclose| after all other threads have completed
// calls to |AWSLC_thread_local_clear|.
OPENSSL_EXPORT int AWSLC_thread_local_shutdown(void);

// General No-op Functions [Deprecated].
//
// Historically, OpenSSL required callers to provide locking callbacks.
// BoringSSL is thread-safe by default, but some old code calls these functions
// and so no-op implementations are provided.

// These defines do nothing but are provided to make old code easier to
// compile.
#define CRYPTO_LOCK 1
#define CRYPTO_UNLOCK 2
#define CRYPTO_READ 4
#define CRYPTO_WRITE 8

// CRYPTO_num_locks returns one. (This is non-zero that callers who allocate
// sizeof(lock) times this value don't get zero and then fail because malloc(0)
// returned NULL.)
OPENSSL_EXPORT OPENSSL_DEPRECATED int CRYPTO_num_locks(void);

// CRYPTO_set_locking_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_locking_callback(
    void (*func)(int mode, int lock_num, const char *file, int line));

// CRYPTO_set_add_lock_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_add_lock_callback(int (*func)(
    int *num, int amount, int lock_num, const char *file, int line));

// CRYPTO_get_locking_callback returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED void (*CRYPTO_get_locking_callback(void))(
    int mode, int lock_num, const char *file, int line);

// CRYPTO_get_lock_name returns a fixed, dummy string.
OPENSSL_EXPORT OPENSSL_DEPRECATED const char *CRYPTO_get_lock_name(
    int lock_num);

// CRYPTO_THREADID_set_callback returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int CRYPTO_THREADID_set_callback(
    void (*threadid_func)(CRYPTO_THREADID *threadid));

// CRYPTO_THREADID_set_numeric does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_THREADID_set_numeric(
    CRYPTO_THREADID *id, unsigned long val);

// CRYPTO_THREADID_set_pointer does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_THREADID_set_pointer(
    CRYPTO_THREADID *id, void *ptr);

// CRYPTO_THREADID_current does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_THREADID_current(
    CRYPTO_THREADID *id);

// CRYPTO_set_id_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_id_callback(
    unsigned long (*func)(void));

typedef struct {
  int references;
  struct CRYPTO_dynlock_value *data;
} CRYPTO_dynlock;

// CRYPTO_set_dynlock_create_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_dynlock_create_callback(
    struct CRYPTO_dynlock_value *(*dyn_create_function)(const char *file,
                                                        int line));

// CRYPTO_set_dynlock_lock_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_dynlock_lock_callback(
    void (*dyn_lock_function)(int mode, struct CRYPTO_dynlock_value *l,
                              const char *file, int line));

// CRYPTO_set_dynlock_destroy_callback does nothing.
OPENSSL_EXPORT OPENSSL_DEPRECATED void CRYPTO_set_dynlock_destroy_callback(
    void (*dyn_destroy_function)(struct CRYPTO_dynlock_value *l,
                                 const char *file, int line));

// CRYPTO_get_dynlock_create_callback returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED struct CRYPTO_dynlock_value *(
    *CRYPTO_get_dynlock_create_callback(void))(const char *file, int line);

// CRYPTO_get_dynlock_lock_callback returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED void (*CRYPTO_get_dynlock_lock_callback(
    void))(int mode, struct CRYPTO_dynlock_value *l, const char *file,
           int line);

// CRYPTO_get_dynlock_destroy_callback returns NULL.
OPENSSL_EXPORT OPENSSL_DEPRECATED void (*CRYPTO_get_dynlock_destroy_callback(
    void))(struct CRYPTO_dynlock_value *l, const char *file, int line);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_THREAD_H
