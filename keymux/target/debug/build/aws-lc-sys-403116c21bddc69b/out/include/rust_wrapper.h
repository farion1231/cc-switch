// Copyright (c) 2022, Google Inc.
// SPDX-License-Identifier: ISC
// Modifications copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR ISC

#ifndef OPENSSL_HEADER_RUST_WRAPPER_H
#define OPENSSL_HEADER_RUST_WRAPPER_H

#include "openssl/is_awslc.h"
#include "openssl/aes.h"
#include "openssl/asn1.h"
#include "openssl/asn1_mac.h"
#include "openssl/asn1t.h"
#include "openssl/base.h"
#include "openssl/base64.h"
#include "openssl/bio.h"
#include "openssl/blake2.h"
#include "openssl/blowfish.h"
#include "openssl/bn.h"
#include "openssl/buf.h"
#include "openssl/buffer.h"
#include "openssl/bytestring.h"
#include "openssl/chacha.h"
#include "openssl/cipher.h"
#include "openssl/cmac.h"
#include "openssl/conf.h"
#include "openssl/cpu.h"
#include "openssl/crypto.h"
#include "openssl/ctrdrbg.h"
#include "openssl/curve25519.h"
#include "openssl/des.h"
#include "openssl/dh.h"
#include "openssl/digest.h"
#include "openssl/dsa.h"
#include "openssl/e_os2.h"
#include "openssl/ec.h"
#include "openssl/ec_key.h"
#include "openssl/ecdh.h"
#include "openssl/ecdsa.h"
#include "openssl/engine.h"
#include "openssl/err.h"
#include "openssl/evp.h"
#include "openssl/evp_errors.h"
#include "openssl/ex_data.h"
#include "openssl/hkdf.h"
#include "openssl/hmac.h"
#include "openssl/hpke.h"
#include "openssl/hrss.h"
#include "openssl/kdf.h"
#include "openssl/lhash.h"
#include "openssl/md4.h"
#include "openssl/md5.h"
#include "openssl/mem.h"
#include "openssl/ocsp.h"
#include "openssl/obj.h"
#include "openssl/obj_mac.h"
#include "openssl/objects.h"
#include "openssl/opensslconf.h"
#include "openssl/opensslv.h"
#include "openssl/ossl_typ.h"
#include "openssl/pem.h"
#include "openssl/pkcs12.h"
#include "openssl/pkcs7.h"
#include "openssl/pkcs8.h"
#include "openssl/poly1305.h"
#include "openssl/pool.h"
#include "openssl/rand.h"
#include "openssl/rc4.h"
#include "openssl/ripemd.h"
#include "openssl/rsa.h"
#include "openssl/safestack.h"
#include "openssl/sha.h"
#include "openssl/siphash.h"
#include "openssl/span.h"
#include "openssl/stack.h"
#include "openssl/thread.h"
#include "openssl/trust_token.h"
#include "openssl/type_check.h"
#include "openssl/x509.h"
#include "openssl/x509_vfy.h"
#include "openssl/x509v3.h"
#include "openssl/experimental/kem_deterministic_api.h"

#if defined(AWS_LC_RUST_INCLUDE_SSL)
#include "openssl/ssl.h"
#include "openssl/ssl3.h"
#endif

#ifdef _WIN32
#define AWS_LC_SYS_EXPORT __declspec(dllexport)
#else
#define AWS_LC_SYS_EXPORT __attribute__((visibility("default")))
#endif

#define AWS_LC_WRAPPER_ADD_PREFIX(a, b) AWS_LC_WRAPPER_ADD_PREFIX_INNER(a, b)
#define AWS_LC_WRAPPER_ADD_PREFIX_INNER(a, b) a ## _ ## b

#if defined(BORINGSSL_PREFIX)
#define ERR_GET_LIB_RUST AWS_LC_WRAPPER_ADD_PREFIX(BORINGSSL_PREFIX, ERR_GET_LIB_RUST)
#define ERR_GET_REASON_RUST AWS_LC_WRAPPER_ADD_PREFIX(BORINGSSL_PREFIX, ERR_GET_REASON_RUST)
#define ERR_GET_FUNC_RUST AWS_LC_WRAPPER_ADD_PREFIX(BORINGSSL_PREFIX, ERR_GET_FUNC_RUST)
#endif  // BORINGSSL_PREFIX

#if defined(__cplusplus)
extern "C" {
#endif

// The following functions are wrappers over inline functions and macros in
// BoringSSL, which bindgen cannot currently correctly bind. These wrappers
// ensure changes to the functions remain in lockstep with the Rust versions.
AWS_LC_SYS_EXPORT int ERR_GET_LIB_RUST(uint32_t packed_error);
AWS_LC_SYS_EXPORT int ERR_GET_REASON_RUST(uint32_t packed_error);
AWS_LC_SYS_EXPORT int ERR_GET_FUNC_RUST(uint32_t packed_error);

#if defined(__cplusplus)
}  // extern C
#endif

#endif  // OPENSSL_HEADER_RUST_WRAPPER_H
