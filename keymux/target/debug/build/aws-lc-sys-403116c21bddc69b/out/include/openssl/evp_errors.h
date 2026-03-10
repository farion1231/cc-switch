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

#ifndef OPENSSL_HEADER_EVP_ERRORS_H
#define OPENSSL_HEADER_EVP_ERRORS_H

#define EVP_R_BUFFER_TOO_SMALL 100
#define EVP_R_COMMAND_NOT_SUPPORTED 101
#define EVP_R_DECODE_ERROR 102
#define EVP_R_DIFFERENT_KEY_TYPES 103
#define EVP_R_DIFFERENT_PARAMETERS 104
#define EVP_R_ENCODE_ERROR 105
#define EVP_R_EXPECTING_AN_EC_KEY_KEY 106
#define EVP_R_EXPECTING_AN_RSA_KEY 107
#define EVP_R_EXPECTING_A_DSA_KEY 108
#define EVP_R_ILLEGAL_OR_UNSUPPORTED_PADDING_MODE 109
#define EVP_R_INVALID_DIGEST_LENGTH 110
#define EVP_R_INVALID_DIGEST_TYPE 111
#define EVP_R_INVALID_KEYBITS 112
#define EVP_R_INVALID_MGF1_MD 113
#define EVP_R_INVALID_OPERATION 114
#define EVP_R_INVALID_PADDING_MODE 115
#define EVP_R_INVALID_PSS_SALTLEN 116
#define EVP_R_KEYS_NOT_SET 117
#define EVP_R_MISSING_PARAMETERS 118
#define EVP_R_NO_DEFAULT_DIGEST 119
#define EVP_R_NO_KEY_SET 120
#define EVP_R_NO_MDC2_SUPPORT 121
#define EVP_R_NO_NID_FOR_CURVE 122
#define EVP_R_NO_OPERATION_SET 123
#define EVP_R_NO_PARAMETERS_SET 124
#define EVP_R_OPERATION_NOT_SUPPORTED_FOR_THIS_KEYTYPE 125
#define EVP_R_OPERATON_NOT_INITIALIZED 126
#define EVP_R_UNKNOWN_PUBLIC_KEY_TYPE 127
#define EVP_R_UNSUPPORTED_ALGORITHM 128
#define EVP_R_UNSUPPORTED_PUBLIC_KEY_TYPE 129
#define EVP_R_NOT_A_PRIVATE_KEY 130
#define EVP_R_INVALID_SIGNATURE 131
#define EVP_R_MEMORY_LIMIT_EXCEEDED 132
#define EVP_R_INVALID_PARAMETERS 133
#define EVP_R_INVALID_PEER_KEY 134
#define EVP_R_NOT_XOF_OR_INVALID_LENGTH 135
#define EVP_R_EMPTY_PSK 136
#define EVP_R_INVALID_BUFFER_SIZE 137
#define EVP_R_BAD_DECRYPT 138
#define EVP_R_EXPECTING_A_DH_KEY 139
#define EVP_R_INVALID_PSS_MD 500
#define EVP_R_INVALID_PSS_SALT_LEN 501
#define EVP_R_INVALID_PSS_TRAILER_FIELD 502

#endif  // OPENSSL_HEADER_EVP_ERRORS_H
