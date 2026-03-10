/*
 * Copyright 2018-2022 The OpenSSL Project Authors. All Rights Reserved.
 *
 * Licensed under the Apache License 2.0 (the "License").  You may not use
 * this file except in compliance with the License.  You can obtain a copy
 * in the file LICENSE in the source distribution or at
 * https://www.openssl.org/source/license.html
 */

#ifndef OPENSSL_HEADER_SSHKDF_H
#define OPENSSL_HEADER_SSHKDF_H

#include <openssl/base.h>

#if defined(__cplusplus)
extern "C" {
#endif


// SSH-specific KDF
//
// This KDF should only be called from SSH client/server code; it's not a
// general-purpose KDF and is only Approved for FIPS 140-3 use specifically
// in SSH.


// The following defines are the valid |type| values for SSHKDF().

#define EVP_KDF_SSHKDF_TYPE_INITIAL_IV_CLI_TO_SRV     65
#define EVP_KDF_SSHKDF_TYPE_INITIAL_IV_SRV_TO_CLI     66
#define EVP_KDF_SSHKDF_TYPE_ENCRYPTION_KEY_CLI_TO_SRV 67
#define EVP_KDF_SSHKDF_TYPE_ENCRYPTION_KEY_SRV_TO_CLI 68
#define EVP_KDF_SSHKDF_TYPE_INTEGRITY_KEY_CLI_TO_SRV  69
#define EVP_KDF_SSHKDF_TYPE_INTEGRITY_KEY_SRV_TO_CLI  70

// SSHKDF is a key derivation function used in the SSH Transport Layer Protocol
// defined in Section 7.2 of RFC 4253. It calculates a derived key |out| of
// length |out_len| bytes using |evp_md| hash algorithm from the supplied
// shared secret |key|, hash value |xcghash| and session identifier
// |session_id|. It returns one on success and zero on error.
//
// |xcghash| is produced during the SSH Diffie-Hellman exchange.
//
// SSHKDF is only FIPS 140-3 Approved for use in SSH.
OPENSSL_EXPORT int SSHKDF(const EVP_MD *evp_md,
                          const uint8_t *key, size_t key_len,
                          const uint8_t *xcghash, size_t xcghash_len,
                          const uint8_t *session_id, size_t session_id_len,
                          char type,
                          uint8_t *out, size_t out_len);


#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_SSHKDF_H
