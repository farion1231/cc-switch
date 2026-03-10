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

#ifndef OPENSSL_HEADER_MEM_H
#define OPENSSL_HEADER_MEM_H

#include <openssl/base.h>

#include <stdlib.h>
#include <stdarg.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Memory and string functions, see also buf.h.
//
// BoringSSL has its own set of allocation functions, which keep track of
// allocation lengths and zero them out before freeing. All memory returned by
// BoringSSL API calls must therefore generally be freed using |OPENSSL_free|
// unless stated otherwise.


#ifndef _BORINGSSL_PROHIBIT_OPENSSL_MALLOC
// OPENSSL_malloc is similar to a regular |malloc|, but allocates additional
// private data. The resulting pointer must be freed with |OPENSSL_free|. In
// the case of a malloc failure, prior to returning NULL |OPENSSL_malloc| will
// push |ERR_R_MALLOC_FAILURE| onto the openssl error stack.
OPENSSL_EXPORT void *OPENSSL_malloc(size_t size) OPENSSL_WARN_UNUSED_RESULT;

// OPENSSL_zalloc behaves like |OPENSSL_malloc| except it also initializes the
// resulting memory to zero.
OPENSSL_EXPORT void *OPENSSL_zalloc(size_t size) OPENSSL_WARN_UNUSED_RESULT;

// OPENSSL_calloc is similar to a regular |calloc|, but allocates data with
// |OPENSSL_malloc|. On overflow, it will push |ERR_R_OVERFLOW| onto the error
// queue.
OPENSSL_EXPORT void *OPENSSL_calloc(size_t num, size_t size) OPENSSL_WARN_UNUSED_RESULT;

// OPENSSL_realloc returns a pointer to a buffer of |new_size| bytes that
// contains the contents of |ptr|. Unlike |realloc|, a new buffer is always
// allocated and the data at |ptr| is always wiped and freed. Memory is
// allocated with |OPENSSL_malloc| and must be freed with |OPENSSL_free|.
// If |ptr| is null |OPENSSL_malloc| is called instead.
OPENSSL_EXPORT void *OPENSSL_realloc(void *ptr, size_t new_size) OPENSSL_WARN_UNUSED_RESULT;
#endif // !_BORINGSSL_PROHIBIT_OPENSSL_MALLOC

// OPENSSL_free does nothing if |ptr| is NULL. Otherwise it zeros out the
// memory allocated at |ptr| and frees it along with the private data.
// It must only be used on on |ptr| values obtained from |OPENSSL_malloc|
OPENSSL_EXPORT void OPENSSL_free(void *ptr);

// OPENSSL_cleanse zeros out |len| bytes of memory at |ptr|. This is similar to
// |memset_s| from C11.
OPENSSL_EXPORT void OPENSSL_cleanse(void *ptr, size_t len);

// CRYPTO_memcmp returns zero iff the |len| bytes at |a| and |b| are equal. It
// takes an amount of time dependent on |len|, but independent of the contents
// of |a| and |b|. Unlike memcmp, it cannot be used to put elements into a
// defined order as the return value when a != b is undefined, other than to be
// non-zero.
OPENSSL_EXPORT int CRYPTO_memcmp(const void *a, const void *b, size_t len) OPENSSL_WARN_UNUSED_RESULT;

// OPENSSL_hash32 implements the 32 bit, FNV-1a hash.
OPENSSL_EXPORT uint32_t OPENSSL_hash32(const void *ptr, size_t len);

// OPENSSL_strhash calls |OPENSSL_hash32| on the NUL-terminated string |s|.
OPENSSL_EXPORT uint32_t OPENSSL_strhash(const char *s);

// OPENSSL_strdup has the same behaviour as strdup(3).
OPENSSL_EXPORT char *OPENSSL_strdup(const char *s);

// OPENSSL_strnlen has the same behaviour as strnlen(3).
OPENSSL_EXPORT size_t OPENSSL_strnlen(const char *s, size_t len);

// OPENSSL_isalpha is a locale-independent, ASCII-only version of isalpha(3), It
// only recognizes 'a' through 'z' and 'A' through 'Z' as alphabetic.
OPENSSL_EXPORT int OPENSSL_isalpha(int c);

