/* ====================================================================
 * Copyright (c) 2002-2006 The OpenSSL Project.  All rights reserved.
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
 * ==================================================================== */

#ifndef OPENSSL_HEADER_AES_H
#define OPENSSL_HEADER_AES_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Raw AES functions.


#define AES_ENCRYPT 1
#define AES_DECRYPT 0

// AES_MAXNR is the maximum number of AES rounds.
#define AES_MAXNR 14

#define AES_BLOCK_SIZE 16

// aes_key_st should be an opaque type, but EVP requires that the size be
// known.
struct aes_key_st {
  uint32_t rd_key[4 * (AES_MAXNR + 1)];
  unsigned rounds;
};
typedef struct aes_key_st AES_KEY;

// AES_set_encrypt_key configures |aeskey| to encrypt with the |bits|-bit key,
// |key|. |key| must point to |bits|/8 bytes. It returns zero on success and a
// negative number if |bits| is an invalid AES key size.
//
// WARNING: this function breaks the usual return value convention.
OPENSSL_EXPORT int AES_set_encrypt_key(const uint8_t *key, unsigned bits,
                                       AES_KEY *aeskey);

// AES_set_decrypt_key configures |aeskey| to decrypt with the |bits|-bit key,
// |key|. |key| must point to |bits|/8 bytes. It returns zero on success and a
// negative number if |bits| is an invalid AES key size.
//
// WARNING: this function breaks the usual return value convention.
OPENSSL_EXPORT int AES_set_decrypt_key(const uint8_t *key, unsigned bits,
                                       AES_KEY *aeskey);

// AES_encrypt encrypts a single block from |in| to |out| with |key|. The |in|
// and |out| pointers may overlap.
OPENSSL_EXPORT void AES_encrypt(const uint8_t *in, uint8_t *out,
                                const AES_KEY *key);

// AES_decrypt decrypts a single block from |in| to |out| with |key|. The |in|
// and |out| pointers may overlap.
OPENSSL_EXPORT void AES_decrypt(const uint8_t *in, uint8_t *out,
                                const AES_KEY *key);


// Block cipher modes.

// AES_ctr128_encrypt encrypts (or decrypts, it's the same in CTR mode) |len|
// bytes from |in| to |out|. The |num| parameter must be set to zero on the
// first call and |ivec| will be incremented. This function may be called
// in-place with |in| equal to |out|, but otherwise the buffers may not
// partially overlap. A partial overlap may overwrite input data before it is
// read.
OPENSSL_EXPORT void AES_ctr128_encrypt(const uint8_t *in, uint8_t *out,
                                       size_t len, const AES_KEY *key,
                                       uint8_t ivec[AES_BLOCK_SIZE],
                                       uint8_t ecount_buf[AES_BLOCK_SIZE],
                                       unsigned int *num);

// AES_ecb_encrypt encrypts (or decrypts, if |enc| == |AES_DECRYPT|) a single,
// 16 byte block from |in| to |out|. This function may be called in-place with
// |in| equal to |out|, but otherwise the buffers may not partially overlap. A
// partial overlap may overwrite input data before it is read.
OPENSSL_EXPORT void AES_ecb_encrypt(const uint8_t *in, uint8_t *out,
                                    const AES_KEY *key, const int enc);

// AES_cbc_encrypt encrypts (or decrypts, if |enc| == |AES_DECRYPT|) |len|
// bytes from |in| to |out|. The length must be a multiple of the block size.
// This function may be called in-place with |in| equal to |out|, but otherwise
// the buffers may not partially overlap. A partial overlap may overwrite input
// data before it is read.
OPENSSL_EXPORT void AES_cbc_encrypt(const uint8_t *in, uint8_t *out, size_t len,
                                    const AES_KEY *key, uint8_t *ivec,
                                    const int enc);

// AES_ofb128_encrypt encrypts (or decrypts, it's the same in OFB mode) |len|
// bytes from |in| to |out|. The |num| parameter must be set to zero on the
// first call. This function may be called in-place with |in| equal to |out|,
// but otherwise the buffers may not partially overlap. A partial overlap may
// overwrite input data before it is read.
OPENSSL_EXPORT void AES_ofb128_encrypt(const uint8_t *in, uint8_t *out,
                                       size_t len, const AES_KEY *key,
                                       uint8_t *ivec, int *num);

