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

#ifndef OPENSSL_HEADER_BLOWFISH_H
#define OPENSSL_HEADER_BLOWFISH_H

#include <openssl/base.h>

#ifdef __cplusplus
extern "C" {
#endif


#define BF_ENCRYPT 1
#define BF_DECRYPT 0

#define BF_ROUNDS 16
#define BF_BLOCK 8

typedef struct bf_key_st {
  uint32_t P[BF_ROUNDS + 2];
  uint32_t S[4 * 256];
} BF_KEY;

OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_set_key(BF_KEY *key, size_t len,
                                                  const uint8_t *data);
OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_encrypt(uint32_t *data,
                                                  const BF_KEY *key);
OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_decrypt(uint32_t *data,
                                                  const BF_KEY *key);

OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_ecb_encrypt(const uint8_t *in,
                                                      uint8_t *out,
                                                      const BF_KEY *key,
                                                      int enc);
OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_cbc_encrypt(const uint8_t *in,
                                                      uint8_t *out,
                                                      size_t length,
                                                      const BF_KEY *schedule,
                                                      uint8_t *ivec, int enc);

OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_cfb64_encrypt(
    const uint8_t *in, uint8_t *out, size_t length, const BF_KEY *schedule,
    uint8_t *ivec, int *num, int encrypt);

OPENSSL_DEPRECATED OPENSSL_EXPORT void BF_ofb64_encrypt(
    const uint8_t *in, uint8_t *out, size_t length, const BF_KEY *schedule,
    uint8_t *ivec, int *num);

#ifdef __cplusplus
}
#endif

#endif  // OPENSSL_HEADER_BLOWFISH_H