// OPENSSL_isdigit is a locale-independent, ASCII-only version of isdigit(3), It
// only recognizes '0' through '9' as digits.
OPENSSL_EXPORT int OPENSSL_isdigit(int c);

// OPENSSL_isxdigit is a locale-independent, ASCII-only version of isxdigit(3),
// It only recognizes '0' through '9', 'a' through 'f', and 'A through 'F' as
// digits.
OPENSSL_EXPORT int OPENSSL_isxdigit(int c);

// OPENSSL_fromxdigit returns one if |c| is a hexadecimal digit as recognized
// by OPENSSL_isxdigit, and sets |out| to the corresponding value. Otherwise
// zero is returned.
OPENSSL_EXPORT int OPENSSL_fromxdigit(uint8_t *out, int c);

// OPENSSL_hexstr2buf allocates and returns a buffer containing the bytes
// represented by the hexadecimal string |str|. |str| must be a NULL terminated
// string of hex characters. The length of the buffer is stored in |*len|.
// |len| must not be NULL. The caller must free the returned
// buffer with |OPENSSL_free|. If |str| is malformed, NULL is returned.
OPENSSL_EXPORT uint8_t *OPENSSL_hexstr2buf(const char *str, size_t *len);

// OPENSSL_isalnum is a locale-independent, ASCII-only version of isalnum(3), It
// only recognizes what |OPENSSL_isalpha| and |OPENSSL_isdigit| recognize.
OPENSSL_EXPORT int OPENSSL_isalnum(int c);

// OPENSSL_tolower is a locale-independent, ASCII-only version of tolower(3). It
// only lowercases ASCII values. Other values are returned as-is.
OPENSSL_EXPORT int OPENSSL_tolower(int c);

// OPENSSL_isspace is a locale-independent, ASCII-only version of isspace(3). It
// only recognizes '\t', '\n', '\v', '\f', '\r', and ' '.
OPENSSL_EXPORT int OPENSSL_isspace(int c);

// OPENSSL_strcasecmp is a locale-independent, ASCII-only version of
// strcasecmp(3).
OPENSSL_EXPORT int OPENSSL_strcasecmp(const char *a, const char *b);

// OPENSSL_strncasecmp is a locale-independent, ASCII-only version of
// strncasecmp(3).
OPENSSL_EXPORT int OPENSSL_strncasecmp(const char *a, const char *b, size_t n);

// DECIMAL_SIZE returns an upper bound for the length of the decimal
// representation of the given type.
#define DECIMAL_SIZE(type)	((sizeof(type)*8+2)/3+1)

// BIO_snprintf has the same behavior as snprintf(3).
OPENSSL_EXPORT int BIO_snprintf(char *buf, size_t n, const char *format, ...)
    OPENSSL_PRINTF_FORMAT_FUNC(3, 4);

// BIO_vsnprintf has the same behavior as vsnprintf(3).
OPENSSL_EXPORT int BIO_vsnprintf(char *buf, size_t n, const char *format,
                                 va_list args) OPENSSL_PRINTF_FORMAT_FUNC(3, 0);

// OPENSSL_vasprintf has the same behavior as vasprintf(3), except that
// memory allocated in a returned string must be freed with |OPENSSL_free|.
OPENSSL_EXPORT int OPENSSL_vasprintf(char **str, const char *format,
                                     va_list args)
    OPENSSL_PRINTF_FORMAT_FUNC(2, 0);

// OPENSSL_asprintf has the same behavior as asprintf(3), except that
// memory allocated in a returned string must be freed with |OPENSSL_free|.
OPENSSL_EXPORT int OPENSSL_asprintf(char **str, const char *format, ...)
    OPENSSL_PRINTF_FORMAT_FUNC(2, 3);

// OPENSSL_strndup returns an allocated, duplicate of |str|, which is, at most,
// |size| bytes. The result is always NUL terminated. The memory allocated
// must be freed with |OPENSSL_free|.
OPENSSL_EXPORT char *OPENSSL_strndup(const char *str, size_t size);

// OPENSSL_memdup returns an allocated, duplicate of |size| bytes from |data| or
// NULL on allocation failure. The memory allocated must be freed with
// |OPENSSL_free|.
OPENSSL_EXPORT void *OPENSSL_memdup(const void *data, size_t size);

