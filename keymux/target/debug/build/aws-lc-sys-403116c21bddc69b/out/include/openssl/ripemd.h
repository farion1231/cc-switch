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

#ifndef OPENSSL_HEADER_RIPEMD_H
#define OPENSSL_HEADER_RIPEMD_H

#include <openssl/base.h>

#ifdef  __cplusplus
extern "C" {
#endif


# define RIPEMD160_CBLOCK        64
# define RIPEMD160_LBLOCK        (RIPEMD160_CBLOCK/4)
# define RIPEMD160_DIGEST_LENGTH 20

struct RIPEMD160state_st {
  uint32_t h[5];
  uint32_t Nl, Nh;
  uint8_t data[RIPEMD160_CBLOCK];
  unsigned num;
};

// RIPEMD160_Init initialises |ctx| and returns one.
OPENSSL_EXPORT int RIPEMD160_Init(RIPEMD160_CTX *ctx);

// RIPEMD160_Update adds |len| bytes from |data| to |ctx| and returns one.
OPENSSL_EXPORT int RIPEMD160_Update(RIPEMD160_CTX *ctx, const void *data,
                                   size_t len);

// RIPEMD160_Final adds the final padding to |ctx| and writes the resulting
// digest to |out|, which must have at least |RIPEMD160_DIGEST_LENGTH| bytes of
// space. It returns one.
OPENSSL_EXPORT int RIPEMD160_Final(uint8_t out[RIPEMD160_DIGEST_LENGTH],
                                   RIPEMD160_CTX *ctx);

// RIPEMD160 writes the digest of |len| bytes from |data| to |out| and returns
// |out|. There must be at least |RIPEMD160_DIGEST_LENGTH| bytes of space in
// |out|.
OPENSSL_EXPORT uint8_t *RIPEMD160(const uint8_t *data, size_t len,
                                  uint8_t out[RIPEMD160_DIGEST_LENGTH]);

#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_RIPEMD_H
