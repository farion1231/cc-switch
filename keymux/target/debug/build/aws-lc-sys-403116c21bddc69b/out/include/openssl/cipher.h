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

#ifndef OPENSSL_HEADER_CIPHER_H
#define OPENSSL_HEADER_CIPHER_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// Ciphers.


// Cipher primitives.
//
// The following functions return |EVP_CIPHER| objects that implement the named
// cipher algorithm.

OPENSSL_EXPORT const EVP_CIPHER *EVP_rc4(void);

OPENSSL_EXPORT const EVP_CIPHER *EVP_des_cbc(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ecb(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ede(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ede3(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ede_cbc(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ede3_cbc(void);

OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_ecb(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cbc(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_ctr(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_ofb(void);

OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_ecb(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cbc(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_ctr(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_ofb(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_xts(void);

// EVP_aes_256_wrap implements AES-256 in Key Wrap mode. OpenSSL 1.1.1 required
// |EVP_CIPHER_CTX_FLAG_WRAP_ALLOW| to be set with |EVP_CIPHER_CTX_set_flags|,
// in order for |EVP_aes_256_wrap| to work. This is not required in AWS-LC and
// they are no-op flags maintained for compatibility.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_wrap(void);

// EVP_enc_null returns a 'cipher' that passes plaintext through as
// ciphertext.
OPENSSL_EXPORT const EVP_CIPHER *EVP_enc_null(void);

// EVP_rc2_cbc returns a cipher that implements 128-bit RC2 in CBC mode.
OPENSSL_EXPORT const EVP_CIPHER *EVP_rc2_cbc(void);

// EVP_rc2_40_cbc returns a cipher that implements 40-bit RC2 in CBC mode. This
// is obviously very, very weak and is included only in order to read PKCS#12
// files, which often encrypt the certificate chain using this cipher. It is
// deliberately not exported.
const EVP_CIPHER *EVP_rc2_40_cbc(void);

// EVP_chacha20_poly1305 returns a cipher that implements chacha20-poly1305 as
// described in RFC 8439. This cipher implementation is added for
// compatibility. Consumers should use |EVP_aead_chacha20_poly1305| instead.
// Callers are advised that the maximum amount of data that can be encrypted
// using chacha20-poly1305 is 256GB.
OPENSSL_EXPORT const EVP_CIPHER *EVP_chacha20_poly1305(void);

// EVP_get_cipherbynid returns the cipher corresponding to the given NID, or
// NULL if no such cipher is known. Note using this function links almost every
// cipher implemented by BoringSSL into the binary, whether the caller uses them
// or not. Size-conscious callers, such as client software, should not use this
// function.
OPENSSL_EXPORT const EVP_CIPHER *EVP_get_cipherbynid(int nid);


// Cipher context allocation.
//
// An |EVP_CIPHER_CTX| represents the state of an encryption or decryption in
// progress.

// EVP_CIPHER_CTX_init initialises an, already allocated, |EVP_CIPHER_CTX|.
OPENSSL_EXPORT void EVP_CIPHER_CTX_init(EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_new allocates a fresh |EVP_CIPHER_CTX|, calls
// |EVP_CIPHER_CTX_init| and returns it, or NULL on allocation failure.
OPENSSL_EXPORT EVP_CIPHER_CTX *EVP_CIPHER_CTX_new(void);

// EVP_CIPHER_CTX_cleanup frees any memory referenced by |ctx|. It returns
// one.
OPENSSL_EXPORT int EVP_CIPHER_CTX_cleanup(EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_free calls |EVP_CIPHER_CTX_cleanup| on |ctx| and then frees
// |ctx| itself.
OPENSSL_EXPORT void EVP_CIPHER_CTX_free(EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_copy sets |out| to be a duplicate of the current state of
// |in|. The |out| argument must have been previously initialised.
OPENSSL_EXPORT int EVP_CIPHER_CTX_copy(EVP_CIPHER_CTX *out,
                                       const EVP_CIPHER_CTX *in);

// EVP_CIPHER_CTX_reset calls |EVP_CIPHER_CTX_cleanup| followed by
// |EVP_CIPHER_CTX_init| and returns one.
OPENSSL_EXPORT int EVP_CIPHER_CTX_reset(EVP_CIPHER_CTX *ctx);


// Cipher context configuration.

// EVP_CipherInit_ex configures |ctx| for a fresh encryption (or decryption, if
// |enc| is zero) operation using |cipher|. If |ctx| has been previously
// configured with a cipher then |cipher|, |key| and |iv| may be |NULL| and
// |enc| may be -1 to reuse the previous values. The operation will use |key|
// as the key and |iv| as the IV (if any). These should have the correct
// lengths given by |EVP_CIPHER_key_length| and |EVP_CIPHER_iv_length|. It
// returns one on success and zero on error.
OPENSSL_EXPORT int EVP_CipherInit_ex(EVP_CIPHER_CTX *ctx,
                                     const EVP_CIPHER *cipher, ENGINE *engine,
                                     const uint8_t *key, const uint8_t *iv,
                                     int enc);

// EVP_EncryptInit_ex calls |EVP_CipherInit_ex| with |enc| equal to one.
OPENSSL_EXPORT int EVP_EncryptInit_ex(EVP_CIPHER_CTX *ctx,
                                      const EVP_CIPHER *cipher, ENGINE *impl,
                                      const uint8_t *key, const uint8_t *iv);

// EVP_DecryptInit_ex calls |EVP_CipherInit_ex| with |enc| equal to zero.
OPENSSL_EXPORT int EVP_DecryptInit_ex(EVP_CIPHER_CTX *ctx,
                                      const EVP_CIPHER *cipher, ENGINE *impl,
                                      const uint8_t *key, const uint8_t *iv);


// Cipher operations.

// EVP_EncryptUpdate encrypts |in_len| bytes from |in| to |out|. The number
// of output bytes may be up to |in_len| plus the block length minus one and
// |out| must have sufficient space. The number of bytes actually output is
// written to |*out_len|. It returns one on success and zero otherwise.
//
// If |ctx| is an AEAD cipher, e.g. |EVP_aes_128_gcm|, and |out| is NULL, this
// function instead adds |in_len| bytes from |in| to the AAD and sets |*out_len|
// to |in_len|. The AAD must be fully specified in this way before this function
// is used to encrypt plaintext.
OPENSSL_EXPORT int EVP_EncryptUpdate(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                     int *out_len, const uint8_t *in,
                                     int in_len);

// EVP_EncryptFinal_ex writes at most a block of ciphertext to |out| and sets
// |*out_len| to the number of bytes written. If padding is enabled (the
// default) then standard padding is applied to create the final block. If
// padding is disabled (with |EVP_CIPHER_CTX_set_padding|) then any partial
// block remaining will cause an error. The function returns one on success and
// zero otherwise.
OPENSSL_EXPORT int EVP_EncryptFinal_ex(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                       int *out_len);

// EVP_DecryptUpdate decrypts |in_len| bytes from |in| to |out|. The number of
// output bytes may be up to |in_len| plus the block length minus one and |out|
// must have sufficient space. The number of bytes actually output is written
// to |*out_len|. It returns one on success and zero otherwise.
//
// If |ctx| is an AEAD cipher, e.g. |EVP_aes_128_gcm|, and |out| is NULL, this
// function instead adds |in_len| bytes from |in| to the AAD and sets |*out_len|
// to |in_len|. The AAD must be fully specified in this way before this function
// is used to decrypt ciphertext.
OPENSSL_EXPORT int EVP_DecryptUpdate(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                     int *out_len, const uint8_t *in,
                                     int in_len);

// EVP_DecryptFinal_ex writes at most a block of ciphertext to |out| and sets
// |*out_len| to the number of bytes written. If padding is enabled (the
// default) then padding is removed from the final block.
//
// WARNING: it is unsafe to call this function with unauthenticated
// ciphertext if padding is enabled.
OPENSSL_EXPORT int EVP_DecryptFinal_ex(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                       int *out_len);

// EVP_CipherUpdate calls either |EVP_EncryptUpdate| or |EVP_DecryptUpdate|
// depending on how |ctx| has been setup.
OPENSSL_EXPORT int EVP_CipherUpdate(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                    int *out_len, const uint8_t *in,
                                    int in_len);

// EVP_CipherFinal_ex calls either |EVP_EncryptFinal_ex| or
// |EVP_DecryptFinal_ex| depending on how |ctx| has been setup.
OPENSSL_EXPORT int EVP_CipherFinal_ex(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                      int *out_len);


// Cipher context accessors.

// EVP_CIPHER_CTX_cipher returns the |EVP_CIPHER| underlying |ctx|, or NULL if
// none has been set.
OPENSSL_EXPORT const EVP_CIPHER *EVP_CIPHER_CTX_cipher(
    const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_nid returns a NID identifying the |EVP_CIPHER| underlying
// |ctx| (e.g. |NID_aes_128_gcm|). It will crash if no cipher has been
// configured.
OPENSSL_EXPORT int EVP_CIPHER_CTX_nid(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_encrypting returns one if |ctx| is configured for encryption
// and zero otherwise.
OPENSSL_EXPORT int EVP_CIPHER_CTX_encrypting(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_block_size returns the block size, in bytes, of the cipher
// underlying |ctx|, or one if the cipher is a stream cipher. It will crash if
// no cipher has been configured.
OPENSSL_EXPORT unsigned EVP_CIPHER_CTX_block_size(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_key_length returns the key size, in bytes, of the cipher
// underlying |ctx| or zero if no cipher has been configured.
OPENSSL_EXPORT unsigned EVP_CIPHER_CTX_key_length(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_iv_length returns the IV size, in bytes, of the cipher
// underlying |ctx|. It will crash if no cipher has been configured.
OPENSSL_EXPORT unsigned EVP_CIPHER_CTX_iv_length(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_get_app_data returns the opaque, application data pointer for
// |ctx|, or NULL if none has been set.
OPENSSL_EXPORT void *EVP_CIPHER_CTX_get_app_data(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_set_app_data sets the opaque, application data pointer for
// |ctx| to |data|.
OPENSSL_EXPORT void EVP_CIPHER_CTX_set_app_data(EVP_CIPHER_CTX *ctx,
                                                void *data);

// EVP_CIPHER_CTX_flags returns a value which is the OR of zero or more
// |EVP_CIPH_*| flags. It will crash if no cipher has been configured.
OPENSSL_EXPORT uint32_t EVP_CIPHER_CTX_flags(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_mode returns one of the |EVP_CIPH_*| cipher mode values
// enumerated below. It will crash if no cipher has been configured.
OPENSSL_EXPORT uint32_t EVP_CIPHER_CTX_mode(const EVP_CIPHER_CTX *ctx);

// EVP_CIPHER_CTX_ctrl is an |ioctl| like function. The |command| argument
// should be one of the |EVP_CTRL_*| values. The |arg| and |ptr| arguments are
// specific to the command in question.
OPENSSL_EXPORT int EVP_CIPHER_CTX_ctrl(EVP_CIPHER_CTX *ctx, int command,
                                       int arg, void *ptr);

// EVP_CIPHER_CTX_set_padding sets whether padding is enabled for |ctx| and
// returns one. Pass a non-zero |pad| to enable padding (the default) or zero
// to disable.
OPENSSL_EXPORT int EVP_CIPHER_CTX_set_padding(EVP_CIPHER_CTX *ctx, int pad);

// EVP_CIPHER_CTX_set_key_length sets the key length for |ctx|. This is only
// valid for ciphers that can take a variable length key. It returns one on
// success and zero on error.
OPENSSL_EXPORT int EVP_CIPHER_CTX_set_key_length(EVP_CIPHER_CTX *ctx,
                                                 unsigned key_len);


// Cipher accessors.

// EVP_CIPHER_nid returns a NID identifying |cipher|. (For example,
// |NID_aes_128_gcm|.)
OPENSSL_EXPORT int EVP_CIPHER_nid(const EVP_CIPHER *cipher);

// EVP_CIPHER_name returns the short name of |cipher|.
OPENSSL_EXPORT const char *EVP_CIPHER_name(const EVP_CIPHER *cipher);

// EVP_CIPHER_block_size returns the block size, in bytes, for |cipher|, or one
// if |cipher| is a stream cipher.
OPENSSL_EXPORT unsigned EVP_CIPHER_block_size(const EVP_CIPHER *cipher);

// EVP_CIPHER_key_length returns the key size, in bytes, for |cipher|. If
// |cipher| can take a variable key length then this function returns the
// default key length and |EVP_CIPHER_flags| will return a value with
// |EVP_CIPH_VARIABLE_LENGTH| set.
OPENSSL_EXPORT unsigned EVP_CIPHER_key_length(const EVP_CIPHER *cipher);

// EVP_CIPHER_iv_length returns the IV size, in bytes, of |cipher|, or zero if
// |cipher| doesn't take an IV.
OPENSSL_EXPORT unsigned EVP_CIPHER_iv_length(const EVP_CIPHER *cipher);

// EVP_CIPHER_flags returns a value which is the OR of zero or more
// |EVP_CIPH_*| flags.
OPENSSL_EXPORT uint32_t EVP_CIPHER_flags(const EVP_CIPHER *cipher);

// EVP_CIPHER_mode returns one of the cipher mode values enumerated below.
OPENSSL_EXPORT uint32_t EVP_CIPHER_mode(const EVP_CIPHER *cipher);


// Key derivation.

// EVP_BytesToKey generates a key and IV for the cipher |type| by iterating
// |md| |count| times using |data| and |salt|. On entry, the |key| and |iv|
// buffers must have enough space to hold a key and IV for |type|. It returns
// the length of the key on success or zero on error.
OPENSSL_EXPORT int EVP_BytesToKey(const EVP_CIPHER *type, const EVP_MD *md,
                                  const uint8_t *salt, const uint8_t *data,
                                  size_t data_len, unsigned count, uint8_t *key,
                                  uint8_t *iv);


// Cipher modes (for |EVP_CIPHER_mode|).

#define EVP_CIPH_STREAM_CIPHER 0x0
#define EVP_CIPH_ECB_MODE 0x1
#define EVP_CIPH_CBC_MODE 0x2
#define EVP_CIPH_CFB_MODE 0x3
#define EVP_CIPH_OFB_MODE 0x4
#define EVP_CIPH_CTR_MODE 0x5
#define EVP_CIPH_GCM_MODE 0x6
#define EVP_CIPH_XTS_MODE 0x7
#define EVP_CIPH_CCM_MODE 0x8

// EVP_CIPH_FLAG_LENGTH_BITS buffers length in bits not bytes: CFB1 mode only.
#define EVP_CIPH_FLAG_LENGTH_BITS 0x2000
// The following values are never returned from |EVP_CIPHER_mode| and are
// included only to make it easier to compile code with BoringSSL.
#define EVP_CIPH_OCB_MODE 0x9
#define EVP_CIPH_WRAP_MODE 0xa


// Cipher flags (for |EVP_CIPHER_flags|).

// EVP_CIPH_VARIABLE_LENGTH indicates that the cipher takes a variable length
// key.
#define EVP_CIPH_VARIABLE_LENGTH 0x40

// EVP_CIPH_ALWAYS_CALL_INIT indicates that the |init| function for the cipher
// should always be called when initialising a new operation, even if the key
// is NULL to indicate that the same key is being used.
#define EVP_CIPH_ALWAYS_CALL_INIT 0x80

// EVP_CIPH_CUSTOM_IV indicates that the cipher manages the IV itself rather
// than keeping it in the |iv| member of |EVP_CIPHER_CTX|.
#define EVP_CIPH_CUSTOM_IV 0x100

// EVP_CIPH_CTRL_INIT indicates that EVP_CTRL_INIT should be used when
// initialising an |EVP_CIPHER_CTX|.
#define EVP_CIPH_CTRL_INIT 0x200

// EVP_CIPH_FLAG_CUSTOM_CIPHER indicates that the cipher manages blocking
// itself. This causes EVP_(En|De)crypt_ex to be simple wrapper functions.
#define EVP_CIPH_FLAG_CUSTOM_CIPHER 0x400

// EVP_CIPH_FLAG_AEAD_CIPHER specifies that the cipher is an AEAD. This is an
// older version of the proper AEAD interface. See aead.h for the current
// one.
#define EVP_CIPH_FLAG_AEAD_CIPHER 0x800

// EVP_CIPH_CUSTOM_COPY indicates that the |ctrl| callback should be called
// with |EVP_CTRL_COPY| at the end of normal |EVP_CIPHER_CTX_copy|
// processing.
#define EVP_CIPH_CUSTOM_COPY 0x1000

// EVP_CIPH_FLAG_NON_FIPS_ALLOW is meaningless. In OpenSSL it permits non-FIPS
// algorithms in FIPS mode. But BoringSSL FIPS mode doesn't prohibit algorithms
// (it's up the the caller to use the FIPS module in a fashion compliant with
// their needs). Thus this exists only to allow code to compile.
#define EVP_CIPH_FLAG_NON_FIPS_ALLOW 0


// Deprecated functions

// EVP_aes_128/256_cbc_hmac_sha1/256 return |EVP_CIPHER| objects that implement
// the named cipher algorithm. They are imported from OpenSSL to provide AES CBC
// HMAC SHA stitch implementation. These methods are TLS specific.
//
// WARNING: these APIs usage can get wrong easily. Below functions include
// details.
//     |aesni_cbc_hmac_sha1_cipher| and |aesni_cbc_hmac_sha256_cipher|.


OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cbc_hmac_sha1(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cbc_hmac_sha1(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cbc_hmac_sha256(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cbc_hmac_sha256(void);

// EVP_CipherInit acts like EVP_CipherInit_ex except that |EVP_CIPHER_CTX_init|
// is called on |cipher| first, if |cipher| is not NULL.
OPENSSL_EXPORT int EVP_CipherInit(EVP_CIPHER_CTX *ctx, const EVP_CIPHER *cipher,
                                  const uint8_t *key, const uint8_t *iv,
                                  int enc);

// EVP_EncryptInit calls |EVP_CipherInit| with |enc| equal to one.
OPENSSL_EXPORT int EVP_EncryptInit(EVP_CIPHER_CTX *ctx,
                                   const EVP_CIPHER *cipher, const uint8_t *key,
                                   const uint8_t *iv);

// EVP_DecryptInit calls |EVP_CipherInit| with |enc| equal to zero.
OPENSSL_EXPORT int EVP_DecryptInit(EVP_CIPHER_CTX *ctx,
                                   const EVP_CIPHER *cipher, const uint8_t *key,
                                   const uint8_t *iv);

// EVP_CipherFinal calls |EVP_CipherFinal_ex|.
OPENSSL_EXPORT int EVP_CipherFinal(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                   int *out_len);

// EVP_EncryptFinal calls |EVP_EncryptFinal_ex|.
OPENSSL_EXPORT int EVP_EncryptFinal(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                    int *out_len);

// EVP_DecryptFinal calls |EVP_DecryptFinal_ex|.
OPENSSL_EXPORT int EVP_DecryptFinal(EVP_CIPHER_CTX *ctx, uint8_t *out,
                                    int *out_len);

// EVP_Cipher historically exposed an internal implementation detail of |ctx|
// and should not be used. Use |EVP_CipherUpdate| and |EVP_CipherFinal_ex|
// instead.
//
// If |ctx|'s cipher does not have the |EVP_CIPH_FLAG_CUSTOM_CIPHER| flag, it
// encrypts or decrypts |in_len| bytes from |in| and writes the resulting
// |in_len| bytes to |out|. It returns one on success and zero on error.
// |in_len| must be a multiple of the cipher's block size, or the behavior is
// undefined.
//
// TODO(davidben): Rather than being undefined (it'll often round the length up
// and likely read past the buffer), just fail the operation.
//
// If |ctx|'s cipher has the |EVP_CIPH_FLAG_CUSTOM_CIPHER| flag, it runs in one
// of two modes: If |in| is non-NULL, it behaves like |EVP_CipherUpdate|. If
// |in| is NULL, it behaves like |EVP_CipherFinal_ex|. In both cases, it returns
// |*out_len| on success and -1 on error.
//
// WARNING: The two possible calling conventions of this function signal errors
// incompatibly. In the first, zero indicates an error. In the second, zero
// indicates success with zero bytes of output.
OPENSSL_EXPORT int EVP_Cipher(EVP_CIPHER_CTX *ctx, uint8_t *out,
                              const uint8_t *in, size_t in_len);

// EVP_get_cipherbyname returns an |EVP_CIPHER| given a human readable name in
// |name|, or NULL if the name is unknown. Note using this function links almost
// every cipher implemented by BoringSSL into the binary, not just the ones the
// caller requests. Size-conscious callers, such as client software, should not
// use this function.
OPENSSL_EXPORT const EVP_CIPHER *EVP_get_cipherbyname(const char *name);

// These AEADs are deprecated AES-GCM implementations that set
// |EVP_CIPH_FLAG_CUSTOM_CIPHER|. Use |EVP_aead_aes_128_gcm| and
// |EVP_aead_aes_256_gcm| instead.
//
// WARNING: Although these APIs allow streaming an individual AES-GCM operation,
// this is not secure. Until calling |EVP_DecryptFinal_ex|, the tag has not yet
// been checked and output released by |EVP_DecryptUpdate| is unauthenticated
// and easily manipulated by attackers. Callers must buffer the output and may
// not act on it until the entire operation is complete.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_gcm(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_gcm(void);

OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_ccm(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_ccm(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_ccm(void);

// These are deprecated, 192-bit version of AES.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_ecb(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_cbc(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_ctr(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_gcm(void);
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_ofb(void);

// EVP_des_ede3_ecb is an alias for |EVP_des_ede3|. Use the former instead.
OPENSSL_EXPORT const EVP_CIPHER *EVP_des_ede3_ecb(void);

// EVP_aes_128_cfb128 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cfb128(void);

// EVP_aes_128_cfb is an alias for |EVP_aes_128_cfb128| and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cfb(void);

// EVP_aes_128_cfb1 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cfb1(void);

// EVP_aes_128_cfb8 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_128_cfb8(void);

// EVP_aes_192_cfb128 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_cfb128(void);

// EVP_aes_192_cfb is an alias for |EVP_aes_192_cfb128| and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_cfb(void);

// EVP_aes_192_cfb1 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_cfb1(void);

// EVP_aes_192_cfb8 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_192_cfb8(void);

// EVP_aes_256_cfb128 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cfb128(void);

// EVP_aes_256_cfb is an alias for |EVP_aes_256_cfb128| and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cfb(void);

// EVP_aes_256_cfb1 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cfb1(void);

// EVP_aes_256_cfb8 is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_aes_256_cfb8(void);

// EVP_bf_ecb is Blowfish in ECB mode and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_bf_ecb(void);

// EVP_bf_cbc is Blowfish in CBC mode and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_bf_cbc(void);

// EVP_bf_cfb is an alias for |EVP_bf_cfb64| and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_bf_cfb(void);

// EVP_bf_cfb64 is Blowfish in 64-bit CFB mode and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_bf_cfb64(void);

// EVP_bf_ofb is Blowfish in 64-bit OFB mode and is deprecated.
OPENSSL_EXPORT const EVP_CIPHER *EVP_bf_ofb(void);

// EVP_cast5_ecb is CAST5 in ECB mode and is deprecated.
OPENSSL_EXPORT OPENSSL_DEPRECATED const EVP_CIPHER *EVP_cast5_ecb(void);

// EVP_cast5_cbc is CAST5 in CBC mode and is deprecated.
OPENSSL_EXPORT OPENSSL_DEPRECATED const EVP_CIPHER *EVP_cast5_cbc(void);


// General No-op Functions [Deprecated].

// EVP_CIPHER_CTX_set_flags does nothing. We strongly discourage doing
// any additional configurations when consuming |EVP_CIPHER_CTX|.
OPENSSL_EXPORT OPENSSL_DEPRECATED void EVP_CIPHER_CTX_set_flags(
    const EVP_CIPHER_CTX *ctx, uint32_t flags);

// The following flags are related to |EVP_CIPHER_CTX_set_flags|. They
// do nothing and are included only to make it easier to compile code
// with AWS-LC.
#define EVP_CIPHER_CTX_FLAG_WRAP_ALLOW 0

// EVP_add_cipher_alias does nothing and returns one.
OPENSSL_EXPORT OPENSSL_DEPRECATED int EVP_add_cipher_alias(const char *a,
                                                           const char *b);


// Private functions.

// EVP_CIPH_NO_PADDING disables padding in block ciphers.
#define EVP_CIPH_NO_PADDING 0x800

// The following are |EVP_CIPHER_CTX_ctrl| commands.
#define EVP_CTRL_INIT 0x0
#define EVP_CTRL_SET_KEY_LENGTH 0x1
#define EVP_CTRL_GET_RC2_KEY_BITS 0x2
#define EVP_CTRL_SET_RC2_KEY_BITS 0x3
#define EVP_CTRL_GET_RC5_ROUNDS 0x4
#define EVP_CTRL_SET_RC5_ROUNDS 0x5
#define EVP_CTRL_RAND_KEY 0x6
#define EVP_CTRL_PBE_PRF_NID 0x7
#define EVP_CTRL_COPY 0x8
#define EVP_CTRL_AEAD_SET_IVLEN 0x9
#define EVP_CTRL_AEAD_GET_TAG 0x10
#define EVP_CTRL_AEAD_SET_TAG 0x11
#define EVP_CTRL_AEAD_SET_IV_FIXED 0x12
#define EVP_CTRL_GCM_IV_GEN 0x13
#define EVP_CTRL_CCM_SET_L 0x14
#define EVP_CTRL_AEAD_SET_MAC_KEY 0x17
// EVP_CTRL_GCM_SET_IV_INV sets the GCM invocation field, decrypt only
#define EVP_CTRL_GCM_SET_IV_INV 0x18
#define EVP_CTRL_GET_IVLEN 0x19

// The following constants are unused.
#define EVP_GCM_TLS_FIXED_IV_LEN 4
#define EVP_GCM_TLS_EXPLICIT_IV_LEN 8
#define EVP_GCM_TLS_TAG_LEN 16

// The following are legacy aliases for AEAD |EVP_CIPHER_CTX_ctrl| values.
#define EVP_CTRL_GCM_SET_IVLEN EVP_CTRL_AEAD_SET_IVLEN
#define EVP_CTRL_GCM_GET_TAG EVP_CTRL_AEAD_GET_TAG
#define EVP_CTRL_GCM_SET_TAG EVP_CTRL_AEAD_SET_TAG
#define EVP_CTRL_GCM_SET_IV_FIXED EVP_CTRL_AEAD_SET_IV_FIXED

#define EVP_MAX_KEY_LENGTH 64
#define EVP_MAX_IV_LENGTH 16
#define EVP_MAX_BLOCK_LENGTH 32

struct evp_cipher_ctx_st {
  // cipher contains the underlying cipher for this context.
  const EVP_CIPHER *cipher;

  // app_data is a pointer to opaque, user data.
  void *app_data;  // application stuff

  // cipher_data points to the |cipher| specific state.
  void *cipher_data;

  // key_len contains the length of the key, which may differ from
  // |cipher->key_len| if the cipher can take a variable key length.
  unsigned key_len;

  // encrypt is one if encrypting and zero if decrypting.
  int encrypt;

  // flags contains the OR of zero or more |EVP_CIPH_*| flags, above.
  uint32_t flags;

  // oiv contains the original IV value.
  uint8_t oiv[EVP_MAX_IV_LENGTH];

  // iv contains the current IV value, which may have been updated.
  uint8_t iv[EVP_MAX_IV_LENGTH];

  // buf contains a partial block which is used by, for example, CTR mode to
  // store unused keystream bytes.
  uint8_t buf[EVP_MAX_BLOCK_LENGTH];

  // buf_len contains the number of bytes of a partial block contained in
  // |buf|.
  int buf_len;

  // num contains the number of bytes of |iv| which are valid for modes that
  // manage partial blocks themselves.
  unsigned num;

  // final_used is non-zero if the |final| buffer contains plaintext.
  int final_used;

  uint8_t final[EVP_MAX_BLOCK_LENGTH];  // possible final block

  // Has this structure been rendered unusable by a failure.
  int poisoned;
} /* EVP_CIPHER_CTX */;

typedef struct evp_cipher_info_st {
  const EVP_CIPHER *cipher;
  unsigned char iv[EVP_MAX_IV_LENGTH];
} EVP_CIPHER_INFO;


// AES-CBC stitch ctrl method constants

// EVP_CTRL_AEAD_TLS1_AAD is a control command for |EVP_CIPHER_CTX_ctrl| to set
// the length of the TLS additional authenticated data
#define EVP_CTRL_AEAD_TLS1_AAD 0x16
// EVP_AEAD_TLS1_AAD_LEN is the length of the additional authenticated data from
// RFC 5246.
#define EVP_AEAD_TLS1_AAD_LEN 13


#if defined(__cplusplus)
}  // extern C

#if !defined(BORINGSSL_NO_CXX)
extern "C++" {

BSSL_NAMESPACE_BEGIN

BORINGSSL_MAKE_DELETER(EVP_CIPHER_CTX, EVP_CIPHER_CTX_free)

using ScopedEVP_CIPHER_CTX =
    internal::StackAllocated<EVP_CIPHER_CTX, int, EVP_CIPHER_CTX_init,
                             EVP_CIPHER_CTX_cleanup>;

BSSL_NAMESPACE_END

}  // extern C++
#endif

#endif


#define CIPHER_R_AES_KEY_SETUP_FAILED 100
#define CIPHER_R_BAD_DECRYPT 101
#define CIPHER_R_BAD_KEY_LENGTH 102
#define CIPHER_R_BUFFER_TOO_SMALL 103
#define CIPHER_R_CTRL_NOT_IMPLEMENTED 104
#define CIPHER_R_CTRL_OPERATION_NOT_IMPLEMENTED 105
#define CIPHER_R_DATA_NOT_MULTIPLE_OF_BLOCK_LENGTH 106
#define CIPHER_R_INITIALIZATION_ERROR 107
#define CIPHER_R_INPUT_NOT_INITIALIZED 108
#define CIPHER_R_INVALID_AD_SIZE 109
#define CIPHER_R_INVALID_KEY_LENGTH 110
#define CIPHER_R_INVALID_NONCE_SIZE 111
#define CIPHER_R_INVALID_OPERATION 112
#define CIPHER_R_IV_TOO_LARGE 113
#define CIPHER_R_NO_CIPHER_SET 114
#define CIPHER_R_OUTPUT_ALIASES_INPUT 115
#define CIPHER_R_TAG_TOO_LARGE 116
#define CIPHER_R_TOO_LARGE 117
#define CIPHER_R_UNSUPPORTED_AD_SIZE 118
#define CIPHER_R_UNSUPPORTED_INPUT_SIZE 119
#define CIPHER_R_UNSUPPORTED_KEY_SIZE 120
#define CIPHER_R_UNSUPPORTED_NONCE_SIZE 121
#define CIPHER_R_UNSUPPORTED_TAG_SIZE 122
#define CIPHER_R_WRONG_FINAL_BLOCK_LENGTH 123
#define CIPHER_R_NO_DIRECTION_SET 124
#define CIPHER_R_INVALID_NONCE 125
#define CIPHER_R_XTS_DUPLICATED_KEYS 138
#define CIPHER_R_XTS_DATA_UNIT_IS_TOO_LARGE 139
#define CIPHER_R_CTRL_OPERATION_NOT_PERFORMED 140
#define CIPHER_R_SERIALIZATION_INVALID_EVP_AEAD_CTX 141
#define CIPHER_R_ALIGNMENT_CHANGED 142
#define CIPHER_R_SERIALIZATION_INVALID_SERDE_VERSION 143
#define CIPHER_R_SERIALIZATION_INVALID_CIPHER_ID 144

#endif  // OPENSSL_HEADER_CIPHER_H
