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

#ifndef OPENSSL_HEADER_ENGINE_H
#define OPENSSL_HEADER_ENGINE_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Engines are collections of methods. Methods are tables of function pointers,
// defined for certain algorithms, that allow operations on those algorithms to
// be overridden via a callback. This can be used, for example, to implement an
// RSA* that forwards operations to a hardware module.
//
// Default Methods are zero initialized. You should set the function pointers
// that you wish before setting it on an |ENGINE|. Any functions pointers that
// are NULL indicate that the default behaviour should be used.


// Allocation and destruction.

// ENGINE_new returns an empty ENGINE that uses the default method for all
// algorithms.
OPENSSL_EXPORT ENGINE *ENGINE_new(void);

// ENGINE_free decrements the reference counts for all methods linked from
// |engine| and frees |engine| itself. It returns one.
OPENSSL_EXPORT int ENGINE_free(ENGINE *engine);


// Method accessors.
//
// The setter functions do not take ownership of the |method| pointer. The
// consumer must free the |method| pointer after all objects referencing it are
// freed.

// ENGINE_set_RSA takes a |method| pointer and sets it on the |ENGINE| object.
// Returns one on success and zero for failure when |engine| is NULL.
OPENSSL_EXPORT int ENGINE_set_RSA(ENGINE *engine, const RSA_METHOD *method);

// ENGINE_get_RSA returns the meth field of |engine|. If |engine| is NULL,
// function returns NULL.
OPENSSL_EXPORT const RSA_METHOD *ENGINE_get_RSA(const ENGINE *engine);

// ENGINE_set_EC takes a |method| pointer and sets it on the |ENGINE| object.
// Returns one on success and zero for failure when |engine| is NULL.
OPENSSL_EXPORT int ENGINE_set_EC(ENGINE *engine, const EC_KEY_METHOD *method);

// ENGINE_get_EC returns the meth field of |engine|. If |engine| is NULL,
// function returns NULL.
OPENSSL_EXPORT const EC_KEY_METHOD *ENGINE_get_EC(const ENGINE *engine);


// Deprecated functions.

// ENGINE_cleanup does nothing. This has been deprecated since OpenSSL 1.1.0 and
// applications should not rely on it.
OPENSSL_EXPORT void ENGINE_cleanup(void);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(ENGINE, ENGINE_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#define ENGINE_R_OPERATION_NOT_SUPPORTED 100

#endif  // OPENSSL_HEADER_ENGINE_H