// AES_cfb1_encrypt encrypts (or decrypts, if |enc| == |AES_DECRYPT|) |len|
// bytes from |in| to |out|. The |num| parameter must be set to zero on the
// first call. This function may be called in-place with |in| equal to |out|,
// but otherwise the buffers may not partially overlap. A partial overlap may
// overwrite input data before it is read.
OPENSSL_EXPORT void AES_cfb1_encrypt(const uint8_t *in, uint8_t *out,
                                     size_t bits, const AES_KEY *key,
                                     uint8_t *ivec, int *num, int enc);

// AES_cfb8_encrypt encrypts (or decrypts, if |enc| == |AES_DECRYPT|) |len|
// bytes from |in| to |out|. The |num| parameter must be set to zero on the
// first call. This function may be called in-place with |in| equal to |out|,
// but otherwise the buffers may not partially overlap. A partial overlap may
// overwrite input data before it is read.
OPENSSL_EXPORT void AES_cfb8_encrypt(const uint8_t *in, uint8_t *out,
                                     size_t len, const AES_KEY *key,
                                     uint8_t *ivec, int *num, int enc);

// AES_cfb128_encrypt encrypts (or decrypts, if |enc| == |AES_DECRYPT|) |len|
// bytes from |in| to |out|. The |num| parameter must be set to zero on the
// first call. This function may be called in-place with |in| equal to |out|,
// but otherwise the buffers may not partially overlap. A partial overlap may
// overwrite input data before it is read.
OPENSSL_EXPORT void AES_cfb128_encrypt(const uint8_t *in, uint8_t *out,
                                       size_t len, const AES_KEY *key,
                                       uint8_t *ivec, int *num, int enc);


// AES key wrap.
//
// These functions implement AES Key Wrap mode, as defined in RFC 3394. They
// should never be used except to interoperate with existing systems that use
// this mode.

// AES_wrap_key performs AES key wrap on |in| which must be a multiple of 8
// bytes. |iv| must point to an 8 byte value or be NULL to use the default IV.
// |key| must have been configured for encryption. On success, it writes
// |in_len| + 8 bytes to |out| and returns |in_len| + 8. Otherwise, it returns
// -1.
OPENSSL_EXPORT int AES_wrap_key(const AES_KEY *key, const uint8_t *iv,
                                uint8_t *out, const uint8_t *in, size_t in_len);

// AES_unwrap_key performs AES key unwrap on |in| which must be a multiple of 8
// bytes. |iv| must point to an 8 byte value or be NULL to use the default IV.
// |key| must have been configured for decryption. On success, it writes
// |in_len| - 8 bytes to |out| and returns |in_len| - 8. Otherwise, it returns
// -1.
OPENSSL_EXPORT int AES_unwrap_key(const AES_KEY *key, const uint8_t *iv,
                                  uint8_t *out, const uint8_t *in,
                                  size_t in_len);


// AES key wrap with padding.
//
// These functions implement AES Key Wrap with Padding mode, as defined in RFC
// 5649. They should never be used except to interoperate with existing systems
// that use this mode.

// AES_wrap_key_padded performs a padded AES key wrap on |in| which must be
// between 1 and 2^32-1 bytes. |key| must have been configured for encryption.
// On success it writes at most |max_out| bytes of ciphertext to |out|, sets
// |*out_len| to the number of bytes written, and returns one. On failure it
// returns zero. To ensure success, set |max_out| to at least |in_len| + 15.
OPENSSL_EXPORT int AES_wrap_key_padded(const AES_KEY *key, uint8_t *out,
                                       size_t *out_len, size_t max_out,
                                       const uint8_t *in, size_t in_len);

// AES_unwrap_key_padded performs a padded AES key unwrap on |in| which must be
// a multiple of 8 bytes. |key| must have been configured for decryption. On
// success it writes at most |max_out| bytes to |out|, sets |*out_len| to the
// number of bytes written, and returns one. On failure it returns zero. Setting
// |max_out| to |in_len| is a sensible estimate.
OPENSSL_EXPORT int AES_unwrap_key_padded(const AES_KEY *key, uint8_t *out,
                                         size_t *out_len, size_t max_out,
                                         const uint8_t *in, size_t in_len);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_AES_H