// OPENSSL_strlcpy acts like strlcpy(3).
OPENSSL_EXPORT size_t OPENSSL_strlcpy(char *dst, const char *src,
                                      size_t dst_size);

// OPENSSL_strlcat acts like strlcat(3).
OPENSSL_EXPORT size_t OPENSSL_strlcat(char *dst, const char *src,
                                      size_t dst_size);


// Deprecated functions.

// CRYPTO_malloc calls |OPENSSL_malloc|. |file| and |line| are ignored.
OPENSSL_EXPORT void *CRYPTO_malloc(size_t size, const char *file, int line);

// CRYPTO_realloc calls |OPENSSL_realloc|. |file| and |line| are ignored.
OPENSSL_EXPORT void *CRYPTO_realloc(void *ptr, size_t new_size,
                                    const char *file, int line);

// CRYPTO_free calls |OPENSSL_free|. |file| and |line| are ignored.
OPENSSL_EXPORT void CRYPTO_free(void *ptr, const char *file, int line);

// OPENSSL_clear_free calls |OPENSSL_free|. BoringSSL automatically clears all
// allocations on free, but we define |OPENSSL_clear_free| for compatibility.
OPENSSL_EXPORT void OPENSSL_clear_free(void *ptr, size_t len);

// CRYPTO_set_mem_functions is used to override the implementation of |OPENSSL_malloc/free/realloc|.
//
// |OPENSSL_malloc/free/realloc| can be customized by implementing |OPENSSL_memory_alloc/free/realloc| or calling
// CRYPTO_set_mem_functions. If  |OPENSSL_memory_alloc/free/realloc| is defined CRYPTO_set_mem_functions will fail.
// All of the warnings for |OPENSSL_malloc/free/realloc| apply to CRYPTO_set_mem_functions:
// -- https://github.com/aws/aws-lc/blame/d164f5762b1ad5d4f2d1561fb85daa556fdff5ef/crypto/mem.c#L111-L127
// This function is only recommended for debug purpose(e.g. track mem usage).
// AWS-LC differs from OpenSSL's  CRYPTO_set_mem_functions in that __FILE__ and __LINE__ are not supplied.
//
// It returns one on success and zero otherwise.
OPENSSL_EXPORT int CRYPTO_set_mem_functions(
  void *(*m)(size_t, const char *, int),
  void *(*r)(void *, size_t, const char *, int),
  void (*f)(void *, const char *, int));

// OPENSSL supports the concept of secure heaps to help protect applications from pointer overruns or underruns that
// could return arbitrary data from the program's dynamic memory area where sensitive information may be stored.
// AWS-LC does not support secure heaps. The initialization functions intentionally return zero to indicate that secure
// heaps aren't supported. We return the regular malloc and zalloc versions when the secure_* counterparts are called,
// which is what OPENSSL does when secure heap is not enabled.
// If there is any interest in utilizing "secure heaps" with AWS-LC, cut us an issue at
// https://github.com/aws/aws-lc/issues/new/choose

// CRYPTO_secure_malloc_init returns zero.
OPENSSL_EXPORT int CRYPTO_secure_malloc_init(size_t size, size_t min_size);

// CRYPTO_secure_malloc_initialized returns zero.
OPENSSL_EXPORT int CRYPTO_secure_malloc_initialized(void);

// CRYPTO_secure_used returns zero.
OPENSSL_EXPORT size_t CRYPTO_secure_used(void);

// OPENSSL_secure_malloc calls |OPENSSL_malloc|.
OPENSSL_EXPORT void *OPENSSL_secure_malloc(size_t size);

// OPENSSL_secure_zalloc calls |OPENSSL_zalloc|.
OPENSSL_EXPORT void *OPENSSL_secure_zalloc(size_t size);

// OPENSSL_secure_clear_free calls |OPENSSL_clear_free|.
OPENSSL_EXPORT void OPENSSL_secure_clear_free(void *ptr, size_t len);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(char, OPENSSL_free)
BORINGSSL_MAKE_DELETER(uint8_t, OPENSSL_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#endif  // OPENSSL_HEADER_MEM_H
