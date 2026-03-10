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

/* This header is provided in order to make compiling against code that expects
   OpenSSL easier. */

#ifndef OPENSSL_HEADER_OPENSSLCONF_H
#define OPENSSL_HEADER_OPENSSLCONF_H


#if defined(__cplusplus)
extern "C" {
#endif


#define OPENSSL_NO_ASYNC
#define OPENSSL_NO_BLAKE2
#define OPENSSL_NO_BUF_FREELISTS
#define OPENSSL_NO_CAMELLIA
#define OPENSSL_NO_CAPIENG
#define OPENSSL_NO_CAST
#define OPENSSL_NO_CMS
#define OPENSSL_NO_COMP
#define OPENSSL_NO_CRYPTO_MDEBUG
#define OPENSSL_NO_CT
#define OPENSSL_NO_DANE
#define OPENSSL_NO_DEPRECATED
#define OPENSSL_NO_DGRAM
#define OPENSSL_NO_DYNAMIC_ENGINE
#define OPENSSL_NO_EC_NISTP_64_GCC_128
#define OPENSSL_NO_EC2M
#define OPENSSL_NO_EGD
#define OPENSSL_NO_ENGINE
#define OPENSSL_NO_GMP
#define OPENSSL_NO_GOST
#define OPENSSL_NO_HEARTBEATS
#define OPENSSL_NO_HW
#define OPENSSL_NO_IDEA
#define OPENSSL_NO_JPAKE
#define OPENSSL_NO_KRB5
#define OPENSSL_NO_MD2
#define OPENSSL_NO_MDC2
#define OPENSSL_NO_OCB
// OPENSSL_NO_EXTERNAL_PSK_TLS13 indicates lack of support for external
// PSK authentication in TLS >= 1.3. AWS-LC intentionally omits support
// for this due to security conerns outlined in RFC 9258.
#define OPENSSL_NO_EXTERNAL_PSK_TLS13
#define OPENSSL_NO_RC2
#define OPENSSL_NO_RC5
#define OPENSSL_NO_RFC3779
#define OPENSSL_NO_RIPEMD
#define OPENSSL_NO_RMD160
#define OPENSSL_NO_SCTP
#define OPENSSL_NO_SEED
#define OPENSSL_NO_SM2
#define OPENSSL_NO_SM3
#define OPENSSL_NO_SM4
#define OPENSSL_NO_SRP
#define OPENSSL_NO_SSL_TRACE
#define OPENSSL_NO_SSL2
#define OPENSSL_NO_SSL3
#define OPENSSL_NO_SSL3_METHOD
#define OPENSSL_NO_STATIC_ENGINE
#define OPENSSL_NO_STORE
#define OPENSSL_NO_TS
#define OPENSSL_NO_UI_CONSOLE
#define OPENSSL_NO_WHIRLPOOL


#if defined(__cplusplus)
}
#endif


#endif  // OPENSSL_HEADER_OPENSSLCONF_H
