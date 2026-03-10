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

#ifndef OPENSSL_HEADER_BUFFER_H
#define OPENSSL_HEADER_BUFFER_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Memory and string functions, see also mem.h.


// buf_mem_st (aka |BUF_MEM|) is a generic buffer object used by OpenSSL.
struct buf_mem_st {
  size_t length;  // current number of bytes
  char *data;
  size_t max;  // size of buffer
};

// BUF_MEM_new creates a new BUF_MEM which has no allocated data buffer.
OPENSSL_EXPORT BUF_MEM *BUF_MEM_new(void);

// BUF_MEM_free frees |buf->data| if needed and then frees |buf| itself.
OPENSSL_EXPORT void BUF_MEM_free(BUF_MEM *buf);

// BUF_MEM_reserve ensures |buf| has capacity |cap| and allocates memory if
// needed. It returns one on success and zero on error.
OPENSSL_EXPORT int BUF_MEM_reserve(BUF_MEM *buf, size_t cap);

// BUF_MEM_grow ensures that |buf| has length |len| and allocates memory if
// needed. If the length of |buf| increased, the new bytes are filled with
// zeros. It returns the length of |buf|, or zero if there's an error.
OPENSSL_EXPORT size_t BUF_MEM_grow(BUF_MEM *buf, size_t len);

// BUF_MEM_grow_clean calls |BUF_MEM_grow|. BoringSSL always zeros memory
// allocated memory on free.
OPENSSL_EXPORT size_t BUF_MEM_grow_clean(BUF_MEM *buf, size_t len);

// BUF_MEM_append appends |in| to |buf|. It returns one on success and zero on
// error.
OPENSSL_EXPORT int BUF_MEM_append(BUF_MEM *buf, const void *in, size_t len);


// Deprecated functions.

// BUF_strdup calls |OPENSSL_strdup|.
OPENSSL_EXPORT char *BUF_strdup(const char *str);

// BUF_strnlen calls |OPENSSL_strnlen|.
OPENSSL_EXPORT size_t BUF_strnlen(const char *str, size_t max_len);

// BUF_strndup calls |OPENSSL_strndup|.
OPENSSL_EXPORT char *BUF_strndup(const char *str, size_t size);

// BUF_memdup calls |OPENSSL_memdup|.
OPENSSL_EXPORT void *BUF_memdup(const void *data, size_t size);

// BUF_strlcpy calls |OPENSSL_strlcpy|.
OPENSSL_EXPORT size_t BUF_strlcpy(char *dst, const char *src, size_t dst_size);

// BUF_strlcat calls |OPENSSL_strlcat|.
OPENSSL_EXPORT size_t BUF_strlcat(char *dst, const char *src, size_t dst_size);


#if defined(__cplusplus)
}  // extern C

extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(BUF_MEM, BUF_MEM_free)

BSSL_NAMESPACE_END

}  // extern C++

#endif

#endif  // OPENSSL_HEADER_BUFFER_H
