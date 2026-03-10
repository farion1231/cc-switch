; Copyright (c) 2018, Google Inc.
;
; Permission to use, copy, modify, and/or distribute this software for any
; purpose with or without fee is hereby granted, provided that the above
; copyright notice and this permission notice appear in all copies.
;
; THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
; WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
; MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
; SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
; WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
; OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
; CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

%ifndef BORINGSSL_PREFIX_SYMBOLS_NASM_INC

%define BORINGSSL_PREFIX_SYMBOLS_NASM_INC

%ifndef BORINGSSL_PREFIX
%define BORINGSSL_PREFIX aws_lc_0_37_1
%endif ; BORINGSSL_PREFIX

; 32-bit Windows adds underscores to C functions, while 64-bit Windows does not.
%ifidn __OUTPUT_FORMAT__, win32

%xdefine _ACCESS_DESCRIPTION_free _ %+ BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_free
%xdefine _ACCESS_DESCRIPTION_it _ %+ BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_it
%xdefine _ACCESS_DESCRIPTION_new _ %+ BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_new
%xdefine _AES_CMAC _ %+ BORINGSSL_PREFIX %+ _AES_CMAC
%xdefine _AES_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_cbc_encrypt
%xdefine _AES_cfb128_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_cfb128_encrypt
%xdefine _AES_cfb1_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_cfb1_encrypt
%xdefine _AES_cfb8_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_cfb8_encrypt
%xdefine _AES_ctr128_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_ctr128_encrypt
%xdefine _AES_decrypt _ %+ BORINGSSL_PREFIX %+ _AES_decrypt
%xdefine _AES_ecb_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_ecb_encrypt
%xdefine _AES_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_encrypt
%xdefine _AES_ofb128_encrypt _ %+ BORINGSSL_PREFIX %+ _AES_ofb128_encrypt
%xdefine _AES_set_decrypt_key _ %+ BORINGSSL_PREFIX %+ _AES_set_decrypt_key
%xdefine _AES_set_encrypt_key _ %+ BORINGSSL_PREFIX %+ _AES_set_encrypt_key
%xdefine _AES_unwrap_key _ %+ BORINGSSL_PREFIX %+ _AES_unwrap_key
%xdefine _AES_unwrap_key_padded _ %+ BORINGSSL_PREFIX %+ _AES_unwrap_key_padded
%xdefine _AES_wrap_key _ %+ BORINGSSL_PREFIX %+ _AES_wrap_key
%xdefine _AES_wrap_key_padded _ %+ BORINGSSL_PREFIX %+ _AES_wrap_key_padded
%xdefine _ASN1_ANY_it _ %+ BORINGSSL_PREFIX %+ _ASN1_ANY_it
%xdefine _ASN1_BIT_STRING_check _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_check
%xdefine _ASN1_BIT_STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_free
%xdefine _ASN1_BIT_STRING_get_bit _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_get_bit
%xdefine _ASN1_BIT_STRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_it
%xdefine _ASN1_BIT_STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_new
%xdefine _ASN1_BIT_STRING_num_bytes _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_num_bytes
%xdefine _ASN1_BIT_STRING_set _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_set
%xdefine _ASN1_BIT_STRING_set_bit _ %+ BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_set_bit
%xdefine _ASN1_BMPSTRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_free
%xdefine _ASN1_BMPSTRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_it
%xdefine _ASN1_BMPSTRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_new
%xdefine _ASN1_BOOLEAN_it _ %+ BORINGSSL_PREFIX %+ _ASN1_BOOLEAN_it
%xdefine _ASN1_ENUMERATED_free _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_free
%xdefine _ASN1_ENUMERATED_get _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get
%xdefine _ASN1_ENUMERATED_get_int64 _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get_int64
%xdefine _ASN1_ENUMERATED_get_uint64 _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get_uint64
%xdefine _ASN1_ENUMERATED_it _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_it
%xdefine _ASN1_ENUMERATED_new _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_new
%xdefine _ASN1_ENUMERATED_set _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set
%xdefine _ASN1_ENUMERATED_set_int64 _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set_int64
%xdefine _ASN1_ENUMERATED_set_uint64 _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set_uint64
%xdefine _ASN1_ENUMERATED_to_BN _ %+ BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_to_BN
%xdefine _ASN1_FBOOLEAN_it _ %+ BORINGSSL_PREFIX %+ _ASN1_FBOOLEAN_it
%xdefine _ASN1_GENERALIZEDTIME_adj _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_adj
%xdefine _ASN1_GENERALIZEDTIME_check _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_check
%xdefine _ASN1_GENERALIZEDTIME_free _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_free
%xdefine _ASN1_GENERALIZEDTIME_it _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_it
%xdefine _ASN1_GENERALIZEDTIME_new _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_new
%xdefine _ASN1_GENERALIZEDTIME_print _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_print
%xdefine _ASN1_GENERALIZEDTIME_set _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_set
%xdefine _ASN1_GENERALIZEDTIME_set_string _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_set_string
%xdefine _ASN1_GENERALSTRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_free
%xdefine _ASN1_GENERALSTRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_it
%xdefine _ASN1_GENERALSTRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_new
%xdefine _ASN1_IA5STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_IA5STRING_free
%xdefine _ASN1_IA5STRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_IA5STRING_it
%xdefine _ASN1_IA5STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_IA5STRING_new
%xdefine _ASN1_INTEGER_cmp _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_cmp
%xdefine _ASN1_INTEGER_dup _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_dup
%xdefine _ASN1_INTEGER_free _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_free
%xdefine _ASN1_INTEGER_get _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_get
%xdefine _ASN1_INTEGER_get_int64 _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_get_int64
%xdefine _ASN1_INTEGER_get_uint64 _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_get_uint64
%xdefine _ASN1_INTEGER_it _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_it
%xdefine _ASN1_INTEGER_new _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_new
%xdefine _ASN1_INTEGER_set _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_set
%xdefine _ASN1_INTEGER_set_int64 _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_set_int64
%xdefine _ASN1_INTEGER_set_uint64 _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_set_uint64
%xdefine _ASN1_INTEGER_to_BN _ %+ BORINGSSL_PREFIX %+ _ASN1_INTEGER_to_BN
%xdefine _ASN1_NULL_free _ %+ BORINGSSL_PREFIX %+ _ASN1_NULL_free
%xdefine _ASN1_NULL_it _ %+ BORINGSSL_PREFIX %+ _ASN1_NULL_it
%xdefine _ASN1_NULL_new _ %+ BORINGSSL_PREFIX %+ _ASN1_NULL_new
%xdefine _ASN1_OBJECT_create _ %+ BORINGSSL_PREFIX %+ _ASN1_OBJECT_create
%xdefine _ASN1_OBJECT_free _ %+ BORINGSSL_PREFIX %+ _ASN1_OBJECT_free
%xdefine _ASN1_OBJECT_it _ %+ BORINGSSL_PREFIX %+ _ASN1_OBJECT_it
%xdefine _ASN1_OBJECT_new _ %+ BORINGSSL_PREFIX %+ _ASN1_OBJECT_new
%xdefine _ASN1_OCTET_STRING_cmp _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_cmp
%xdefine _ASN1_OCTET_STRING_dup _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_dup
%xdefine _ASN1_OCTET_STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_free
%xdefine _ASN1_OCTET_STRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_it
%xdefine _ASN1_OCTET_STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_new
%xdefine _ASN1_OCTET_STRING_set _ %+ BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_set
%xdefine _ASN1_PRINTABLESTRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_free
%xdefine _ASN1_PRINTABLESTRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_it
%xdefine _ASN1_PRINTABLESTRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_new
%xdefine _ASN1_PRINTABLE_free _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_free
%xdefine _ASN1_PRINTABLE_it _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_it
%xdefine _ASN1_PRINTABLE_new _ %+ BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_new
%xdefine _ASN1_SEQUENCE_ANY_it _ %+ BORINGSSL_PREFIX %+ _ASN1_SEQUENCE_ANY_it
%xdefine _ASN1_SEQUENCE_it _ %+ BORINGSSL_PREFIX %+ _ASN1_SEQUENCE_it
%xdefine _ASN1_SET_ANY_it _ %+ BORINGSSL_PREFIX %+ _ASN1_SET_ANY_it
%xdefine _ASN1_STRING_TABLE_add _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_TABLE_add
%xdefine _ASN1_STRING_TABLE_cleanup _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_TABLE_cleanup
%xdefine _ASN1_STRING_clear_free _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_clear_free
%xdefine _ASN1_STRING_cmp _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_cmp
%xdefine _ASN1_STRING_copy _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_copy
%xdefine _ASN1_STRING_data _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_data
%xdefine _ASN1_STRING_dup _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_dup
%xdefine _ASN1_STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_free
%xdefine _ASN1_STRING_get0_data _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_get0_data
%xdefine _ASN1_STRING_get_default_mask _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_get_default_mask
%xdefine _ASN1_STRING_length _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_length
%xdefine _ASN1_STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_new
%xdefine _ASN1_STRING_print _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_print
%xdefine _ASN1_STRING_print_ex _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_print_ex
%xdefine _ASN1_STRING_print_ex_fp _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_print_ex_fp
%xdefine _ASN1_STRING_set _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_set
%xdefine _ASN1_STRING_set0 _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_set0
%xdefine _ASN1_STRING_set_by_NID _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_set_by_NID
%xdefine _ASN1_STRING_set_default_mask _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_set_default_mask
%xdefine _ASN1_STRING_set_default_mask_asc _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_set_default_mask_asc
%xdefine _ASN1_STRING_to_UTF8 _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_to_UTF8
%xdefine _ASN1_STRING_type _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_type
%xdefine _ASN1_STRING_type_new _ %+ BORINGSSL_PREFIX %+ _ASN1_STRING_type_new
%xdefine _ASN1_T61STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_T61STRING_free
%xdefine _ASN1_T61STRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_T61STRING_it
%xdefine _ASN1_T61STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_T61STRING_new
%xdefine _ASN1_TBOOLEAN_it _ %+ BORINGSSL_PREFIX %+ _ASN1_TBOOLEAN_it
%xdefine _ASN1_TIME_adj _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_adj
%xdefine _ASN1_TIME_check _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_check
%xdefine _ASN1_TIME_diff _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_diff
%xdefine _ASN1_TIME_free _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_free
%xdefine _ASN1_TIME_it _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_it
%xdefine _ASN1_TIME_new _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_new
%xdefine _ASN1_TIME_print _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_print
%xdefine _ASN1_TIME_set _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_set
%xdefine _ASN1_TIME_set_posix _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_set_posix
%xdefine _ASN1_TIME_set_string _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_set_string
%xdefine _ASN1_TIME_set_string_X509 _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_set_string_X509
%xdefine _ASN1_TIME_to_generalizedtime _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_to_generalizedtime
%xdefine _ASN1_TIME_to_posix _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_to_posix
%xdefine _ASN1_TIME_to_time_t _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_to_time_t
%xdefine _ASN1_TIME_to_tm _ %+ BORINGSSL_PREFIX %+ _ASN1_TIME_to_tm
%xdefine _ASN1_TYPE_cmp _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_cmp
%xdefine _ASN1_TYPE_free _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_free
%xdefine _ASN1_TYPE_get _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_get
%xdefine _ASN1_TYPE_new _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_new
%xdefine _ASN1_TYPE_set _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_set
%xdefine _ASN1_TYPE_set1 _ %+ BORINGSSL_PREFIX %+ _ASN1_TYPE_set1
%xdefine _ASN1_UNIVERSALSTRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_free
%xdefine _ASN1_UNIVERSALSTRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_it
%xdefine _ASN1_UNIVERSALSTRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_new
%xdefine _ASN1_UTCTIME_adj _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_adj
%xdefine _ASN1_UTCTIME_check _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_check
%xdefine _ASN1_UTCTIME_cmp_time_t _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_cmp_time_t
%xdefine _ASN1_UTCTIME_free _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_free
%xdefine _ASN1_UTCTIME_it _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_it
%xdefine _ASN1_UTCTIME_new _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_new
%xdefine _ASN1_UTCTIME_print _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_print
%xdefine _ASN1_UTCTIME_set _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_set
%xdefine _ASN1_UTCTIME_set_string _ %+ BORINGSSL_PREFIX %+ _ASN1_UTCTIME_set_string
%xdefine _ASN1_UTF8STRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_free
%xdefine _ASN1_UTF8STRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_it
%xdefine _ASN1_UTF8STRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_new
%xdefine _ASN1_VISIBLESTRING_free _ %+ BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_free
%xdefine _ASN1_VISIBLESTRING_it _ %+ BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_it
%xdefine _ASN1_VISIBLESTRING_new _ %+ BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_new
%xdefine _ASN1_digest _ %+ BORINGSSL_PREFIX %+ _ASN1_digest
%xdefine _ASN1_dup _ %+ BORINGSSL_PREFIX %+ _ASN1_dup
%xdefine _ASN1_generate_v3 _ %+ BORINGSSL_PREFIX %+ _ASN1_generate_v3
%xdefine _ASN1_get_object _ %+ BORINGSSL_PREFIX %+ _ASN1_get_object
%xdefine _ASN1_i2d_bio _ %+ BORINGSSL_PREFIX %+ _ASN1_i2d_bio
%xdefine _ASN1_item_d2i _ %+ BORINGSSL_PREFIX %+ _ASN1_item_d2i
%xdefine _ASN1_item_d2i_bio _ %+ BORINGSSL_PREFIX %+ _ASN1_item_d2i_bio
%xdefine _ASN1_item_d2i_fp _ %+ BORINGSSL_PREFIX %+ _ASN1_item_d2i_fp
%xdefine _ASN1_item_digest _ %+ BORINGSSL_PREFIX %+ _ASN1_item_digest
%xdefine _ASN1_item_dup _ %+ BORINGSSL_PREFIX %+ _ASN1_item_dup
%xdefine _ASN1_item_ex_d2i _ %+ BORINGSSL_PREFIX %+ _ASN1_item_ex_d2i
%xdefine _ASN1_item_ex_free _ %+ BORINGSSL_PREFIX %+ _ASN1_item_ex_free
%xdefine _ASN1_item_ex_i2d _ %+ BORINGSSL_PREFIX %+ _ASN1_item_ex_i2d
%xdefine _ASN1_item_ex_new _ %+ BORINGSSL_PREFIX %+ _ASN1_item_ex_new
%xdefine _ASN1_item_free _ %+ BORINGSSL_PREFIX %+ _ASN1_item_free
%xdefine _ASN1_item_i2d _ %+ BORINGSSL_PREFIX %+ _ASN1_item_i2d
%xdefine _ASN1_item_i2d_bio _ %+ BORINGSSL_PREFIX %+ _ASN1_item_i2d_bio
%xdefine _ASN1_item_i2d_fp _ %+ BORINGSSL_PREFIX %+ _ASN1_item_i2d_fp
%xdefine _ASN1_item_new _ %+ BORINGSSL_PREFIX %+ _ASN1_item_new
%xdefine _ASN1_item_pack _ %+ BORINGSSL_PREFIX %+ _ASN1_item_pack
%xdefine _ASN1_item_sign _ %+ BORINGSSL_PREFIX %+ _ASN1_item_sign
%xdefine _ASN1_item_sign_ctx _ %+ BORINGSSL_PREFIX %+ _ASN1_item_sign_ctx
%xdefine _ASN1_item_unpack _ %+ BORINGSSL_PREFIX %+ _ASN1_item_unpack
%xdefine _ASN1_item_verify _ %+ BORINGSSL_PREFIX %+ _ASN1_item_verify
%xdefine _ASN1_mbstring_copy _ %+ BORINGSSL_PREFIX %+ _ASN1_mbstring_copy
%xdefine _ASN1_mbstring_ncopy _ %+ BORINGSSL_PREFIX %+ _ASN1_mbstring_ncopy
%xdefine _ASN1_object_size _ %+ BORINGSSL_PREFIX %+ _ASN1_object_size
%xdefine _ASN1_parse _ %+ BORINGSSL_PREFIX %+ _ASN1_parse
%xdefine _ASN1_primitive_free _ %+ BORINGSSL_PREFIX %+ _ASN1_primitive_free
%xdefine _ASN1_put_eoc _ %+ BORINGSSL_PREFIX %+ _ASN1_put_eoc
%xdefine _ASN1_put_object _ %+ BORINGSSL_PREFIX %+ _ASN1_put_object
%xdefine _ASN1_tag2bit _ %+ BORINGSSL_PREFIX %+ _ASN1_tag2bit
%xdefine _ASN1_tag2str _ %+ BORINGSSL_PREFIX %+ _ASN1_tag2str
%xdefine _ASN1_template_free _ %+ BORINGSSL_PREFIX %+ _ASN1_template_free
%xdefine _AUTHORITY_INFO_ACCESS_free _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_free
%xdefine _AUTHORITY_INFO_ACCESS_it _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_it
%xdefine _AUTHORITY_INFO_ACCESS_new _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_new
%xdefine _AUTHORITY_KEYID_free _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_free
%xdefine _AUTHORITY_KEYID_it _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_it
%xdefine _AUTHORITY_KEYID_new _ %+ BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_new
%xdefine _AWSLC_non_fips_pkey_evp_asn1_methods _ %+ BORINGSSL_PREFIX %+ _AWSLC_non_fips_pkey_evp_asn1_methods
%xdefine _AWSLC_non_fips_pkey_evp_methods _ %+ BORINGSSL_PREFIX %+ _AWSLC_non_fips_pkey_evp_methods
%xdefine _AWSLC_thread_local_clear _ %+ BORINGSSL_PREFIX %+ _AWSLC_thread_local_clear
%xdefine _AWSLC_thread_local_shutdown _ %+ BORINGSSL_PREFIX %+ _AWSLC_thread_local_shutdown
%xdefine _AWS_LC_FIPS_failure _ %+ BORINGSSL_PREFIX %+ _AWS_LC_FIPS_failure
%xdefine _BASIC_CONSTRAINTS_free _ %+ BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_free
%xdefine _BASIC_CONSTRAINTS_it _ %+ BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_it
%xdefine _BASIC_CONSTRAINTS_new _ %+ BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_new
%xdefine _BF_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _BF_cbc_encrypt
%xdefine _BF_cfb64_encrypt _ %+ BORINGSSL_PREFIX %+ _BF_cfb64_encrypt
%xdefine _BF_decrypt _ %+ BORINGSSL_PREFIX %+ _BF_decrypt
%xdefine _BF_ecb_encrypt _ %+ BORINGSSL_PREFIX %+ _BF_ecb_encrypt
%xdefine _BF_encrypt _ %+ BORINGSSL_PREFIX %+ _BF_encrypt
%xdefine _BF_ofb64_encrypt _ %+ BORINGSSL_PREFIX %+ _BF_ofb64_encrypt
%xdefine _BF_set_key _ %+ BORINGSSL_PREFIX %+ _BF_set_key
%xdefine _BIO_ADDR_clear _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_clear
%xdefine _BIO_ADDR_copy _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_copy
%xdefine _BIO_ADDR_dup _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_dup
%xdefine _BIO_ADDR_family _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_family
%xdefine _BIO_ADDR_free _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_free
%xdefine _BIO_ADDR_new _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_new
%xdefine _BIO_ADDR_rawaddress _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_rawaddress
%xdefine _BIO_ADDR_rawmake _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_rawmake
%xdefine _BIO_ADDR_rawport _ %+ BORINGSSL_PREFIX %+ _BIO_ADDR_rawport
%xdefine _BIO_append_filename _ %+ BORINGSSL_PREFIX %+ _BIO_append_filename
%xdefine _BIO_callback_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_callback_ctrl
%xdefine _BIO_clear_flags _ %+ BORINGSSL_PREFIX %+ _BIO_clear_flags
%xdefine _BIO_clear_retry_flags _ %+ BORINGSSL_PREFIX %+ _BIO_clear_retry_flags
%xdefine _BIO_copy_next_retry _ %+ BORINGSSL_PREFIX %+ _BIO_copy_next_retry
%xdefine _BIO_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl
%xdefine _BIO_ctrl_dgram_connect _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl_dgram_connect
%xdefine _BIO_ctrl_get_read_request _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl_get_read_request
%xdefine _BIO_ctrl_get_write_guarantee _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl_get_write_guarantee
%xdefine _BIO_ctrl_pending _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl_pending
%xdefine _BIO_ctrl_set_connected _ %+ BORINGSSL_PREFIX %+ _BIO_ctrl_set_connected
%xdefine _BIO_destroy_bio_pair _ %+ BORINGSSL_PREFIX %+ _BIO_destroy_bio_pair
%xdefine _BIO_dgram_get_peer _ %+ BORINGSSL_PREFIX %+ _BIO_dgram_get_peer
%xdefine _BIO_dgram_recv_timedout _ %+ BORINGSSL_PREFIX %+ _BIO_dgram_recv_timedout
%xdefine _BIO_dgram_send_timedout _ %+ BORINGSSL_PREFIX %+ _BIO_dgram_send_timedout
%xdefine _BIO_dgram_set_peer _ %+ BORINGSSL_PREFIX %+ _BIO_dgram_set_peer
%xdefine _BIO_do_connect _ %+ BORINGSSL_PREFIX %+ _BIO_do_connect
%xdefine _BIO_dump _ %+ BORINGSSL_PREFIX %+ _BIO_dump
%xdefine _BIO_eof _ %+ BORINGSSL_PREFIX %+ _BIO_eof
%xdefine _BIO_f_base64 _ %+ BORINGSSL_PREFIX %+ _BIO_f_base64
%xdefine _BIO_f_cipher _ %+ BORINGSSL_PREFIX %+ _BIO_f_cipher
%xdefine _BIO_f_md _ %+ BORINGSSL_PREFIX %+ _BIO_f_md
%xdefine _BIO_find_type _ %+ BORINGSSL_PREFIX %+ _BIO_find_type
%xdefine _BIO_flush _ %+ BORINGSSL_PREFIX %+ _BIO_flush
%xdefine _BIO_free _ %+ BORINGSSL_PREFIX %+ _BIO_free
%xdefine _BIO_free_all _ %+ BORINGSSL_PREFIX %+ _BIO_free_all
%xdefine _BIO_get_callback_arg _ %+ BORINGSSL_PREFIX %+ _BIO_get_callback_arg
%xdefine _BIO_get_cipher_ctx _ %+ BORINGSSL_PREFIX %+ _BIO_get_cipher_ctx
%xdefine _BIO_get_cipher_status _ %+ BORINGSSL_PREFIX %+ _BIO_get_cipher_status
%xdefine _BIO_get_close _ %+ BORINGSSL_PREFIX %+ _BIO_get_close
%xdefine _BIO_get_data _ %+ BORINGSSL_PREFIX %+ _BIO_get_data
%xdefine _BIO_get_ex_data _ %+ BORINGSSL_PREFIX %+ _BIO_get_ex_data
%xdefine _BIO_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _BIO_get_ex_new_index
%xdefine _BIO_get_fd _ %+ BORINGSSL_PREFIX %+ _BIO_get_fd
%xdefine _BIO_get_fp _ %+ BORINGSSL_PREFIX %+ _BIO_get_fp
%xdefine _BIO_get_init _ %+ BORINGSSL_PREFIX %+ _BIO_get_init
%xdefine _BIO_get_md _ %+ BORINGSSL_PREFIX %+ _BIO_get_md
%xdefine _BIO_get_md_ctx _ %+ BORINGSSL_PREFIX %+ _BIO_get_md_ctx
%xdefine _BIO_get_mem_ptr _ %+ BORINGSSL_PREFIX %+ _BIO_get_mem_ptr
%xdefine _BIO_get_new_index _ %+ BORINGSSL_PREFIX %+ _BIO_get_new_index
%xdefine _BIO_get_retry_flags _ %+ BORINGSSL_PREFIX %+ _BIO_get_retry_flags
%xdefine _BIO_get_retry_reason _ %+ BORINGSSL_PREFIX %+ _BIO_get_retry_reason
%xdefine _BIO_get_shutdown _ %+ BORINGSSL_PREFIX %+ _BIO_get_shutdown
%xdefine _BIO_gets _ %+ BORINGSSL_PREFIX %+ _BIO_gets
%xdefine _BIO_hexdump _ %+ BORINGSSL_PREFIX %+ _BIO_hexdump
%xdefine _BIO_indent _ %+ BORINGSSL_PREFIX %+ _BIO_indent
%xdefine _BIO_int_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_int_ctrl
%xdefine _BIO_mem_contents _ %+ BORINGSSL_PREFIX %+ _BIO_mem_contents
%xdefine _BIO_meth_free _ %+ BORINGSSL_PREFIX %+ _BIO_meth_free
%xdefine _BIO_meth_get_callback_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_callback_ctrl
%xdefine _BIO_meth_get_create _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_create
%xdefine _BIO_meth_get_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_ctrl
%xdefine _BIO_meth_get_destroy _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_destroy
%xdefine _BIO_meth_get_gets _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_gets
%xdefine _BIO_meth_get_puts _ %+ BORINGSSL_PREFIX %+ _BIO_meth_get_puts
%xdefine _BIO_meth_new _ %+ BORINGSSL_PREFIX %+ _BIO_meth_new
%xdefine _BIO_meth_set_callback_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_callback_ctrl
%xdefine _BIO_meth_set_create _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_create
%xdefine _BIO_meth_set_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_ctrl
%xdefine _BIO_meth_set_destroy _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_destroy
%xdefine _BIO_meth_set_gets _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_gets
%xdefine _BIO_meth_set_puts _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_puts
%xdefine _BIO_meth_set_read _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_read
%xdefine _BIO_meth_set_write _ %+ BORINGSSL_PREFIX %+ _BIO_meth_set_write
%xdefine _BIO_method_name _ %+ BORINGSSL_PREFIX %+ _BIO_method_name
%xdefine _BIO_method_type _ %+ BORINGSSL_PREFIX %+ _BIO_method_type
%xdefine _BIO_new _ %+ BORINGSSL_PREFIX %+ _BIO_new
%xdefine _BIO_new_bio_pair _ %+ BORINGSSL_PREFIX %+ _BIO_new_bio_pair
%xdefine _BIO_new_connect _ %+ BORINGSSL_PREFIX %+ _BIO_new_connect
%xdefine _BIO_new_dgram _ %+ BORINGSSL_PREFIX %+ _BIO_new_dgram
%xdefine _BIO_new_fd _ %+ BORINGSSL_PREFIX %+ _BIO_new_fd
%xdefine _BIO_new_file _ %+ BORINGSSL_PREFIX %+ _BIO_new_file
%xdefine _BIO_new_fp _ %+ BORINGSSL_PREFIX %+ _BIO_new_fp
%xdefine _BIO_new_mem_buf _ %+ BORINGSSL_PREFIX %+ _BIO_new_mem_buf
%xdefine _BIO_new_socket _ %+ BORINGSSL_PREFIX %+ _BIO_new_socket
%xdefine _BIO_next _ %+ BORINGSSL_PREFIX %+ _BIO_next
%xdefine _BIO_number_read _ %+ BORINGSSL_PREFIX %+ _BIO_number_read
%xdefine _BIO_number_written _ %+ BORINGSSL_PREFIX %+ _BIO_number_written
%xdefine _BIO_pending _ %+ BORINGSSL_PREFIX %+ _BIO_pending
%xdefine _BIO_pop _ %+ BORINGSSL_PREFIX %+ _BIO_pop
%xdefine _BIO_printf _ %+ BORINGSSL_PREFIX %+ _BIO_printf
%xdefine _BIO_ptr_ctrl _ %+ BORINGSSL_PREFIX %+ _BIO_ptr_ctrl
%xdefine _BIO_push _ %+ BORINGSSL_PREFIX %+ _BIO_push
%xdefine _BIO_puts _ %+ BORINGSSL_PREFIX %+ _BIO_puts
%xdefine _BIO_read _ %+ BORINGSSL_PREFIX %+ _BIO_read
%xdefine _BIO_read_asn1 _ %+ BORINGSSL_PREFIX %+ _BIO_read_asn1
%xdefine _BIO_read_ex _ %+ BORINGSSL_PREFIX %+ _BIO_read_ex
%xdefine _BIO_read_filename _ %+ BORINGSSL_PREFIX %+ _BIO_read_filename
%xdefine _BIO_reset _ %+ BORINGSSL_PREFIX %+ _BIO_reset
%xdefine _BIO_rw_filename _ %+ BORINGSSL_PREFIX %+ _BIO_rw_filename
%xdefine _BIO_s_connect _ %+ BORINGSSL_PREFIX %+ _BIO_s_connect
%xdefine _BIO_s_datagram _ %+ BORINGSSL_PREFIX %+ _BIO_s_datagram
%xdefine _BIO_s_fd _ %+ BORINGSSL_PREFIX %+ _BIO_s_fd
%xdefine _BIO_s_file _ %+ BORINGSSL_PREFIX %+ _BIO_s_file
%xdefine _BIO_s_mem _ %+ BORINGSSL_PREFIX %+ _BIO_s_mem
%xdefine _BIO_s_secmem _ %+ BORINGSSL_PREFIX %+ _BIO_s_secmem
%xdefine _BIO_s_socket _ %+ BORINGSSL_PREFIX %+ _BIO_s_socket
%xdefine _BIO_seek _ %+ BORINGSSL_PREFIX %+ _BIO_seek
%xdefine _BIO_set_callback _ %+ BORINGSSL_PREFIX %+ _BIO_set_callback
%xdefine _BIO_set_callback_arg _ %+ BORINGSSL_PREFIX %+ _BIO_set_callback_arg
%xdefine _BIO_set_callback_ex _ %+ BORINGSSL_PREFIX %+ _BIO_set_callback_ex
%xdefine _BIO_set_cipher _ %+ BORINGSSL_PREFIX %+ _BIO_set_cipher
%xdefine _BIO_set_close _ %+ BORINGSSL_PREFIX %+ _BIO_set_close
%xdefine _BIO_set_conn_hostname _ %+ BORINGSSL_PREFIX %+ _BIO_set_conn_hostname
%xdefine _BIO_set_conn_int_port _ %+ BORINGSSL_PREFIX %+ _BIO_set_conn_int_port
%xdefine _BIO_set_conn_port _ %+ BORINGSSL_PREFIX %+ _BIO_set_conn_port
%xdefine _BIO_set_data _ %+ BORINGSSL_PREFIX %+ _BIO_set_data
%xdefine _BIO_set_ex_data _ %+ BORINGSSL_PREFIX %+ _BIO_set_ex_data
%xdefine _BIO_set_fd _ %+ BORINGSSL_PREFIX %+ _BIO_set_fd
%xdefine _BIO_set_flags _ %+ BORINGSSL_PREFIX %+ _BIO_set_flags
%xdefine _BIO_set_fp _ %+ BORINGSSL_PREFIX %+ _BIO_set_fp
%xdefine _BIO_set_init _ %+ BORINGSSL_PREFIX %+ _BIO_set_init
%xdefine _BIO_set_md _ %+ BORINGSSL_PREFIX %+ _BIO_set_md
%xdefine _BIO_set_mem_buf _ %+ BORINGSSL_PREFIX %+ _BIO_set_mem_buf
%xdefine _BIO_set_mem_eof_return _ %+ BORINGSSL_PREFIX %+ _BIO_set_mem_eof_return
%xdefine _BIO_set_nbio _ %+ BORINGSSL_PREFIX %+ _BIO_set_nbio
%xdefine _BIO_set_retry_read _ %+ BORINGSSL_PREFIX %+ _BIO_set_retry_read
%xdefine _BIO_set_retry_reason _ %+ BORINGSSL_PREFIX %+ _BIO_set_retry_reason
%xdefine _BIO_set_retry_special _ %+ BORINGSSL_PREFIX %+ _BIO_set_retry_special
%xdefine _BIO_set_retry_write _ %+ BORINGSSL_PREFIX %+ _BIO_set_retry_write
%xdefine _BIO_set_shutdown _ %+ BORINGSSL_PREFIX %+ _BIO_set_shutdown
%xdefine _BIO_set_write_buffer_size _ %+ BORINGSSL_PREFIX %+ _BIO_set_write_buffer_size
%xdefine _BIO_should_io_special _ %+ BORINGSSL_PREFIX %+ _BIO_should_io_special
%xdefine _BIO_should_read _ %+ BORINGSSL_PREFIX %+ _BIO_should_read
%xdefine _BIO_should_retry _ %+ BORINGSSL_PREFIX %+ _BIO_should_retry
%xdefine _BIO_should_write _ %+ BORINGSSL_PREFIX %+ _BIO_should_write
%xdefine _BIO_shutdown_wr _ %+ BORINGSSL_PREFIX %+ _BIO_shutdown_wr
%xdefine _BIO_snprintf _ %+ BORINGSSL_PREFIX %+ _BIO_snprintf
%xdefine _BIO_tell _ %+ BORINGSSL_PREFIX %+ _BIO_tell
%xdefine _BIO_test_flags _ %+ BORINGSSL_PREFIX %+ _BIO_test_flags
%xdefine _BIO_up_ref _ %+ BORINGSSL_PREFIX %+ _BIO_up_ref
%xdefine _BIO_vfree _ %+ BORINGSSL_PREFIX %+ _BIO_vfree
%xdefine _BIO_vsnprintf _ %+ BORINGSSL_PREFIX %+ _BIO_vsnprintf
%xdefine _BIO_wpending _ %+ BORINGSSL_PREFIX %+ _BIO_wpending
%xdefine _BIO_write _ %+ BORINGSSL_PREFIX %+ _BIO_write
%xdefine _BIO_write_all _ %+ BORINGSSL_PREFIX %+ _BIO_write_all
%xdefine _BIO_write_ex _ %+ BORINGSSL_PREFIX %+ _BIO_write_ex
%xdefine _BIO_write_filename _ %+ BORINGSSL_PREFIX %+ _BIO_write_filename
%xdefine _BLAKE2B256 _ %+ BORINGSSL_PREFIX %+ _BLAKE2B256
%xdefine _BLAKE2B256_Final _ %+ BORINGSSL_PREFIX %+ _BLAKE2B256_Final
%xdefine _BLAKE2B256_Init _ %+ BORINGSSL_PREFIX %+ _BLAKE2B256_Init
%xdefine _BLAKE2B256_Update _ %+ BORINGSSL_PREFIX %+ _BLAKE2B256_Update
%xdefine _BN_BLINDING_convert _ %+ BORINGSSL_PREFIX %+ _BN_BLINDING_convert
%xdefine _BN_BLINDING_free _ %+ BORINGSSL_PREFIX %+ _BN_BLINDING_free
%xdefine _BN_BLINDING_invalidate _ %+ BORINGSSL_PREFIX %+ _BN_BLINDING_invalidate
%xdefine _BN_BLINDING_invert _ %+ BORINGSSL_PREFIX %+ _BN_BLINDING_invert
%xdefine _BN_BLINDING_new _ %+ BORINGSSL_PREFIX %+ _BN_BLINDING_new
%xdefine _BN_CTX_end _ %+ BORINGSSL_PREFIX %+ _BN_CTX_end
%xdefine _BN_CTX_free _ %+ BORINGSSL_PREFIX %+ _BN_CTX_free
%xdefine _BN_CTX_get _ %+ BORINGSSL_PREFIX %+ _BN_CTX_get
%xdefine _BN_CTX_new _ %+ BORINGSSL_PREFIX %+ _BN_CTX_new
%xdefine _BN_CTX_secure_new _ %+ BORINGSSL_PREFIX %+ _BN_CTX_secure_new
%xdefine _BN_CTX_start _ %+ BORINGSSL_PREFIX %+ _BN_CTX_start
%xdefine _BN_GENCB_call _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_call
%xdefine _BN_GENCB_free _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_free
%xdefine _BN_GENCB_get_arg _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_get_arg
%xdefine _BN_GENCB_new _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_new
%xdefine _BN_GENCB_set _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_set
%xdefine _BN_GENCB_set_old _ %+ BORINGSSL_PREFIX %+ _BN_GENCB_set_old
%xdefine _BN_MONT_CTX_copy _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_copy
%xdefine _BN_MONT_CTX_free _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_free
%xdefine _BN_MONT_CTX_new _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_new
%xdefine _BN_MONT_CTX_new_consttime _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_new_consttime
%xdefine _BN_MONT_CTX_new_for_modulus _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_new_for_modulus
%xdefine _BN_MONT_CTX_set _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_set
%xdefine _BN_MONT_CTX_set_locked _ %+ BORINGSSL_PREFIX %+ _BN_MONT_CTX_set_locked
%xdefine _BN_abs_is_word _ %+ BORINGSSL_PREFIX %+ _BN_abs_is_word
%xdefine _BN_add _ %+ BORINGSSL_PREFIX %+ _BN_add
%xdefine _BN_add_word _ %+ BORINGSSL_PREFIX %+ _BN_add_word
%xdefine _BN_asc2bn _ %+ BORINGSSL_PREFIX %+ _BN_asc2bn
%xdefine _BN_bin2bn _ %+ BORINGSSL_PREFIX %+ _BN_bin2bn
%xdefine _BN_bn2bin _ %+ BORINGSSL_PREFIX %+ _BN_bn2bin
%xdefine _BN_bn2bin_padded _ %+ BORINGSSL_PREFIX %+ _BN_bn2bin_padded
%xdefine _BN_bn2binpad _ %+ BORINGSSL_PREFIX %+ _BN_bn2binpad
%xdefine _BN_bn2cbb_padded _ %+ BORINGSSL_PREFIX %+ _BN_bn2cbb_padded
%xdefine _BN_bn2dec _ %+ BORINGSSL_PREFIX %+ _BN_bn2dec
%xdefine _BN_bn2hex _ %+ BORINGSSL_PREFIX %+ _BN_bn2hex
%xdefine _BN_bn2le_padded _ %+ BORINGSSL_PREFIX %+ _BN_bn2le_padded
%xdefine _BN_bn2mpi _ %+ BORINGSSL_PREFIX %+ _BN_bn2mpi
%xdefine _BN_clear _ %+ BORINGSSL_PREFIX %+ _BN_clear
%xdefine _BN_clear_bit _ %+ BORINGSSL_PREFIX %+ _BN_clear_bit
%xdefine _BN_clear_free _ %+ BORINGSSL_PREFIX %+ _BN_clear_free
%xdefine _BN_cmp _ %+ BORINGSSL_PREFIX %+ _BN_cmp
%xdefine _BN_cmp_word _ %+ BORINGSSL_PREFIX %+ _BN_cmp_word
%xdefine _BN_copy _ %+ BORINGSSL_PREFIX %+ _BN_copy
%xdefine _BN_count_low_zero_bits _ %+ BORINGSSL_PREFIX %+ _BN_count_low_zero_bits
%xdefine _BN_dec2bn _ %+ BORINGSSL_PREFIX %+ _BN_dec2bn
%xdefine _BN_div _ %+ BORINGSSL_PREFIX %+ _BN_div
%xdefine _BN_div_word _ %+ BORINGSSL_PREFIX %+ _BN_div_word
%xdefine _BN_dup _ %+ BORINGSSL_PREFIX %+ _BN_dup
%xdefine _BN_enhanced_miller_rabin_primality_test _ %+ BORINGSSL_PREFIX %+ _BN_enhanced_miller_rabin_primality_test
%xdefine _BN_equal_consttime _ %+ BORINGSSL_PREFIX %+ _BN_equal_consttime
%xdefine _BN_exp _ %+ BORINGSSL_PREFIX %+ _BN_exp
%xdefine _BN_free _ %+ BORINGSSL_PREFIX %+ _BN_free
%xdefine _BN_from_montgomery _ %+ BORINGSSL_PREFIX %+ _BN_from_montgomery
%xdefine _BN_gcd _ %+ BORINGSSL_PREFIX %+ _BN_gcd
%xdefine _BN_generate_prime_ex _ %+ BORINGSSL_PREFIX %+ _BN_generate_prime_ex
%xdefine _BN_get_flags _ %+ BORINGSSL_PREFIX %+ _BN_get_flags
%xdefine _BN_get_minimal_width _ %+ BORINGSSL_PREFIX %+ _BN_get_minimal_width
%xdefine _BN_get_rfc3526_prime_1536 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_1536
%xdefine _BN_get_rfc3526_prime_2048 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_2048
%xdefine _BN_get_rfc3526_prime_3072 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_3072
%xdefine _BN_get_rfc3526_prime_4096 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_4096
%xdefine _BN_get_rfc3526_prime_6144 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_6144
%xdefine _BN_get_rfc3526_prime_8192 _ %+ BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_8192
%xdefine _BN_get_u64 _ %+ BORINGSSL_PREFIX %+ _BN_get_u64
%xdefine _BN_get_word _ %+ BORINGSSL_PREFIX %+ _BN_get_word
%xdefine _BN_hex2bn _ %+ BORINGSSL_PREFIX %+ _BN_hex2bn
%xdefine _BN_init _ %+ BORINGSSL_PREFIX %+ _BN_init
%xdefine _BN_is_bit_set _ %+ BORINGSSL_PREFIX %+ _BN_is_bit_set
%xdefine _BN_is_negative _ %+ BORINGSSL_PREFIX %+ _BN_is_negative
%xdefine _BN_is_odd _ %+ BORINGSSL_PREFIX %+ _BN_is_odd
%xdefine _BN_is_one _ %+ BORINGSSL_PREFIX %+ _BN_is_one
%xdefine _BN_is_pow2 _ %+ BORINGSSL_PREFIX %+ _BN_is_pow2
%xdefine _BN_is_prime_ex _ %+ BORINGSSL_PREFIX %+ _BN_is_prime_ex
%xdefine _BN_is_prime_fasttest_ex _ %+ BORINGSSL_PREFIX %+ _BN_is_prime_fasttest_ex
%xdefine _BN_is_word _ %+ BORINGSSL_PREFIX %+ _BN_is_word
%xdefine _BN_is_zero _ %+ BORINGSSL_PREFIX %+ _BN_is_zero
%xdefine _BN_le2bn _ %+ BORINGSSL_PREFIX %+ _BN_le2bn
%xdefine _BN_lshift _ %+ BORINGSSL_PREFIX %+ _BN_lshift
%xdefine _BN_lshift1 _ %+ BORINGSSL_PREFIX %+ _BN_lshift1
%xdefine _BN_marshal_asn1 _ %+ BORINGSSL_PREFIX %+ _BN_marshal_asn1
%xdefine _BN_mask_bits _ %+ BORINGSSL_PREFIX %+ _BN_mask_bits
%xdefine _BN_mod_add _ %+ BORINGSSL_PREFIX %+ _BN_mod_add
%xdefine _BN_mod_add_quick _ %+ BORINGSSL_PREFIX %+ _BN_mod_add_quick
%xdefine _BN_mod_exp _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp
%xdefine _BN_mod_exp2_mont _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp2_mont
%xdefine _BN_mod_exp_mont _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp_mont
%xdefine _BN_mod_exp_mont_consttime _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp_mont_consttime
%xdefine _BN_mod_exp_mont_consttime_x2 _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp_mont_consttime_x2
%xdefine _BN_mod_exp_mont_word _ %+ BORINGSSL_PREFIX %+ _BN_mod_exp_mont_word
%xdefine _BN_mod_inverse _ %+ BORINGSSL_PREFIX %+ _BN_mod_inverse
%xdefine _BN_mod_inverse_blinded _ %+ BORINGSSL_PREFIX %+ _BN_mod_inverse_blinded
%xdefine _BN_mod_inverse_odd _ %+ BORINGSSL_PREFIX %+ _BN_mod_inverse_odd
%xdefine _BN_mod_lshift _ %+ BORINGSSL_PREFIX %+ _BN_mod_lshift
%xdefine _BN_mod_lshift1 _ %+ BORINGSSL_PREFIX %+ _BN_mod_lshift1
%xdefine _BN_mod_lshift1_quick _ %+ BORINGSSL_PREFIX %+ _BN_mod_lshift1_quick
%xdefine _BN_mod_lshift_quick _ %+ BORINGSSL_PREFIX %+ _BN_mod_lshift_quick
%xdefine _BN_mod_mul _ %+ BORINGSSL_PREFIX %+ _BN_mod_mul
%xdefine _BN_mod_mul_montgomery _ %+ BORINGSSL_PREFIX %+ _BN_mod_mul_montgomery
%xdefine _BN_mod_pow2 _ %+ BORINGSSL_PREFIX %+ _BN_mod_pow2
%xdefine _BN_mod_sqr _ %+ BORINGSSL_PREFIX %+ _BN_mod_sqr
%xdefine _BN_mod_sqrt _ %+ BORINGSSL_PREFIX %+ _BN_mod_sqrt
%xdefine _BN_mod_sub _ %+ BORINGSSL_PREFIX %+ _BN_mod_sub
%xdefine _BN_mod_sub_quick _ %+ BORINGSSL_PREFIX %+ _BN_mod_sub_quick
%xdefine _BN_mod_word _ %+ BORINGSSL_PREFIX %+ _BN_mod_word
%xdefine _BN_mpi2bn _ %+ BORINGSSL_PREFIX %+ _BN_mpi2bn
%xdefine _BN_mul _ %+ BORINGSSL_PREFIX %+ _BN_mul
%xdefine _BN_mul_word _ %+ BORINGSSL_PREFIX %+ _BN_mul_word
%xdefine _BN_new _ %+ BORINGSSL_PREFIX %+ _BN_new
%xdefine _BN_nnmod _ %+ BORINGSSL_PREFIX %+ _BN_nnmod
%xdefine _BN_nnmod_pow2 _ %+ BORINGSSL_PREFIX %+ _BN_nnmod_pow2
%xdefine _BN_num_bits _ %+ BORINGSSL_PREFIX %+ _BN_num_bits
%xdefine _BN_num_bits_word _ %+ BORINGSSL_PREFIX %+ _BN_num_bits_word
%xdefine _BN_num_bytes _ %+ BORINGSSL_PREFIX %+ _BN_num_bytes
%xdefine _BN_one _ %+ BORINGSSL_PREFIX %+ _BN_one
%xdefine _BN_parse_asn1_unsigned _ %+ BORINGSSL_PREFIX %+ _BN_parse_asn1_unsigned
%xdefine _BN_primality_test _ %+ BORINGSSL_PREFIX %+ _BN_primality_test
%xdefine _BN_print _ %+ BORINGSSL_PREFIX %+ _BN_print
%xdefine _BN_print_fp _ %+ BORINGSSL_PREFIX %+ _BN_print_fp
%xdefine _BN_pseudo_rand _ %+ BORINGSSL_PREFIX %+ _BN_pseudo_rand
%xdefine _BN_pseudo_rand_range _ %+ BORINGSSL_PREFIX %+ _BN_pseudo_rand_range
%xdefine _BN_rand _ %+ BORINGSSL_PREFIX %+ _BN_rand
%xdefine _BN_rand_range _ %+ BORINGSSL_PREFIX %+ _BN_rand_range
%xdefine _BN_rand_range_ex _ %+ BORINGSSL_PREFIX %+ _BN_rand_range_ex
%xdefine _BN_rshift _ %+ BORINGSSL_PREFIX %+ _BN_rshift
%xdefine _BN_rshift1 _ %+ BORINGSSL_PREFIX %+ _BN_rshift1
%xdefine _BN_secure_new _ %+ BORINGSSL_PREFIX %+ _BN_secure_new
%xdefine _BN_set_bit _ %+ BORINGSSL_PREFIX %+ _BN_set_bit
%xdefine _BN_set_flags _ %+ BORINGSSL_PREFIX %+ _BN_set_flags
%xdefine _BN_set_negative _ %+ BORINGSSL_PREFIX %+ _BN_set_negative
%xdefine _BN_set_u64 _ %+ BORINGSSL_PREFIX %+ _BN_set_u64
%xdefine _BN_set_word _ %+ BORINGSSL_PREFIX %+ _BN_set_word
%xdefine _BN_sqr _ %+ BORINGSSL_PREFIX %+ _BN_sqr
%xdefine _BN_sqrt _ %+ BORINGSSL_PREFIX %+ _BN_sqrt
%xdefine _BN_sub _ %+ BORINGSSL_PREFIX %+ _BN_sub
%xdefine _BN_sub_word _ %+ BORINGSSL_PREFIX %+ _BN_sub_word
%xdefine _BN_to_ASN1_ENUMERATED _ %+ BORINGSSL_PREFIX %+ _BN_to_ASN1_ENUMERATED
%xdefine _BN_to_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _BN_to_ASN1_INTEGER
%xdefine _BN_to_montgomery _ %+ BORINGSSL_PREFIX %+ _BN_to_montgomery
%xdefine _BN_uadd _ %+ BORINGSSL_PREFIX %+ _BN_uadd
%xdefine _BN_ucmp _ %+ BORINGSSL_PREFIX %+ _BN_ucmp
%xdefine _BN_usub _ %+ BORINGSSL_PREFIX %+ _BN_usub
%xdefine _BN_value_one _ %+ BORINGSSL_PREFIX %+ _BN_value_one
%xdefine _BN_zero _ %+ BORINGSSL_PREFIX %+ _BN_zero
%xdefine _BORINGSSL_function_hit _ %+ BORINGSSL_PREFIX %+ _BORINGSSL_function_hit
%xdefine _BORINGSSL_self_test _ %+ BORINGSSL_PREFIX %+ _BORINGSSL_self_test
%xdefine _BUF_MEM_append _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_append
%xdefine _BUF_MEM_free _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_free
%xdefine _BUF_MEM_grow _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_grow
%xdefine _BUF_MEM_grow_clean _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_grow_clean
%xdefine _BUF_MEM_new _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_new
%xdefine _BUF_MEM_reserve _ %+ BORINGSSL_PREFIX %+ _BUF_MEM_reserve
%xdefine _BUF_memdup _ %+ BORINGSSL_PREFIX %+ _BUF_memdup
%xdefine _BUF_strdup _ %+ BORINGSSL_PREFIX %+ _BUF_strdup
%xdefine _BUF_strlcat _ %+ BORINGSSL_PREFIX %+ _BUF_strlcat
%xdefine _BUF_strlcpy _ %+ BORINGSSL_PREFIX %+ _BUF_strlcpy
%xdefine _BUF_strndup _ %+ BORINGSSL_PREFIX %+ _BUF_strndup
%xdefine _BUF_strnlen _ %+ BORINGSSL_PREFIX %+ _BUF_strnlen
%xdefine _CAST_S_table0 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table0
%xdefine _CAST_S_table1 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table1
%xdefine _CAST_S_table2 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table2
%xdefine _CAST_S_table3 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table3
%xdefine _CAST_S_table4 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table4
%xdefine _CAST_S_table5 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table5
%xdefine _CAST_S_table6 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table6
%xdefine _CAST_S_table7 _ %+ BORINGSSL_PREFIX %+ _CAST_S_table7
%xdefine _CAST_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _CAST_cbc_encrypt
%xdefine _CAST_decrypt _ %+ BORINGSSL_PREFIX %+ _CAST_decrypt
%xdefine _CAST_ecb_encrypt _ %+ BORINGSSL_PREFIX %+ _CAST_ecb_encrypt
%xdefine _CAST_encrypt _ %+ BORINGSSL_PREFIX %+ _CAST_encrypt
%xdefine _CAST_set_key _ %+ BORINGSSL_PREFIX %+ _CAST_set_key
%xdefine _CBB_add_asn1 _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1
%xdefine _CBB_add_asn1_bool _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_bool
%xdefine _CBB_add_asn1_int64 _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_int64
%xdefine _CBB_add_asn1_int64_with_tag _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_int64_with_tag
%xdefine _CBB_add_asn1_octet_string _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_octet_string
%xdefine _CBB_add_asn1_oid_from_text _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_oid_from_text
%xdefine _CBB_add_asn1_uint64 _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_uint64
%xdefine _CBB_add_asn1_uint64_with_tag _ %+ BORINGSSL_PREFIX %+ _CBB_add_asn1_uint64_with_tag
%xdefine _CBB_add_bytes _ %+ BORINGSSL_PREFIX %+ _CBB_add_bytes
%xdefine _CBB_add_space _ %+ BORINGSSL_PREFIX %+ _CBB_add_space
%xdefine _CBB_add_u16 _ %+ BORINGSSL_PREFIX %+ _CBB_add_u16
%xdefine _CBB_add_u16_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBB_add_u16_length_prefixed
%xdefine _CBB_add_u16le _ %+ BORINGSSL_PREFIX %+ _CBB_add_u16le
%xdefine _CBB_add_u24 _ %+ BORINGSSL_PREFIX %+ _CBB_add_u24
%xdefine _CBB_add_u24_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBB_add_u24_length_prefixed
%xdefine _CBB_add_u32 _ %+ BORINGSSL_PREFIX %+ _CBB_add_u32
%xdefine _CBB_add_u32le _ %+ BORINGSSL_PREFIX %+ _CBB_add_u32le
%xdefine _CBB_add_u64 _ %+ BORINGSSL_PREFIX %+ _CBB_add_u64
%xdefine _CBB_add_u64le _ %+ BORINGSSL_PREFIX %+ _CBB_add_u64le
%xdefine _CBB_add_u8 _ %+ BORINGSSL_PREFIX %+ _CBB_add_u8
%xdefine _CBB_add_u8_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBB_add_u8_length_prefixed
%xdefine _CBB_add_zeros _ %+ BORINGSSL_PREFIX %+ _CBB_add_zeros
%xdefine _CBB_cleanup _ %+ BORINGSSL_PREFIX %+ _CBB_cleanup
%xdefine _CBB_data _ %+ BORINGSSL_PREFIX %+ _CBB_data
%xdefine _CBB_did_write _ %+ BORINGSSL_PREFIX %+ _CBB_did_write
%xdefine _CBB_discard_child _ %+ BORINGSSL_PREFIX %+ _CBB_discard_child
%xdefine _CBB_finish _ %+ BORINGSSL_PREFIX %+ _CBB_finish
%xdefine _CBB_finish_i2d _ %+ BORINGSSL_PREFIX %+ _CBB_finish_i2d
%xdefine _CBB_flush _ %+ BORINGSSL_PREFIX %+ _CBB_flush
%xdefine _CBB_flush_asn1_set_of _ %+ BORINGSSL_PREFIX %+ _CBB_flush_asn1_set_of
%xdefine _CBB_init _ %+ BORINGSSL_PREFIX %+ _CBB_init
%xdefine _CBB_init_fixed _ %+ BORINGSSL_PREFIX %+ _CBB_init_fixed
%xdefine _CBB_len _ %+ BORINGSSL_PREFIX %+ _CBB_len
%xdefine _CBB_reserve _ %+ BORINGSSL_PREFIX %+ _CBB_reserve
%xdefine _CBB_zero _ %+ BORINGSSL_PREFIX %+ _CBB_zero
%xdefine _CBS_asn1_ber_to_der _ %+ BORINGSSL_PREFIX %+ _CBS_asn1_ber_to_der
%xdefine _CBS_asn1_bitstring_has_bit _ %+ BORINGSSL_PREFIX %+ _CBS_asn1_bitstring_has_bit
%xdefine _CBS_asn1_oid_to_text _ %+ BORINGSSL_PREFIX %+ _CBS_asn1_oid_to_text
%xdefine _CBS_contains_zero_byte _ %+ BORINGSSL_PREFIX %+ _CBS_contains_zero_byte
%xdefine _CBS_copy_bytes _ %+ BORINGSSL_PREFIX %+ _CBS_copy_bytes
%xdefine _CBS_data _ %+ BORINGSSL_PREFIX %+ _CBS_data
%xdefine _CBS_get_any_asn1 _ %+ BORINGSSL_PREFIX %+ _CBS_get_any_asn1
%xdefine _CBS_get_any_asn1_element _ %+ BORINGSSL_PREFIX %+ _CBS_get_any_asn1_element
%xdefine _CBS_get_any_ber_asn1_element _ %+ BORINGSSL_PREFIX %+ _CBS_get_any_ber_asn1_element
%xdefine _CBS_get_asn1 _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1
%xdefine _CBS_get_asn1_bool _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1_bool
%xdefine _CBS_get_asn1_element _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1_element
%xdefine _CBS_get_asn1_implicit_string _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1_implicit_string
%xdefine _CBS_get_asn1_int64 _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1_int64
%xdefine _CBS_get_asn1_uint64 _ %+ BORINGSSL_PREFIX %+ _CBS_get_asn1_uint64
%xdefine _CBS_get_bytes _ %+ BORINGSSL_PREFIX %+ _CBS_get_bytes
%xdefine _CBS_get_last_u8 _ %+ BORINGSSL_PREFIX %+ _CBS_get_last_u8
%xdefine _CBS_get_optional_asn1 _ %+ BORINGSSL_PREFIX %+ _CBS_get_optional_asn1
%xdefine _CBS_get_optional_asn1_bool _ %+ BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_bool
%xdefine _CBS_get_optional_asn1_int64 _ %+ BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_int64
%xdefine _CBS_get_optional_asn1_octet_string _ %+ BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_octet_string
%xdefine _CBS_get_optional_asn1_uint64 _ %+ BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_uint64
%xdefine _CBS_get_u16 _ %+ BORINGSSL_PREFIX %+ _CBS_get_u16
%xdefine _CBS_get_u16_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBS_get_u16_length_prefixed
%xdefine _CBS_get_u16le _ %+ BORINGSSL_PREFIX %+ _CBS_get_u16le
%xdefine _CBS_get_u24 _ %+ BORINGSSL_PREFIX %+ _CBS_get_u24
%xdefine _CBS_get_u24_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBS_get_u24_length_prefixed
%xdefine _CBS_get_u32 _ %+ BORINGSSL_PREFIX %+ _CBS_get_u32
%xdefine _CBS_get_u32le _ %+ BORINGSSL_PREFIX %+ _CBS_get_u32le
%xdefine _CBS_get_u64 _ %+ BORINGSSL_PREFIX %+ _CBS_get_u64
%xdefine _CBS_get_u64_decimal _ %+ BORINGSSL_PREFIX %+ _CBS_get_u64_decimal
%xdefine _CBS_get_u64le _ %+ BORINGSSL_PREFIX %+ _CBS_get_u64le
%xdefine _CBS_get_u8 _ %+ BORINGSSL_PREFIX %+ _CBS_get_u8
%xdefine _CBS_get_u8_length_prefixed _ %+ BORINGSSL_PREFIX %+ _CBS_get_u8_length_prefixed
%xdefine _CBS_get_until_first _ %+ BORINGSSL_PREFIX %+ _CBS_get_until_first
%xdefine _CBS_init _ %+ BORINGSSL_PREFIX %+ _CBS_init
%xdefine _CBS_is_unsigned_asn1_integer _ %+ BORINGSSL_PREFIX %+ _CBS_is_unsigned_asn1_integer
%xdefine _CBS_is_valid_asn1_bitstring _ %+ BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_bitstring
%xdefine _CBS_is_valid_asn1_integer _ %+ BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_integer
%xdefine _CBS_is_valid_asn1_oid _ %+ BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_oid
%xdefine _CBS_len _ %+ BORINGSSL_PREFIX %+ _CBS_len
%xdefine _CBS_mem_equal _ %+ BORINGSSL_PREFIX %+ _CBS_mem_equal
%xdefine _CBS_parse_generalized_time _ %+ BORINGSSL_PREFIX %+ _CBS_parse_generalized_time
%xdefine _CBS_parse_utc_time _ %+ BORINGSSL_PREFIX %+ _CBS_parse_utc_time
%xdefine _CBS_peek_asn1_tag _ %+ BORINGSSL_PREFIX %+ _CBS_peek_asn1_tag
%xdefine _CBS_skip _ %+ BORINGSSL_PREFIX %+ _CBS_skip
%xdefine _CBS_stow _ %+ BORINGSSL_PREFIX %+ _CBS_stow
%xdefine _CBS_strdup _ %+ BORINGSSL_PREFIX %+ _CBS_strdup
%xdefine _CERTIFICATEPOLICIES_free _ %+ BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_free
%xdefine _CERTIFICATEPOLICIES_it _ %+ BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_it
%xdefine _CERTIFICATEPOLICIES_new _ %+ BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_new
%xdefine _CMAC_CTX_copy _ %+ BORINGSSL_PREFIX %+ _CMAC_CTX_copy
%xdefine _CMAC_CTX_free _ %+ BORINGSSL_PREFIX %+ _CMAC_CTX_free
%xdefine _CMAC_CTX_get0_cipher_ctx _ %+ BORINGSSL_PREFIX %+ _CMAC_CTX_get0_cipher_ctx
%xdefine _CMAC_CTX_new _ %+ BORINGSSL_PREFIX %+ _CMAC_CTX_new
%xdefine _CMAC_Final _ %+ BORINGSSL_PREFIX %+ _CMAC_Final
%xdefine _CMAC_Init _ %+ BORINGSSL_PREFIX %+ _CMAC_Init
%xdefine _CMAC_Reset _ %+ BORINGSSL_PREFIX %+ _CMAC_Reset
%xdefine _CMAC_Update _ %+ BORINGSSL_PREFIX %+ _CMAC_Update
%xdefine _CONF_VALUE_new _ %+ BORINGSSL_PREFIX %+ _CONF_VALUE_new
%xdefine _CONF_get1_default_config_file _ %+ BORINGSSL_PREFIX %+ _CONF_get1_default_config_file
%xdefine _CONF_modules_finish _ %+ BORINGSSL_PREFIX %+ _CONF_modules_finish
%xdefine _CONF_modules_free _ %+ BORINGSSL_PREFIX %+ _CONF_modules_free
%xdefine _CONF_modules_load_file _ %+ BORINGSSL_PREFIX %+ _CONF_modules_load_file
%xdefine _CONF_modules_unload _ %+ BORINGSSL_PREFIX %+ _CONF_modules_unload
%xdefine _CONF_parse_list _ %+ BORINGSSL_PREFIX %+ _CONF_parse_list
%xdefine _CRL_DIST_POINTS_free _ %+ BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_free
%xdefine _CRL_DIST_POINTS_it _ %+ BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_it
%xdefine _CRL_DIST_POINTS_new _ %+ BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_new
%xdefine _CRYPTO_BUFFER_POOL_free _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_POOL_free
%xdefine _CRYPTO_BUFFER_POOL_new _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_POOL_new
%xdefine _CRYPTO_BUFFER_alloc _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_alloc
%xdefine _CRYPTO_BUFFER_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_data
%xdefine _CRYPTO_BUFFER_free _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_free
%xdefine _CRYPTO_BUFFER_init_CBS _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_init_CBS
%xdefine _CRYPTO_BUFFER_len _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_len
%xdefine _CRYPTO_BUFFER_new _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new
%xdefine _CRYPTO_BUFFER_new_from_CBS _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new_from_CBS
%xdefine _CRYPTO_BUFFER_new_from_static_data_unsafe _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new_from_static_data_unsafe
%xdefine _CRYPTO_BUFFER_up_ref _ %+ BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_up_ref
%xdefine _CRYPTO_MUTEX_cleanup _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_cleanup
%xdefine _CRYPTO_MUTEX_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_init
%xdefine _CRYPTO_MUTEX_lock_read _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_lock_read
%xdefine _CRYPTO_MUTEX_lock_write _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_lock_write
%xdefine _CRYPTO_MUTEX_unlock_read _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_unlock_read
%xdefine _CRYPTO_MUTEX_unlock_write _ %+ BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_unlock_write
%xdefine _CRYPTO_POLYVAL_finish _ %+ BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_finish
%xdefine _CRYPTO_POLYVAL_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_init
%xdefine _CRYPTO_POLYVAL_update_blocks _ %+ BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_update_blocks
%xdefine _CRYPTO_STATIC_MUTEX_is_write_locked _ %+ BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_is_write_locked
%xdefine _CRYPTO_STATIC_MUTEX_lock_read _ %+ BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_lock_read
%xdefine _CRYPTO_STATIC_MUTEX_lock_write _ %+ BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_lock_write
%xdefine _CRYPTO_STATIC_MUTEX_unlock_read _ %+ BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_unlock_read
%xdefine _CRYPTO_STATIC_MUTEX_unlock_write _ %+ BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_unlock_write
%xdefine _CRYPTO_THREADID_current _ %+ BORINGSSL_PREFIX %+ _CRYPTO_THREADID_current
%xdefine _CRYPTO_THREADID_set_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_callback
%xdefine _CRYPTO_THREADID_set_numeric _ %+ BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_numeric
%xdefine _CRYPTO_THREADID_set_pointer _ %+ BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_pointer
%xdefine _CRYPTO_cbc128_decrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cbc128_decrypt
%xdefine _CRYPTO_cbc128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cbc128_encrypt
%xdefine _CRYPTO_cfb128_1_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cfb128_1_encrypt
%xdefine _CRYPTO_cfb128_8_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cfb128_8_encrypt
%xdefine _CRYPTO_cfb128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cfb128_encrypt
%xdefine _CRYPTO_chacha_20 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_chacha_20
%xdefine _CRYPTO_cleanup_all_ex_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_cleanup_all_ex_data
%xdefine _CRYPTO_ctr128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_ctr128_encrypt
%xdefine _CRYPTO_ctr128_encrypt_ctr32 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_ctr128_encrypt_ctr32
%xdefine _CRYPTO_fork_detect_ignore_inheritzero_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _CRYPTO_fork_detect_ignore_inheritzero_FOR_TESTING
%xdefine _CRYPTO_fork_detect_ignore_wipeonfork_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _CRYPTO_fork_detect_ignore_wipeonfork_FOR_TESTING
%xdefine _CRYPTO_free _ %+ BORINGSSL_PREFIX %+ _CRYPTO_free
%xdefine _CRYPTO_free_ex_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_free_ex_data
%xdefine _CRYPTO_gcm128_aad _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_aad
%xdefine _CRYPTO_gcm128_decrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_decrypt
%xdefine _CRYPTO_gcm128_decrypt_ctr32 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_decrypt_ctr32
%xdefine _CRYPTO_gcm128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_encrypt
%xdefine _CRYPTO_gcm128_encrypt_ctr32 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_encrypt_ctr32
%xdefine _CRYPTO_gcm128_finish _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_finish
%xdefine _CRYPTO_gcm128_init_key _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_init_key
%xdefine _CRYPTO_gcm128_setiv _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_setiv
%xdefine _CRYPTO_gcm128_tag _ %+ BORINGSSL_PREFIX %+ _CRYPTO_gcm128_tag
%xdefine _CRYPTO_get_dynlock_create_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_create_callback
%xdefine _CRYPTO_get_dynlock_destroy_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_destroy_callback
%xdefine _CRYPTO_get_dynlock_lock_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_lock_callback
%xdefine _CRYPTO_get_ex_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_ex_data
%xdefine _CRYPTO_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_ex_new_index
%xdefine _CRYPTO_get_fork_ube_generation _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_fork_ube_generation
%xdefine _CRYPTO_get_lock_name _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_lock_name
%xdefine _CRYPTO_get_locking_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_locking_callback
%xdefine _CRYPTO_get_sysgenid_path _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_sysgenid_path
%xdefine _CRYPTO_get_thread_local _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_thread_local
%xdefine _CRYPTO_get_ube_generation_number _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_ube_generation_number
%xdefine _CRYPTO_get_vm_ube_active _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_active
%xdefine _CRYPTO_get_vm_ube_generation _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_generation
%xdefine _CRYPTO_get_vm_ube_supported _ %+ BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_supported
%xdefine _CRYPTO_ghash_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_ghash_init
%xdefine _CRYPTO_has_asm _ %+ BORINGSSL_PREFIX %+ _CRYPTO_has_asm
%xdefine _CRYPTO_has_broken_NEON _ %+ BORINGSSL_PREFIX %+ _CRYPTO_has_broken_NEON
%xdefine _CRYPTO_hchacha20 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_hchacha20
%xdefine _CRYPTO_is_ARMv8_DIT_capable_for_testing _ %+ BORINGSSL_PREFIX %+ _CRYPTO_is_ARMv8_DIT_capable_for_testing
%xdefine _CRYPTO_is_PPC64LE_vcrypto_capable _ %+ BORINGSSL_PREFIX %+ _CRYPTO_is_PPC64LE_vcrypto_capable
%xdefine _CRYPTO_is_confidential_build _ %+ BORINGSSL_PREFIX %+ _CRYPTO_is_confidential_build
%xdefine _CRYPTO_library_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_library_init
%xdefine _CRYPTO_malloc _ %+ BORINGSSL_PREFIX %+ _CRYPTO_malloc
%xdefine _CRYPTO_malloc_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_malloc_init
%xdefine _CRYPTO_mem_ctrl _ %+ BORINGSSL_PREFIX %+ _CRYPTO_mem_ctrl
%xdefine _CRYPTO_memcmp _ %+ BORINGSSL_PREFIX %+ _CRYPTO_memcmp
%xdefine _CRYPTO_needs_hwcap2_workaround _ %+ BORINGSSL_PREFIX %+ _CRYPTO_needs_hwcap2_workaround
%xdefine _CRYPTO_new_ex_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_new_ex_data
%xdefine _CRYPTO_num_locks _ %+ BORINGSSL_PREFIX %+ _CRYPTO_num_locks
%xdefine _CRYPTO_ofb128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_ofb128_encrypt
%xdefine _CRYPTO_once _ %+ BORINGSSL_PREFIX %+ _CRYPTO_once
%xdefine _CRYPTO_poly1305_finish _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_finish
%xdefine _CRYPTO_poly1305_finish_neon _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_finish_neon
%xdefine _CRYPTO_poly1305_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_init
%xdefine _CRYPTO_poly1305_init_neon _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_init_neon
%xdefine _CRYPTO_poly1305_update _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_update
%xdefine _CRYPTO_poly1305_update_neon _ %+ BORINGSSL_PREFIX %+ _CRYPTO_poly1305_update_neon
%xdefine _CRYPTO_pre_sandbox_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_pre_sandbox_init
%xdefine _CRYPTO_rdrand_multiple8 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_rdrand_multiple8
%xdefine _CRYPTO_realloc _ %+ BORINGSSL_PREFIX %+ _CRYPTO_realloc
%xdefine _CRYPTO_refcount_dec_and_test_zero _ %+ BORINGSSL_PREFIX %+ _CRYPTO_refcount_dec_and_test_zero
%xdefine _CRYPTO_refcount_inc _ %+ BORINGSSL_PREFIX %+ _CRYPTO_refcount_inc
%xdefine _CRYPTO_rndr_multiple8 _ %+ BORINGSSL_PREFIX %+ _CRYPTO_rndr_multiple8
%xdefine _CRYPTO_secure_malloc_init _ %+ BORINGSSL_PREFIX %+ _CRYPTO_secure_malloc_init
%xdefine _CRYPTO_secure_malloc_initialized _ %+ BORINGSSL_PREFIX %+ _CRYPTO_secure_malloc_initialized
%xdefine _CRYPTO_secure_used _ %+ BORINGSSL_PREFIX %+ _CRYPTO_secure_used
%xdefine _CRYPTO_set_add_lock_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_add_lock_callback
%xdefine _CRYPTO_set_dynlock_create_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_create_callback
%xdefine _CRYPTO_set_dynlock_destroy_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_destroy_callback
%xdefine _CRYPTO_set_dynlock_lock_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_lock_callback
%xdefine _CRYPTO_set_ex_data _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_ex_data
%xdefine _CRYPTO_set_id_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_id_callback
%xdefine _CRYPTO_set_locking_callback _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_locking_callback
%xdefine _CRYPTO_set_mem_functions _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_mem_functions
%xdefine _CRYPTO_set_thread_local _ %+ BORINGSSL_PREFIX %+ _CRYPTO_set_thread_local
%xdefine _CRYPTO_sysrand _ %+ BORINGSSL_PREFIX %+ _CRYPTO_sysrand
%xdefine _CRYPTO_sysrand_if_available _ %+ BORINGSSL_PREFIX %+ _CRYPTO_sysrand_if_available
%xdefine _CRYPTO_tls1_prf _ %+ BORINGSSL_PREFIX %+ _CRYPTO_tls1_prf
%xdefine _CRYPTO_xts128_encrypt _ %+ BORINGSSL_PREFIX %+ _CRYPTO_xts128_encrypt
%xdefine _CTR_DRBG_clear _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_clear
%xdefine _CTR_DRBG_free _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_free
%xdefine _CTR_DRBG_generate _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_generate
%xdefine _CTR_DRBG_init _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_init
%xdefine _CTR_DRBG_new _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_new
%xdefine _CTR_DRBG_reseed _ %+ BORINGSSL_PREFIX %+ _CTR_DRBG_reseed
%xdefine _ChaCha20_ctr32_avx2 _ %+ BORINGSSL_PREFIX %+ _ChaCha20_ctr32_avx2
%xdefine _ChaCha20_ctr32_neon _ %+ BORINGSSL_PREFIX %+ _ChaCha20_ctr32_neon
%xdefine _ChaCha20_ctr32_nohw _ %+ BORINGSSL_PREFIX %+ _ChaCha20_ctr32_nohw
%xdefine _ChaCha20_ctr32_ssse3 _ %+ BORINGSSL_PREFIX %+ _ChaCha20_ctr32_ssse3
%xdefine _ChaCha20_ctr32_ssse3_4x _ %+ BORINGSSL_PREFIX %+ _ChaCha20_ctr32_ssse3_4x
%xdefine _DES_decrypt3 _ %+ BORINGSSL_PREFIX %+ _DES_decrypt3
%xdefine _DES_ecb3_encrypt _ %+ BORINGSSL_PREFIX %+ _DES_ecb3_encrypt
%xdefine _DES_ecb3_encrypt_ex _ %+ BORINGSSL_PREFIX %+ _DES_ecb3_encrypt_ex
%xdefine _DES_ecb_encrypt _ %+ BORINGSSL_PREFIX %+ _DES_ecb_encrypt
%xdefine _DES_ecb_encrypt_ex _ %+ BORINGSSL_PREFIX %+ _DES_ecb_encrypt_ex
%xdefine _DES_ede2_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _DES_ede2_cbc_encrypt
%xdefine _DES_ede3_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _DES_ede3_cbc_encrypt
%xdefine _DES_ede3_cbc_encrypt_ex _ %+ BORINGSSL_PREFIX %+ _DES_ede3_cbc_encrypt_ex
%xdefine _DES_encrypt3 _ %+ BORINGSSL_PREFIX %+ _DES_encrypt3
%xdefine _DES_is_weak_key _ %+ BORINGSSL_PREFIX %+ _DES_is_weak_key
%xdefine _DES_key_sched _ %+ BORINGSSL_PREFIX %+ _DES_key_sched
%xdefine _DES_ncbc_encrypt _ %+ BORINGSSL_PREFIX %+ _DES_ncbc_encrypt
%xdefine _DES_ncbc_encrypt_ex _ %+ BORINGSSL_PREFIX %+ _DES_ncbc_encrypt_ex
%xdefine _DES_set_key _ %+ BORINGSSL_PREFIX %+ _DES_set_key
%xdefine _DES_set_key_ex _ %+ BORINGSSL_PREFIX %+ _DES_set_key_ex
%xdefine _DES_set_key_unchecked _ %+ BORINGSSL_PREFIX %+ _DES_set_key_unchecked
%xdefine _DES_set_odd_parity _ %+ BORINGSSL_PREFIX %+ _DES_set_odd_parity
%xdefine _DH_bits _ %+ BORINGSSL_PREFIX %+ _DH_bits
%xdefine _DH_check _ %+ BORINGSSL_PREFIX %+ _DH_check
%xdefine _DH_check_pub_key _ %+ BORINGSSL_PREFIX %+ _DH_check_pub_key
%xdefine _DH_clear_flags _ %+ BORINGSSL_PREFIX %+ _DH_clear_flags
%xdefine _DH_compute_key _ %+ BORINGSSL_PREFIX %+ _DH_compute_key
%xdefine _DH_compute_key_hashed _ %+ BORINGSSL_PREFIX %+ _DH_compute_key_hashed
%xdefine _DH_compute_key_padded _ %+ BORINGSSL_PREFIX %+ _DH_compute_key_padded
%xdefine _DH_free _ %+ BORINGSSL_PREFIX %+ _DH_free
%xdefine _DH_generate_key _ %+ BORINGSSL_PREFIX %+ _DH_generate_key
%xdefine _DH_generate_parameters _ %+ BORINGSSL_PREFIX %+ _DH_generate_parameters
%xdefine _DH_generate_parameters_ex _ %+ BORINGSSL_PREFIX %+ _DH_generate_parameters_ex
%xdefine _DH_get0_g _ %+ BORINGSSL_PREFIX %+ _DH_get0_g
%xdefine _DH_get0_key _ %+ BORINGSSL_PREFIX %+ _DH_get0_key
%xdefine _DH_get0_p _ %+ BORINGSSL_PREFIX %+ _DH_get0_p
%xdefine _DH_get0_pqg _ %+ BORINGSSL_PREFIX %+ _DH_get0_pqg
%xdefine _DH_get0_priv_key _ %+ BORINGSSL_PREFIX %+ _DH_get0_priv_key
%xdefine _DH_get0_pub_key _ %+ BORINGSSL_PREFIX %+ _DH_get0_pub_key
%xdefine _DH_get0_q _ %+ BORINGSSL_PREFIX %+ _DH_get0_q
%xdefine _DH_get_2048_256 _ %+ BORINGSSL_PREFIX %+ _DH_get_2048_256
%xdefine _DH_get_rfc7919_2048 _ %+ BORINGSSL_PREFIX %+ _DH_get_rfc7919_2048
%xdefine _DH_get_rfc7919_3072 _ %+ BORINGSSL_PREFIX %+ _DH_get_rfc7919_3072
%xdefine _DH_get_rfc7919_4096 _ %+ BORINGSSL_PREFIX %+ _DH_get_rfc7919_4096
%xdefine _DH_get_rfc7919_8192 _ %+ BORINGSSL_PREFIX %+ _DH_get_rfc7919_8192
%xdefine _DH_marshal_parameters _ %+ BORINGSSL_PREFIX %+ _DH_marshal_parameters
%xdefine _DH_new _ %+ BORINGSSL_PREFIX %+ _DH_new
%xdefine _DH_new_by_nid _ %+ BORINGSSL_PREFIX %+ _DH_new_by_nid
%xdefine _DH_num_bits _ %+ BORINGSSL_PREFIX %+ _DH_num_bits
%xdefine _DH_parse_parameters _ %+ BORINGSSL_PREFIX %+ _DH_parse_parameters
%xdefine _DH_set0_key _ %+ BORINGSSL_PREFIX %+ _DH_set0_key
%xdefine _DH_set0_pqg _ %+ BORINGSSL_PREFIX %+ _DH_set0_pqg
%xdefine _DH_set_length _ %+ BORINGSSL_PREFIX %+ _DH_set_length
%xdefine _DH_size _ %+ BORINGSSL_PREFIX %+ _DH_size
%xdefine _DH_up_ref _ %+ BORINGSSL_PREFIX %+ _DH_up_ref
%xdefine _DHparams_dup _ %+ BORINGSSL_PREFIX %+ _DHparams_dup
%xdefine _DIRECTORYSTRING_free _ %+ BORINGSSL_PREFIX %+ _DIRECTORYSTRING_free
%xdefine _DIRECTORYSTRING_it _ %+ BORINGSSL_PREFIX %+ _DIRECTORYSTRING_it
%xdefine _DIRECTORYSTRING_new _ %+ BORINGSSL_PREFIX %+ _DIRECTORYSTRING_new
%xdefine _DISPLAYTEXT_free _ %+ BORINGSSL_PREFIX %+ _DISPLAYTEXT_free
%xdefine _DISPLAYTEXT_it _ %+ BORINGSSL_PREFIX %+ _DISPLAYTEXT_it
%xdefine _DISPLAYTEXT_new _ %+ BORINGSSL_PREFIX %+ _DISPLAYTEXT_new
%xdefine _DIST_POINT_NAME_free _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_NAME_free
%xdefine _DIST_POINT_NAME_it _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_NAME_it
%xdefine _DIST_POINT_NAME_new _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_NAME_new
%xdefine _DIST_POINT_free _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_free
%xdefine _DIST_POINT_it _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_it
%xdefine _DIST_POINT_new _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_new
%xdefine _DIST_POINT_set_dpname _ %+ BORINGSSL_PREFIX %+ _DIST_POINT_set_dpname
%xdefine _DSA_SIG_free _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_free
%xdefine _DSA_SIG_get0 _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_get0
%xdefine _DSA_SIG_marshal _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_marshal
%xdefine _DSA_SIG_new _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_new
%xdefine _DSA_SIG_parse _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_parse
%xdefine _DSA_SIG_set0 _ %+ BORINGSSL_PREFIX %+ _DSA_SIG_set0
%xdefine _DSA_bits _ %+ BORINGSSL_PREFIX %+ _DSA_bits
%xdefine _DSA_check_signature _ %+ BORINGSSL_PREFIX %+ _DSA_check_signature
%xdefine _DSA_do_check_signature _ %+ BORINGSSL_PREFIX %+ _DSA_do_check_signature
%xdefine _DSA_do_sign _ %+ BORINGSSL_PREFIX %+ _DSA_do_sign
%xdefine _DSA_do_verify _ %+ BORINGSSL_PREFIX %+ _DSA_do_verify
%xdefine _DSA_dup_DH _ %+ BORINGSSL_PREFIX %+ _DSA_dup_DH
%xdefine _DSA_free _ %+ BORINGSSL_PREFIX %+ _DSA_free
%xdefine _DSA_generate_key _ %+ BORINGSSL_PREFIX %+ _DSA_generate_key
%xdefine _DSA_generate_parameters_ex _ %+ BORINGSSL_PREFIX %+ _DSA_generate_parameters_ex
%xdefine _DSA_get0_g _ %+ BORINGSSL_PREFIX %+ _DSA_get0_g
%xdefine _DSA_get0_key _ %+ BORINGSSL_PREFIX %+ _DSA_get0_key
%xdefine _DSA_get0_p _ %+ BORINGSSL_PREFIX %+ _DSA_get0_p
%xdefine _DSA_get0_pqg _ %+ BORINGSSL_PREFIX %+ _DSA_get0_pqg
%xdefine _DSA_get0_priv_key _ %+ BORINGSSL_PREFIX %+ _DSA_get0_priv_key
%xdefine _DSA_get0_pub_key _ %+ BORINGSSL_PREFIX %+ _DSA_get0_pub_key
%xdefine _DSA_get0_q _ %+ BORINGSSL_PREFIX %+ _DSA_get0_q
%xdefine _DSA_get_ex_data _ %+ BORINGSSL_PREFIX %+ _DSA_get_ex_data
%xdefine _DSA_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _DSA_get_ex_new_index
%xdefine _DSA_marshal_parameters _ %+ BORINGSSL_PREFIX %+ _DSA_marshal_parameters
%xdefine _DSA_marshal_private_key _ %+ BORINGSSL_PREFIX %+ _DSA_marshal_private_key
%xdefine _DSA_marshal_public_key _ %+ BORINGSSL_PREFIX %+ _DSA_marshal_public_key
%xdefine _DSA_new _ %+ BORINGSSL_PREFIX %+ _DSA_new
%xdefine _DSA_parse_parameters _ %+ BORINGSSL_PREFIX %+ _DSA_parse_parameters
%xdefine _DSA_parse_private_key _ %+ BORINGSSL_PREFIX %+ _DSA_parse_private_key
%xdefine _DSA_parse_public_key _ %+ BORINGSSL_PREFIX %+ _DSA_parse_public_key
%xdefine _DSA_print _ %+ BORINGSSL_PREFIX %+ _DSA_print
%xdefine _DSA_print_fp _ %+ BORINGSSL_PREFIX %+ _DSA_print_fp
%xdefine _DSA_set0_key _ %+ BORINGSSL_PREFIX %+ _DSA_set0_key
%xdefine _DSA_set0_pqg _ %+ BORINGSSL_PREFIX %+ _DSA_set0_pqg
%xdefine _DSA_set_ex_data _ %+ BORINGSSL_PREFIX %+ _DSA_set_ex_data
%xdefine _DSA_sign _ %+ BORINGSSL_PREFIX %+ _DSA_sign
%xdefine _DSA_size _ %+ BORINGSSL_PREFIX %+ _DSA_size
%xdefine _DSA_up_ref _ %+ BORINGSSL_PREFIX %+ _DSA_up_ref
%xdefine _DSA_verify _ %+ BORINGSSL_PREFIX %+ _DSA_verify
%xdefine _DSAparams_dup _ %+ BORINGSSL_PREFIX %+ _DSAparams_dup
%xdefine _ECDH_compute_key _ %+ BORINGSSL_PREFIX %+ _ECDH_compute_key
%xdefine _ECDH_compute_key_fips _ %+ BORINGSSL_PREFIX %+ _ECDH_compute_key_fips
%xdefine _ECDH_compute_shared_secret _ %+ BORINGSSL_PREFIX %+ _ECDH_compute_shared_secret
%xdefine _ECDSA_SIG_free _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_free
%xdefine _ECDSA_SIG_from_bytes _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_from_bytes
%xdefine _ECDSA_SIG_get0 _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_get0
%xdefine _ECDSA_SIG_get0_r _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_get0_r
%xdefine _ECDSA_SIG_get0_s _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_get0_s
%xdefine _ECDSA_SIG_marshal _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_marshal
%xdefine _ECDSA_SIG_max_len _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_max_len
%xdefine _ECDSA_SIG_new _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_new
%xdefine _ECDSA_SIG_parse _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_parse
%xdefine _ECDSA_SIG_set0 _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_set0
%xdefine _ECDSA_SIG_to_bytes _ %+ BORINGSSL_PREFIX %+ _ECDSA_SIG_to_bytes
%xdefine _ECDSA_do_sign _ %+ BORINGSSL_PREFIX %+ _ECDSA_do_sign
%xdefine _ECDSA_do_verify _ %+ BORINGSSL_PREFIX %+ _ECDSA_do_verify
%xdefine _ECDSA_sign _ %+ BORINGSSL_PREFIX %+ _ECDSA_sign
%xdefine _ECDSA_sign_with_nonce_and_leak_private_key_for_testing _ %+ BORINGSSL_PREFIX %+ _ECDSA_sign_with_nonce_and_leak_private_key_for_testing
%xdefine _ECDSA_size _ %+ BORINGSSL_PREFIX %+ _ECDSA_size
%xdefine _ECDSA_verify _ %+ BORINGSSL_PREFIX %+ _ECDSA_verify
%xdefine _ECPKParameters_print _ %+ BORINGSSL_PREFIX %+ _ECPKParameters_print
%xdefine _EC_GFp_mont_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_mont_method
%xdefine _EC_GFp_nistp224_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_nistp224_method
%xdefine _EC_GFp_nistp256_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_nistp256_method
%xdefine _EC_GFp_nistp384_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_nistp384_method
%xdefine _EC_GFp_nistp521_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_nistp521_method
%xdefine _EC_GFp_nistz256_method _ %+ BORINGSSL_PREFIX %+ _EC_GFp_nistz256_method
%xdefine _EC_GROUP_cmp _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_cmp
%xdefine _EC_GROUP_dup _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_dup
%xdefine _EC_GROUP_free _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_free
%xdefine _EC_GROUP_get0_generator _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get0_generator
%xdefine _EC_GROUP_get0_order _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get0_order
%xdefine _EC_GROUP_get0_seed _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get0_seed
%xdefine _EC_GROUP_get_asn1_flag _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_asn1_flag
%xdefine _EC_GROUP_get_cofactor _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_cofactor
%xdefine _EC_GROUP_get_curve_GFp _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_curve_GFp
%xdefine _EC_GROUP_get_curve_name _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_curve_name
%xdefine _EC_GROUP_get_degree _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_degree
%xdefine _EC_GROUP_get_order _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_order
%xdefine _EC_GROUP_get_point_conversion_form _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_point_conversion_form
%xdefine _EC_GROUP_get_seed_len _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_get_seed_len
%xdefine _EC_GROUP_method_of _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_method_of
%xdefine _EC_GROUP_new_by_curve_name _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_new_by_curve_name
%xdefine _EC_GROUP_new_by_curve_name_mutable _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_new_by_curve_name_mutable
%xdefine _EC_GROUP_new_curve_GFp _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_new_curve_GFp
%xdefine _EC_GROUP_order_bits _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_order_bits
%xdefine _EC_GROUP_set_asn1_flag _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_set_asn1_flag
%xdefine _EC_GROUP_set_generator _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_set_generator
%xdefine _EC_GROUP_set_point_conversion_form _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_set_point_conversion_form
%xdefine _EC_GROUP_set_seed _ %+ BORINGSSL_PREFIX %+ _EC_GROUP_set_seed
%xdefine _EC_KEY_METHOD_free _ %+ BORINGSSL_PREFIX %+ _EC_KEY_METHOD_free
%xdefine _EC_KEY_METHOD_new _ %+ BORINGSSL_PREFIX %+ _EC_KEY_METHOD_new
%xdefine _EC_KEY_METHOD_set_flags _ %+ BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_flags
%xdefine _EC_KEY_METHOD_set_init_awslc _ %+ BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_init_awslc
%xdefine _EC_KEY_METHOD_set_sign_awslc _ %+ BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_sign_awslc
%xdefine _EC_KEY_OpenSSL _ %+ BORINGSSL_PREFIX %+ _EC_KEY_OpenSSL
%xdefine _EC_KEY_check_fips _ %+ BORINGSSL_PREFIX %+ _EC_KEY_check_fips
%xdefine _EC_KEY_check_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_check_key
%xdefine _EC_KEY_decoded_from_explicit_params _ %+ BORINGSSL_PREFIX %+ _EC_KEY_decoded_from_explicit_params
%xdefine _EC_KEY_derive_from_secret _ %+ BORINGSSL_PREFIX %+ _EC_KEY_derive_from_secret
%xdefine _EC_KEY_dup _ %+ BORINGSSL_PREFIX %+ _EC_KEY_dup
%xdefine _EC_KEY_free _ %+ BORINGSSL_PREFIX %+ _EC_KEY_free
%xdefine _EC_KEY_generate_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_generate_key
%xdefine _EC_KEY_generate_key_fips _ %+ BORINGSSL_PREFIX %+ _EC_KEY_generate_key_fips
%xdefine _EC_KEY_get0_group _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get0_group
%xdefine _EC_KEY_get0_private_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get0_private_key
%xdefine _EC_KEY_get0_public_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get0_public_key
%xdefine _EC_KEY_get_conv_form _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_conv_form
%xdefine _EC_KEY_get_default_method _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_default_method
%xdefine _EC_KEY_get_enc_flags _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_enc_flags
%xdefine _EC_KEY_get_ex_data _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_ex_data
%xdefine _EC_KEY_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_ex_new_index
%xdefine _EC_KEY_get_method _ %+ BORINGSSL_PREFIX %+ _EC_KEY_get_method
%xdefine _EC_KEY_is_opaque _ %+ BORINGSSL_PREFIX %+ _EC_KEY_is_opaque
%xdefine _EC_KEY_key2buf _ %+ BORINGSSL_PREFIX %+ _EC_KEY_key2buf
%xdefine _EC_KEY_marshal_curve_name _ %+ BORINGSSL_PREFIX %+ _EC_KEY_marshal_curve_name
%xdefine _EC_KEY_marshal_private_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_marshal_private_key
%xdefine _EC_KEY_new _ %+ BORINGSSL_PREFIX %+ _EC_KEY_new
%xdefine _EC_KEY_new_by_curve_name _ %+ BORINGSSL_PREFIX %+ _EC_KEY_new_by_curve_name
%xdefine _EC_KEY_new_method _ %+ BORINGSSL_PREFIX %+ _EC_KEY_new_method
%xdefine _EC_KEY_parse_curve_name _ %+ BORINGSSL_PREFIX %+ _EC_KEY_parse_curve_name
%xdefine _EC_KEY_parse_parameters _ %+ BORINGSSL_PREFIX %+ _EC_KEY_parse_parameters
%xdefine _EC_KEY_parse_parameters_and_type _ %+ BORINGSSL_PREFIX %+ _EC_KEY_parse_parameters_and_type
%xdefine _EC_KEY_parse_private_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_parse_private_key
%xdefine _EC_KEY_set_asn1_flag _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_asn1_flag
%xdefine _EC_KEY_set_conv_form _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_conv_form
%xdefine _EC_KEY_set_enc_flags _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_enc_flags
%xdefine _EC_KEY_set_ex_data _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_ex_data
%xdefine _EC_KEY_set_group _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_group
%xdefine _EC_KEY_set_method _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_method
%xdefine _EC_KEY_set_private_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_private_key
%xdefine _EC_KEY_set_public_key _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_public_key
%xdefine _EC_KEY_set_public_key_affine_coordinates _ %+ BORINGSSL_PREFIX %+ _EC_KEY_set_public_key_affine_coordinates
%xdefine _EC_KEY_up_ref _ %+ BORINGSSL_PREFIX %+ _EC_KEY_up_ref
%xdefine _EC_METHOD_get_field_type _ %+ BORINGSSL_PREFIX %+ _EC_METHOD_get_field_type
%xdefine _EC_POINT_add _ %+ BORINGSSL_PREFIX %+ _EC_POINT_add
%xdefine _EC_POINT_bn2point _ %+ BORINGSSL_PREFIX %+ _EC_POINT_bn2point
%xdefine _EC_POINT_clear_free _ %+ BORINGSSL_PREFIX %+ _EC_POINT_clear_free
%xdefine _EC_POINT_cmp _ %+ BORINGSSL_PREFIX %+ _EC_POINT_cmp
%xdefine _EC_POINT_copy _ %+ BORINGSSL_PREFIX %+ _EC_POINT_copy
%xdefine _EC_POINT_dbl _ %+ BORINGSSL_PREFIX %+ _EC_POINT_dbl
%xdefine _EC_POINT_dup _ %+ BORINGSSL_PREFIX %+ _EC_POINT_dup
%xdefine _EC_POINT_free _ %+ BORINGSSL_PREFIX %+ _EC_POINT_free
%xdefine _EC_POINT_get_affine_coordinates _ %+ BORINGSSL_PREFIX %+ _EC_POINT_get_affine_coordinates
%xdefine _EC_POINT_get_affine_coordinates_GFp _ %+ BORINGSSL_PREFIX %+ _EC_POINT_get_affine_coordinates_GFp
%xdefine _EC_POINT_invert _ %+ BORINGSSL_PREFIX %+ _EC_POINT_invert
%xdefine _EC_POINT_is_at_infinity _ %+ BORINGSSL_PREFIX %+ _EC_POINT_is_at_infinity
%xdefine _EC_POINT_is_on_curve _ %+ BORINGSSL_PREFIX %+ _EC_POINT_is_on_curve
%xdefine _EC_POINT_mul _ %+ BORINGSSL_PREFIX %+ _EC_POINT_mul
%xdefine _EC_POINT_new _ %+ BORINGSSL_PREFIX %+ _EC_POINT_new
%xdefine _EC_POINT_oct2point _ %+ BORINGSSL_PREFIX %+ _EC_POINT_oct2point
%xdefine _EC_POINT_point2bn _ %+ BORINGSSL_PREFIX %+ _EC_POINT_point2bn
%xdefine _EC_POINT_point2cbb _ %+ BORINGSSL_PREFIX %+ _EC_POINT_point2cbb
%xdefine _EC_POINT_point2oct _ %+ BORINGSSL_PREFIX %+ _EC_POINT_point2oct
%xdefine _EC_POINT_set_affine_coordinates _ %+ BORINGSSL_PREFIX %+ _EC_POINT_set_affine_coordinates
%xdefine _EC_POINT_set_affine_coordinates_GFp _ %+ BORINGSSL_PREFIX %+ _EC_POINT_set_affine_coordinates_GFp
%xdefine _EC_POINT_set_compressed_coordinates_GFp _ %+ BORINGSSL_PREFIX %+ _EC_POINT_set_compressed_coordinates_GFp
%xdefine _EC_POINT_set_to_infinity _ %+ BORINGSSL_PREFIX %+ _EC_POINT_set_to_infinity
%xdefine _EC_curve_nid2nist _ %+ BORINGSSL_PREFIX %+ _EC_curve_nid2nist
%xdefine _EC_curve_nist2nid _ %+ BORINGSSL_PREFIX %+ _EC_curve_nist2nid
%xdefine _EC_get_builtin_curves _ %+ BORINGSSL_PREFIX %+ _EC_get_builtin_curves
%xdefine _EC_group_p224 _ %+ BORINGSSL_PREFIX %+ _EC_group_p224
%xdefine _EC_group_p256 _ %+ BORINGSSL_PREFIX %+ _EC_group_p256
%xdefine _EC_group_p384 _ %+ BORINGSSL_PREFIX %+ _EC_group_p384
%xdefine _EC_group_p521 _ %+ BORINGSSL_PREFIX %+ _EC_group_p521
%xdefine _EC_group_secp256k1 _ %+ BORINGSSL_PREFIX %+ _EC_group_secp256k1
%xdefine _EC_hash_to_curve_p256_xmd_sha256_sswu _ %+ BORINGSSL_PREFIX %+ _EC_hash_to_curve_p256_xmd_sha256_sswu
%xdefine _EC_hash_to_curve_p384_xmd_sha384_sswu _ %+ BORINGSSL_PREFIX %+ _EC_hash_to_curve_p384_xmd_sha384_sswu
%xdefine _ED25519_check_public_key _ %+ BORINGSSL_PREFIX %+ _ED25519_check_public_key
%xdefine _ED25519_keypair _ %+ BORINGSSL_PREFIX %+ _ED25519_keypair
%xdefine _ED25519_keypair_from_seed _ %+ BORINGSSL_PREFIX %+ _ED25519_keypair_from_seed
%xdefine _ED25519_keypair_internal _ %+ BORINGSSL_PREFIX %+ _ED25519_keypair_internal
%xdefine _ED25519_sign _ %+ BORINGSSL_PREFIX %+ _ED25519_sign
%xdefine _ED25519_sign_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519_sign_no_self_test
%xdefine _ED25519_verify _ %+ BORINGSSL_PREFIX %+ _ED25519_verify
%xdefine _ED25519_verify_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519_verify_no_self_test
%xdefine _ED25519ctx_sign _ %+ BORINGSSL_PREFIX %+ _ED25519ctx_sign
%xdefine _ED25519ctx_sign_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ctx_sign_no_self_test
%xdefine _ED25519ctx_verify _ %+ BORINGSSL_PREFIX %+ _ED25519ctx_verify
%xdefine _ED25519ctx_verify_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ctx_verify_no_self_test
%xdefine _ED25519ph_sign _ %+ BORINGSSL_PREFIX %+ _ED25519ph_sign
%xdefine _ED25519ph_sign_digest _ %+ BORINGSSL_PREFIX %+ _ED25519ph_sign_digest
%xdefine _ED25519ph_sign_digest_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ph_sign_digest_no_self_test
%xdefine _ED25519ph_sign_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ph_sign_no_self_test
%xdefine _ED25519ph_verify _ %+ BORINGSSL_PREFIX %+ _ED25519ph_verify
%xdefine _ED25519ph_verify_digest _ %+ BORINGSSL_PREFIX %+ _ED25519ph_verify_digest
%xdefine _ED25519ph_verify_digest_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ph_verify_digest_no_self_test
%xdefine _ED25519ph_verify_no_self_test _ %+ BORINGSSL_PREFIX %+ _ED25519ph_verify_no_self_test
%xdefine _EDIPARTYNAME_free _ %+ BORINGSSL_PREFIX %+ _EDIPARTYNAME_free
%xdefine _EDIPARTYNAME_it _ %+ BORINGSSL_PREFIX %+ _EDIPARTYNAME_it
%xdefine _EDIPARTYNAME_new _ %+ BORINGSSL_PREFIX %+ _EDIPARTYNAME_new
%xdefine _ENGINE_cleanup _ %+ BORINGSSL_PREFIX %+ _ENGINE_cleanup
%xdefine _ENGINE_free _ %+ BORINGSSL_PREFIX %+ _ENGINE_free
%xdefine _ENGINE_get_EC _ %+ BORINGSSL_PREFIX %+ _ENGINE_get_EC
%xdefine _ENGINE_get_RSA _ %+ BORINGSSL_PREFIX %+ _ENGINE_get_RSA
%xdefine _ENGINE_load_builtin_engines _ %+ BORINGSSL_PREFIX %+ _ENGINE_load_builtin_engines
%xdefine _ENGINE_new _ %+ BORINGSSL_PREFIX %+ _ENGINE_new
%xdefine _ENGINE_register_all_ciphers _ %+ BORINGSSL_PREFIX %+ _ENGINE_register_all_ciphers
%xdefine _ENGINE_register_all_complete _ %+ BORINGSSL_PREFIX %+ _ENGINE_register_all_complete
%xdefine _ENGINE_register_all_digests _ %+ BORINGSSL_PREFIX %+ _ENGINE_register_all_digests
%xdefine _ENGINE_set_EC _ %+ BORINGSSL_PREFIX %+ _ENGINE_set_EC
%xdefine _ENGINE_set_RSA _ %+ BORINGSSL_PREFIX %+ _ENGINE_set_RSA
%xdefine _ERR_SAVE_STATE_free _ %+ BORINGSSL_PREFIX %+ _ERR_SAVE_STATE_free
%xdefine _ERR_add_error_data _ %+ BORINGSSL_PREFIX %+ _ERR_add_error_data
%xdefine _ERR_add_error_dataf _ %+ BORINGSSL_PREFIX %+ _ERR_add_error_dataf
%xdefine _ERR_clear_error _ %+ BORINGSSL_PREFIX %+ _ERR_clear_error
%xdefine _ERR_clear_system_error _ %+ BORINGSSL_PREFIX %+ _ERR_clear_system_error
%xdefine _ERR_error_string _ %+ BORINGSSL_PREFIX %+ _ERR_error_string
%xdefine _ERR_error_string_n _ %+ BORINGSSL_PREFIX %+ _ERR_error_string_n
%xdefine _ERR_free_strings _ %+ BORINGSSL_PREFIX %+ _ERR_free_strings
%xdefine _ERR_func_error_string _ %+ BORINGSSL_PREFIX %+ _ERR_func_error_string
%xdefine _ERR_get_error _ %+ BORINGSSL_PREFIX %+ _ERR_get_error
%xdefine _ERR_get_error_line _ %+ BORINGSSL_PREFIX %+ _ERR_get_error_line
%xdefine _ERR_get_error_line_data _ %+ BORINGSSL_PREFIX %+ _ERR_get_error_line_data
%xdefine _ERR_get_next_error_library _ %+ BORINGSSL_PREFIX %+ _ERR_get_next_error_library
%xdefine _ERR_lib_error_string _ %+ BORINGSSL_PREFIX %+ _ERR_lib_error_string
%xdefine _ERR_load_BIO_strings _ %+ BORINGSSL_PREFIX %+ _ERR_load_BIO_strings
%xdefine _ERR_load_CRYPTO_strings _ %+ BORINGSSL_PREFIX %+ _ERR_load_CRYPTO_strings
%xdefine _ERR_load_ERR_strings _ %+ BORINGSSL_PREFIX %+ _ERR_load_ERR_strings
%xdefine _ERR_load_RAND_strings _ %+ BORINGSSL_PREFIX %+ _ERR_load_RAND_strings
%xdefine _ERR_load_crypto_strings _ %+ BORINGSSL_PREFIX %+ _ERR_load_crypto_strings
%xdefine _ERR_peek_error _ %+ BORINGSSL_PREFIX %+ _ERR_peek_error
%xdefine _ERR_peek_error_line _ %+ BORINGSSL_PREFIX %+ _ERR_peek_error_line
%xdefine _ERR_peek_error_line_data _ %+ BORINGSSL_PREFIX %+ _ERR_peek_error_line_data
%xdefine _ERR_peek_last_error _ %+ BORINGSSL_PREFIX %+ _ERR_peek_last_error
%xdefine _ERR_peek_last_error_line _ %+ BORINGSSL_PREFIX %+ _ERR_peek_last_error_line
%xdefine _ERR_peek_last_error_line_data _ %+ BORINGSSL_PREFIX %+ _ERR_peek_last_error_line_data
%xdefine _ERR_pop_to_mark _ %+ BORINGSSL_PREFIX %+ _ERR_pop_to_mark
%xdefine _ERR_print_errors _ %+ BORINGSSL_PREFIX %+ _ERR_print_errors
%xdefine _ERR_print_errors_cb _ %+ BORINGSSL_PREFIX %+ _ERR_print_errors_cb
%xdefine _ERR_print_errors_fp _ %+ BORINGSSL_PREFIX %+ _ERR_print_errors_fp
%xdefine _ERR_put_error _ %+ BORINGSSL_PREFIX %+ _ERR_put_error
%xdefine _ERR_reason_error_string _ %+ BORINGSSL_PREFIX %+ _ERR_reason_error_string
%xdefine _ERR_remove_state _ %+ BORINGSSL_PREFIX %+ _ERR_remove_state
%xdefine _ERR_remove_thread_state _ %+ BORINGSSL_PREFIX %+ _ERR_remove_thread_state
%xdefine _ERR_restore_state _ %+ BORINGSSL_PREFIX %+ _ERR_restore_state
%xdefine _ERR_save_state _ %+ BORINGSSL_PREFIX %+ _ERR_save_state
%xdefine _ERR_set_error_data _ %+ BORINGSSL_PREFIX %+ _ERR_set_error_data
%xdefine _ERR_set_mark _ %+ BORINGSSL_PREFIX %+ _ERR_set_mark
%xdefine _EVP_AEAD_CTX_aead _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_aead
%xdefine _EVP_AEAD_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_cleanup
%xdefine _EVP_AEAD_CTX_deserialize_state _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_deserialize_state
%xdefine _EVP_AEAD_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_free
%xdefine _EVP_AEAD_CTX_get_aead_id _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_get_aead_id
%xdefine _EVP_AEAD_CTX_get_iv _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_get_iv
%xdefine _EVP_AEAD_CTX_init _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_init
%xdefine _EVP_AEAD_CTX_init_with_direction _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_init_with_direction
%xdefine _EVP_AEAD_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_new
%xdefine _EVP_AEAD_CTX_open _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_open
%xdefine _EVP_AEAD_CTX_open_gather _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_open_gather
%xdefine _EVP_AEAD_CTX_seal _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_seal
%xdefine _EVP_AEAD_CTX_seal_scatter _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_seal_scatter
%xdefine _EVP_AEAD_CTX_serialize_state _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_serialize_state
%xdefine _EVP_AEAD_CTX_tag_len _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_tag_len
%xdefine _EVP_AEAD_CTX_zero _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_zero
%xdefine _EVP_AEAD_get_iv_from_ipv4_nanosecs _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_get_iv_from_ipv4_nanosecs
%xdefine _EVP_AEAD_key_length _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_key_length
%xdefine _EVP_AEAD_max_overhead _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_max_overhead
%xdefine _EVP_AEAD_max_tag_len _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_max_tag_len
%xdefine _EVP_AEAD_nonce_length _ %+ BORINGSSL_PREFIX %+ _EVP_AEAD_nonce_length
%xdefine _EVP_BytesToKey _ %+ BORINGSSL_PREFIX %+ _EVP_BytesToKey
%xdefine _EVP_CIPHER_CTX_block_size _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_block_size
%xdefine _EVP_CIPHER_CTX_cipher _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_cipher
%xdefine _EVP_CIPHER_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_cleanup
%xdefine _EVP_CIPHER_CTX_copy _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_copy
%xdefine _EVP_CIPHER_CTX_ctrl _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_ctrl
%xdefine _EVP_CIPHER_CTX_encrypting _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_encrypting
%xdefine _EVP_CIPHER_CTX_flags _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_flags
%xdefine _EVP_CIPHER_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_free
%xdefine _EVP_CIPHER_CTX_get_app_data _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_get_app_data
%xdefine _EVP_CIPHER_CTX_init _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_init
%xdefine _EVP_CIPHER_CTX_iv_length _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_iv_length
%xdefine _EVP_CIPHER_CTX_key_length _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_key_length
%xdefine _EVP_CIPHER_CTX_mode _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_mode
%xdefine _EVP_CIPHER_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_new
%xdefine _EVP_CIPHER_CTX_nid _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_nid
%xdefine _EVP_CIPHER_CTX_reset _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_reset
%xdefine _EVP_CIPHER_CTX_set_app_data _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_app_data
%xdefine _EVP_CIPHER_CTX_set_flags _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_flags
%xdefine _EVP_CIPHER_CTX_set_key_length _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_key_length
%xdefine _EVP_CIPHER_CTX_set_padding _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_padding
%xdefine _EVP_CIPHER_block_size _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_block_size
%xdefine _EVP_CIPHER_do_all_sorted _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_do_all_sorted
%xdefine _EVP_CIPHER_flags _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_flags
%xdefine _EVP_CIPHER_iv_length _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_iv_length
%xdefine _EVP_CIPHER_key_length _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_key_length
%xdefine _EVP_CIPHER_mode _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_mode
%xdefine _EVP_CIPHER_name _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_name
%xdefine _EVP_CIPHER_nid _ %+ BORINGSSL_PREFIX %+ _EVP_CIPHER_nid
%xdefine _EVP_Cipher _ %+ BORINGSSL_PREFIX %+ _EVP_Cipher
%xdefine _EVP_CipherFinal _ %+ BORINGSSL_PREFIX %+ _EVP_CipherFinal
%xdefine _EVP_CipherFinal_ex _ %+ BORINGSSL_PREFIX %+ _EVP_CipherFinal_ex
%xdefine _EVP_CipherInit _ %+ BORINGSSL_PREFIX %+ _EVP_CipherInit
%xdefine _EVP_CipherInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_CipherInit_ex
%xdefine _EVP_CipherUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_CipherUpdate
%xdefine _EVP_DecodeBase64 _ %+ BORINGSSL_PREFIX %+ _EVP_DecodeBase64
%xdefine _EVP_DecodeBlock _ %+ BORINGSSL_PREFIX %+ _EVP_DecodeBlock
%xdefine _EVP_DecodeFinal _ %+ BORINGSSL_PREFIX %+ _EVP_DecodeFinal
%xdefine _EVP_DecodeInit _ %+ BORINGSSL_PREFIX %+ _EVP_DecodeInit
%xdefine _EVP_DecodeUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_DecodeUpdate
%xdefine _EVP_DecodedLength _ %+ BORINGSSL_PREFIX %+ _EVP_DecodedLength
%xdefine _EVP_DecryptFinal _ %+ BORINGSSL_PREFIX %+ _EVP_DecryptFinal
%xdefine _EVP_DecryptFinal_ex _ %+ BORINGSSL_PREFIX %+ _EVP_DecryptFinal_ex
%xdefine _EVP_DecryptInit _ %+ BORINGSSL_PREFIX %+ _EVP_DecryptInit
%xdefine _EVP_DecryptInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_DecryptInit_ex
%xdefine _EVP_DecryptUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_DecryptUpdate
%xdefine _EVP_Digest _ %+ BORINGSSL_PREFIX %+ _EVP_Digest
%xdefine _EVP_DigestFinal _ %+ BORINGSSL_PREFIX %+ _EVP_DigestFinal
%xdefine _EVP_DigestFinalXOF _ %+ BORINGSSL_PREFIX %+ _EVP_DigestFinalXOF
%xdefine _EVP_DigestFinal_ex _ %+ BORINGSSL_PREFIX %+ _EVP_DigestFinal_ex
%xdefine _EVP_DigestInit _ %+ BORINGSSL_PREFIX %+ _EVP_DigestInit
%xdefine _EVP_DigestInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_DigestInit_ex
%xdefine _EVP_DigestSign _ %+ BORINGSSL_PREFIX %+ _EVP_DigestSign
%xdefine _EVP_DigestSignFinal _ %+ BORINGSSL_PREFIX %+ _EVP_DigestSignFinal
%xdefine _EVP_DigestSignInit _ %+ BORINGSSL_PREFIX %+ _EVP_DigestSignInit
%xdefine _EVP_DigestSignUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_DigestSignUpdate
%xdefine _EVP_DigestSqueeze _ %+ BORINGSSL_PREFIX %+ _EVP_DigestSqueeze
%xdefine _EVP_DigestUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_DigestUpdate
%xdefine _EVP_DigestVerify _ %+ BORINGSSL_PREFIX %+ _EVP_DigestVerify
%xdefine _EVP_DigestVerifyFinal _ %+ BORINGSSL_PREFIX %+ _EVP_DigestVerifyFinal
%xdefine _EVP_DigestVerifyInit _ %+ BORINGSSL_PREFIX %+ _EVP_DigestVerifyInit
%xdefine _EVP_DigestVerifyUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_DigestVerifyUpdate
%xdefine _EVP_ENCODE_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_ENCODE_CTX_free
%xdefine _EVP_ENCODE_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_ENCODE_CTX_new
%xdefine _EVP_EncodeBlock _ %+ BORINGSSL_PREFIX %+ _EVP_EncodeBlock
%xdefine _EVP_EncodeFinal _ %+ BORINGSSL_PREFIX %+ _EVP_EncodeFinal
%xdefine _EVP_EncodeInit _ %+ BORINGSSL_PREFIX %+ _EVP_EncodeInit
%xdefine _EVP_EncodeUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_EncodeUpdate
%xdefine _EVP_EncodedLength _ %+ BORINGSSL_PREFIX %+ _EVP_EncodedLength
%xdefine _EVP_EncryptFinal _ %+ BORINGSSL_PREFIX %+ _EVP_EncryptFinal
%xdefine _EVP_EncryptFinal_ex _ %+ BORINGSSL_PREFIX %+ _EVP_EncryptFinal_ex
%xdefine _EVP_EncryptInit _ %+ BORINGSSL_PREFIX %+ _EVP_EncryptInit
%xdefine _EVP_EncryptInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_EncryptInit_ex
%xdefine _EVP_EncryptUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_EncryptUpdate
%xdefine _EVP_HPKE_AEAD_aead _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_AEAD_aead
%xdefine _EVP_HPKE_AEAD_id _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_AEAD_id
%xdefine _EVP_HPKE_CTX_aead _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_aead
%xdefine _EVP_HPKE_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_cleanup
%xdefine _EVP_HPKE_CTX_export _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_export
%xdefine _EVP_HPKE_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_free
%xdefine _EVP_HPKE_CTX_kdf _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_kdf
%xdefine _EVP_HPKE_CTX_kem _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_kem
%xdefine _EVP_HPKE_CTX_max_overhead _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_max_overhead
%xdefine _EVP_HPKE_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_new
%xdefine _EVP_HPKE_CTX_open _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_open
%xdefine _EVP_HPKE_CTX_seal _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_seal
%xdefine _EVP_HPKE_CTX_setup_auth_recipient _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_recipient
%xdefine _EVP_HPKE_CTX_setup_auth_sender _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_sender
%xdefine _EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing
%xdefine _EVP_HPKE_CTX_setup_recipient _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_recipient
%xdefine _EVP_HPKE_CTX_setup_sender _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_sender
%xdefine _EVP_HPKE_CTX_setup_sender_with_seed_for_testing _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_sender_with_seed_for_testing
%xdefine _EVP_HPKE_CTX_zero _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_zero
%xdefine _EVP_HPKE_KDF_hkdf_md _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KDF_hkdf_md
%xdefine _EVP_HPKE_KDF_id _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KDF_id
%xdefine _EVP_HPKE_KEM_enc_len _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_enc_len
%xdefine _EVP_HPKE_KEM_id _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_id
%xdefine _EVP_HPKE_KEM_private_key_len _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_private_key_len
%xdefine _EVP_HPKE_KEM_public_key_len _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_public_key_len
%xdefine _EVP_HPKE_KEY_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_cleanup
%xdefine _EVP_HPKE_KEY_copy _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_copy
%xdefine _EVP_HPKE_KEY_free _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_free
%xdefine _EVP_HPKE_KEY_generate _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_generate
%xdefine _EVP_HPKE_KEY_init _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_init
%xdefine _EVP_HPKE_KEY_kem _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_kem
%xdefine _EVP_HPKE_KEY_move _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_move
%xdefine _EVP_HPKE_KEY_new _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_new
%xdefine _EVP_HPKE_KEY_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_private_key
%xdefine _EVP_HPKE_KEY_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_public_key
%xdefine _EVP_HPKE_KEY_zero _ %+ BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_zero
%xdefine _EVP_MD_CTX_block_size _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_block_size
%xdefine _EVP_MD_CTX_cleanse _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_cleanse
%xdefine _EVP_MD_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_cleanup
%xdefine _EVP_MD_CTX_copy _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_copy
%xdefine _EVP_MD_CTX_copy_ex _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_copy_ex
%xdefine _EVP_MD_CTX_create _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_create
%xdefine _EVP_MD_CTX_destroy _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_destroy
%xdefine _EVP_MD_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_free
%xdefine _EVP_MD_CTX_get_pkey_ctx _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_get_pkey_ctx
%xdefine _EVP_MD_CTX_init _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_init
%xdefine _EVP_MD_CTX_md _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_md
%xdefine _EVP_MD_CTX_move _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_move
%xdefine _EVP_MD_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_new
%xdefine _EVP_MD_CTX_pkey_ctx _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_pkey_ctx
%xdefine _EVP_MD_CTX_reset _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_reset
%xdefine _EVP_MD_CTX_set_flags _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_set_flags
%xdefine _EVP_MD_CTX_set_pkey_ctx _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_set_pkey_ctx
%xdefine _EVP_MD_CTX_size _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_size
%xdefine _EVP_MD_CTX_type _ %+ BORINGSSL_PREFIX %+ _EVP_MD_CTX_type
%xdefine _EVP_MD_block_size _ %+ BORINGSSL_PREFIX %+ _EVP_MD_block_size
%xdefine _EVP_MD_do_all _ %+ BORINGSSL_PREFIX %+ _EVP_MD_do_all
%xdefine _EVP_MD_do_all_sorted _ %+ BORINGSSL_PREFIX %+ _EVP_MD_do_all_sorted
%xdefine _EVP_MD_flags _ %+ BORINGSSL_PREFIX %+ _EVP_MD_flags
%xdefine _EVP_MD_get0_name _ %+ BORINGSSL_PREFIX %+ _EVP_MD_get0_name
%xdefine _EVP_MD_get_pkey_type _ %+ BORINGSSL_PREFIX %+ _EVP_MD_get_pkey_type
%xdefine _EVP_MD_meth_get_flags _ %+ BORINGSSL_PREFIX %+ _EVP_MD_meth_get_flags
%xdefine _EVP_MD_name _ %+ BORINGSSL_PREFIX %+ _EVP_MD_name
%xdefine _EVP_MD_nid _ %+ BORINGSSL_PREFIX %+ _EVP_MD_nid
%xdefine _EVP_MD_pkey_type _ %+ BORINGSSL_PREFIX %+ _EVP_MD_pkey_type
%xdefine _EVP_MD_size _ %+ BORINGSSL_PREFIX %+ _EVP_MD_size
%xdefine _EVP_MD_type _ %+ BORINGSSL_PREFIX %+ _EVP_MD_type
%xdefine _EVP_MD_unstable_sha3_enable _ %+ BORINGSSL_PREFIX %+ _EVP_MD_unstable_sha3_enable
%xdefine _EVP_MD_unstable_sha3_is_enabled _ %+ BORINGSSL_PREFIX %+ _EVP_MD_unstable_sha3_is_enabled
%xdefine _EVP_PBE_scrypt _ %+ BORINGSSL_PREFIX %+ _EVP_PBE_scrypt
%xdefine _EVP_PKCS82PKEY _ %+ BORINGSSL_PREFIX %+ _EVP_PKCS82PKEY
%xdefine _EVP_PKEY2PKCS8 _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY2PKCS8
%xdefine _EVP_PKEY_CTX_add1_hkdf_info _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_add1_hkdf_info
%xdefine _EVP_PKEY_CTX_ctrl _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_ctrl
%xdefine _EVP_PKEY_CTX_ctrl_str _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_ctrl_str
%xdefine _EVP_PKEY_CTX_dup _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_dup
%xdefine _EVP_PKEY_CTX_free _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_free
%xdefine _EVP_PKEY_CTX_get0_pkey _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_pkey
%xdefine _EVP_PKEY_CTX_get0_rsa_oaep_label _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_rsa_oaep_label
%xdefine _EVP_PKEY_CTX_get0_signature_context _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_signature_context
%xdefine _EVP_PKEY_CTX_get_app_data _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_app_data
%xdefine _EVP_PKEY_CTX_get_keygen_info _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_keygen_info
%xdefine _EVP_PKEY_CTX_get_rsa_mgf1_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_mgf1_md
%xdefine _EVP_PKEY_CTX_get_rsa_oaep_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_oaep_md
%xdefine _EVP_PKEY_CTX_get_rsa_padding _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_padding
%xdefine _EVP_PKEY_CTX_get_rsa_pss_saltlen _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_pss_saltlen
%xdefine _EVP_PKEY_CTX_get_signature_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_signature_md
%xdefine _EVP_PKEY_CTX_hkdf_mode _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_hkdf_mode
%xdefine _EVP_PKEY_CTX_kem_set_params _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_kem_set_params
%xdefine _EVP_PKEY_CTX_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_md
%xdefine _EVP_PKEY_CTX_new _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_new
%xdefine _EVP_PKEY_CTX_new_id _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_new_id
%xdefine _EVP_PKEY_CTX_pqdsa_set_params _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_pqdsa_set_params
%xdefine _EVP_PKEY_CTX_set0_rsa_oaep_label _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set0_rsa_oaep_label
%xdefine _EVP_PKEY_CTX_set1_hkdf_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set1_hkdf_key
%xdefine _EVP_PKEY_CTX_set1_hkdf_salt _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set1_hkdf_salt
%xdefine _EVP_PKEY_CTX_set_app_data _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_app_data
%xdefine _EVP_PKEY_CTX_set_cb _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_cb
%xdefine _EVP_PKEY_CTX_set_dh_pad _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_pad
%xdefine _EVP_PKEY_CTX_set_dh_paramgen_generator _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_paramgen_generator
%xdefine _EVP_PKEY_CTX_set_dh_paramgen_prime_len _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_paramgen_prime_len
%xdefine _EVP_PKEY_CTX_set_dsa_paramgen_bits _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_bits
%xdefine _EVP_PKEY_CTX_set_dsa_paramgen_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_md
%xdefine _EVP_PKEY_CTX_set_dsa_paramgen_q_bits _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_q_bits
%xdefine _EVP_PKEY_CTX_set_ec_param_enc _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_ec_param_enc
%xdefine _EVP_PKEY_CTX_set_ec_paramgen_curve_nid _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_ec_paramgen_curve_nid
%xdefine _EVP_PKEY_CTX_set_hkdf_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_hkdf_md
%xdefine _EVP_PKEY_CTX_set_rsa_keygen_bits _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_keygen_bits
%xdefine _EVP_PKEY_CTX_set_rsa_keygen_pubexp _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_keygen_pubexp
%xdefine _EVP_PKEY_CTX_set_rsa_mgf1_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_mgf1_md
%xdefine _EVP_PKEY_CTX_set_rsa_oaep_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_oaep_md
%xdefine _EVP_PKEY_CTX_set_rsa_padding _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_padding
%xdefine _EVP_PKEY_CTX_set_rsa_pss_keygen_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_md
%xdefine _EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md
%xdefine _EVP_PKEY_CTX_set_rsa_pss_keygen_saltlen _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_saltlen
%xdefine _EVP_PKEY_CTX_set_rsa_pss_saltlen _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_saltlen
%xdefine _EVP_PKEY_CTX_set_signature_context _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_signature_context
%xdefine _EVP_PKEY_CTX_set_signature_md _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_signature_md
%xdefine _EVP_PKEY_asn1_find _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_find
%xdefine _EVP_PKEY_asn1_find_str _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_find_str
%xdefine _EVP_PKEY_asn1_get0 _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get0
%xdefine _EVP_PKEY_asn1_get0_info _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get0_info
%xdefine _EVP_PKEY_asn1_get_count _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get_count
%xdefine _EVP_PKEY_assign _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_assign
%xdefine _EVP_PKEY_assign_DH _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_assign_DH
%xdefine _EVP_PKEY_assign_DSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_assign_DSA
%xdefine _EVP_PKEY_assign_EC_KEY _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_assign_EC_KEY
%xdefine _EVP_PKEY_assign_RSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_assign_RSA
%xdefine _EVP_PKEY_base_id _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_base_id
%xdefine _EVP_PKEY_bits _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_bits
%xdefine _EVP_PKEY_check _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_check
%xdefine _EVP_PKEY_cmp _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_cmp
%xdefine _EVP_PKEY_cmp_parameters _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_cmp_parameters
%xdefine _EVP_PKEY_copy_parameters _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_copy_parameters
%xdefine _EVP_PKEY_decapsulate _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_decapsulate
%xdefine _EVP_PKEY_decrypt _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_decrypt
%xdefine _EVP_PKEY_decrypt_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_decrypt_init
%xdefine _EVP_PKEY_derive _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_derive
%xdefine _EVP_PKEY_derive_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_derive_init
%xdefine _EVP_PKEY_derive_set_peer _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_derive_set_peer
%xdefine _EVP_PKEY_ec_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_ec_pkey_meth
%xdefine _EVP_PKEY_ed25519_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_ed25519_pkey_meth
%xdefine _EVP_PKEY_ed25519ph_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_ed25519ph_pkey_meth
%xdefine _EVP_PKEY_encapsulate _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_encapsulate
%xdefine _EVP_PKEY_encapsulate_deterministic _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_encapsulate_deterministic
%xdefine _EVP_PKEY_encrypt _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_encrypt
%xdefine _EVP_PKEY_encrypt_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_encrypt_init
%xdefine _EVP_PKEY_free _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_free
%xdefine _EVP_PKEY_get0 _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get0
%xdefine _EVP_PKEY_get0_DH _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get0_DH
%xdefine _EVP_PKEY_get0_DSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get0_DSA
%xdefine _EVP_PKEY_get0_EC_KEY _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get0_EC_KEY
%xdefine _EVP_PKEY_get0_RSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get0_RSA
%xdefine _EVP_PKEY_get1_DH _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get1_DH
%xdefine _EVP_PKEY_get1_DSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get1_DSA
%xdefine _EVP_PKEY_get1_EC_KEY _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get1_EC_KEY
%xdefine _EVP_PKEY_get1_RSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get1_RSA
%xdefine _EVP_PKEY_get1_tls_encodedpoint _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get1_tls_encodedpoint
%xdefine _EVP_PKEY_get_raw_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get_raw_private_key
%xdefine _EVP_PKEY_get_raw_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_get_raw_public_key
%xdefine _EVP_PKEY_hkdf_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_hkdf_pkey_meth
%xdefine _EVP_PKEY_hmac_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_hmac_pkey_meth
%xdefine _EVP_PKEY_id _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_id
%xdefine _EVP_PKEY_is_opaque _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_is_opaque
%xdefine _EVP_PKEY_kem_check_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_check_key
%xdefine _EVP_PKEY_kem_new_raw_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_key
%xdefine _EVP_PKEY_kem_new_raw_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_public_key
%xdefine _EVP_PKEY_kem_new_raw_secret_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_secret_key
%xdefine _EVP_PKEY_kem_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_pkey_meth
%xdefine _EVP_PKEY_kem_set_params _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_kem_set_params
%xdefine _EVP_PKEY_keygen _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_keygen
%xdefine _EVP_PKEY_keygen_deterministic _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_keygen_deterministic
%xdefine _EVP_PKEY_keygen_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_keygen_init
%xdefine _EVP_PKEY_missing_parameters _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_missing_parameters
%xdefine _EVP_PKEY_new _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_new
%xdefine _EVP_PKEY_new_mac_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_new_mac_key
%xdefine _EVP_PKEY_new_raw_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_new_raw_private_key
%xdefine _EVP_PKEY_new_raw_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_new_raw_public_key
%xdefine _EVP_PKEY_param_check _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_param_check
%xdefine _EVP_PKEY_paramgen _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_paramgen
%xdefine _EVP_PKEY_paramgen_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_paramgen_init
%xdefine _EVP_PKEY_pqdsa_new_raw_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_new_raw_private_key
%xdefine _EVP_PKEY_pqdsa_new_raw_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_new_raw_public_key
%xdefine _EVP_PKEY_pqdsa_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_pkey_meth
%xdefine _EVP_PKEY_pqdsa_set_params _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_set_params
%xdefine _EVP_PKEY_print_params _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_print_params
%xdefine _EVP_PKEY_print_private _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_print_private
%xdefine _EVP_PKEY_print_public _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_print_public
%xdefine _EVP_PKEY_public_check _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_public_check
%xdefine _EVP_PKEY_rsa_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_rsa_pkey_meth
%xdefine _EVP_PKEY_rsa_pss_pkey_meth _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_rsa_pss_pkey_meth
%xdefine _EVP_PKEY_set1_DH _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set1_DH
%xdefine _EVP_PKEY_set1_DSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set1_DSA
%xdefine _EVP_PKEY_set1_EC_KEY _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set1_EC_KEY
%xdefine _EVP_PKEY_set1_RSA _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set1_RSA
%xdefine _EVP_PKEY_set1_tls_encodedpoint _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set1_tls_encodedpoint
%xdefine _EVP_PKEY_set_type _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set_type
%xdefine _EVP_PKEY_set_type_str _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_set_type_str
%xdefine _EVP_PKEY_sign _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_sign
%xdefine _EVP_PKEY_sign_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_sign_init
%xdefine _EVP_PKEY_size _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_size
%xdefine _EVP_PKEY_type _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_type
%xdefine _EVP_PKEY_up_ref _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_up_ref
%xdefine _EVP_PKEY_verify _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_verify
%xdefine _EVP_PKEY_verify_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_verify_init
%xdefine _EVP_PKEY_verify_recover _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_verify_recover
%xdefine _EVP_PKEY_verify_recover_init _ %+ BORINGSSL_PREFIX %+ _EVP_PKEY_verify_recover_init
%xdefine _EVP_RSA_PKEY_CTX_ctrl _ %+ BORINGSSL_PREFIX %+ _EVP_RSA_PKEY_CTX_ctrl
%xdefine _EVP_SignFinal _ %+ BORINGSSL_PREFIX %+ _EVP_SignFinal
%xdefine _EVP_SignInit _ %+ BORINGSSL_PREFIX %+ _EVP_SignInit
%xdefine _EVP_SignInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_SignInit_ex
%xdefine _EVP_SignUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_SignUpdate
%xdefine _EVP_VerifyFinal _ %+ BORINGSSL_PREFIX %+ _EVP_VerifyFinal
%xdefine _EVP_VerifyInit _ %+ BORINGSSL_PREFIX %+ _EVP_VerifyInit
%xdefine _EVP_VerifyInit_ex _ %+ BORINGSSL_PREFIX %+ _EVP_VerifyInit_ex
%xdefine _EVP_VerifyUpdate _ %+ BORINGSSL_PREFIX %+ _EVP_VerifyUpdate
%xdefine _EVP_add_cipher_alias _ %+ BORINGSSL_PREFIX %+ _EVP_add_cipher_alias
%xdefine _EVP_add_digest _ %+ BORINGSSL_PREFIX %+ _EVP_add_digest
%xdefine _EVP_aead_aes_128_cbc_sha1_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha1_tls
%xdefine _EVP_aead_aes_128_cbc_sha1_tls_implicit_iv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha1_tls_implicit_iv
%xdefine _EVP_aead_aes_128_cbc_sha256_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha256_tls
%xdefine _EVP_aead_aes_128_cbc_sha256_tls_implicit_iv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha256_tls_implicit_iv
%xdefine _EVP_aead_aes_128_ccm_bluetooth _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_bluetooth
%xdefine _EVP_aead_aes_128_ccm_bluetooth_8 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_bluetooth_8
%xdefine _EVP_aead_aes_128_ccm_matter _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_matter
%xdefine _EVP_aead_aes_128_ctr_hmac_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ctr_hmac_sha256
%xdefine _EVP_aead_aes_128_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm
%xdefine _EVP_aead_aes_128_gcm_randnonce _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_randnonce
%xdefine _EVP_aead_aes_128_gcm_siv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_siv
%xdefine _EVP_aead_aes_128_gcm_tls12 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_tls12
%xdefine _EVP_aead_aes_128_gcm_tls13 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_tls13
%xdefine _EVP_aead_aes_192_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_192_gcm
%xdefine _EVP_aead_aes_256_cbc_sha1_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha1_tls
%xdefine _EVP_aead_aes_256_cbc_sha1_tls_implicit_iv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha1_tls_implicit_iv
%xdefine _EVP_aead_aes_256_cbc_sha384_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha384_tls
%xdefine _EVP_aead_aes_256_ctr_hmac_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_ctr_hmac_sha256
%xdefine _EVP_aead_aes_256_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm
%xdefine _EVP_aead_aes_256_gcm_randnonce _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_randnonce
%xdefine _EVP_aead_aes_256_gcm_siv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_siv
%xdefine _EVP_aead_aes_256_gcm_tls12 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_tls12
%xdefine _EVP_aead_aes_256_gcm_tls13 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_tls13
%xdefine _EVP_aead_chacha20_poly1305 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_chacha20_poly1305
%xdefine _EVP_aead_des_ede3_cbc_sha1_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_des_ede3_cbc_sha1_tls
%xdefine _EVP_aead_des_ede3_cbc_sha1_tls_implicit_iv _ %+ BORINGSSL_PREFIX %+ _EVP_aead_des_ede3_cbc_sha1_tls_implicit_iv
%xdefine _EVP_aead_null_sha1_tls _ %+ BORINGSSL_PREFIX %+ _EVP_aead_null_sha1_tls
%xdefine _EVP_aead_xchacha20_poly1305 _ %+ BORINGSSL_PREFIX %+ _EVP_aead_xchacha20_poly1305
%xdefine _EVP_aes_128_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cbc
%xdefine _EVP_aes_128_cbc_hmac_sha1 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cbc_hmac_sha1
%xdefine _EVP_aes_128_cbc_hmac_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cbc_hmac_sha256
%xdefine _EVP_aes_128_ccm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_ccm
%xdefine _EVP_aes_128_cfb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cfb
%xdefine _EVP_aes_128_cfb1 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cfb1
%xdefine _EVP_aes_128_cfb128 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cfb128
%xdefine _EVP_aes_128_cfb8 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_cfb8
%xdefine _EVP_aes_128_ctr _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_ctr
%xdefine _EVP_aes_128_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_ecb
%xdefine _EVP_aes_128_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_gcm
%xdefine _EVP_aes_128_ofb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_128_ofb
%xdefine _EVP_aes_192_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_cbc
%xdefine _EVP_aes_192_ccm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_ccm
%xdefine _EVP_aes_192_cfb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_cfb
%xdefine _EVP_aes_192_cfb1 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_cfb1
%xdefine _EVP_aes_192_cfb128 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_cfb128
%xdefine _EVP_aes_192_cfb8 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_cfb8
%xdefine _EVP_aes_192_ctr _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_ctr
%xdefine _EVP_aes_192_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_ecb
%xdefine _EVP_aes_192_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_gcm
%xdefine _EVP_aes_192_ofb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_192_ofb
%xdefine _EVP_aes_256_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cbc
%xdefine _EVP_aes_256_cbc_hmac_sha1 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cbc_hmac_sha1
%xdefine _EVP_aes_256_cbc_hmac_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cbc_hmac_sha256
%xdefine _EVP_aes_256_ccm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_ccm
%xdefine _EVP_aes_256_cfb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cfb
%xdefine _EVP_aes_256_cfb1 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cfb1
%xdefine _EVP_aes_256_cfb128 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cfb128
%xdefine _EVP_aes_256_cfb8 _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_cfb8
%xdefine _EVP_aes_256_ctr _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_ctr
%xdefine _EVP_aes_256_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_ecb
%xdefine _EVP_aes_256_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_gcm
%xdefine _EVP_aes_256_ofb _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_ofb
%xdefine _EVP_aes_256_wrap _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_wrap
%xdefine _EVP_aes_256_xts _ %+ BORINGSSL_PREFIX %+ _EVP_aes_256_xts
%xdefine _EVP_bf_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_bf_cbc
%xdefine _EVP_bf_cfb _ %+ BORINGSSL_PREFIX %+ _EVP_bf_cfb
%xdefine _EVP_bf_cfb64 _ %+ BORINGSSL_PREFIX %+ _EVP_bf_cfb64
%xdefine _EVP_bf_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_bf_ecb
%xdefine _EVP_bf_ofb _ %+ BORINGSSL_PREFIX %+ _EVP_bf_ofb
%xdefine _EVP_blake2b256 _ %+ BORINGSSL_PREFIX %+ _EVP_blake2b256
%xdefine _EVP_cast5_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_cast5_cbc
%xdefine _EVP_cast5_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_cast5_ecb
%xdefine _EVP_chacha20_poly1305 _ %+ BORINGSSL_PREFIX %+ _EVP_chacha20_poly1305
%xdefine _EVP_cleanup _ %+ BORINGSSL_PREFIX %+ _EVP_cleanup
%xdefine _EVP_des_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_des_cbc
%xdefine _EVP_des_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_des_ecb
%xdefine _EVP_des_ede _ %+ BORINGSSL_PREFIX %+ _EVP_des_ede
%xdefine _EVP_des_ede3 _ %+ BORINGSSL_PREFIX %+ _EVP_des_ede3
%xdefine _EVP_des_ede3_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_des_ede3_cbc
%xdefine _EVP_des_ede3_ecb _ %+ BORINGSSL_PREFIX %+ _EVP_des_ede3_ecb
%xdefine _EVP_des_ede_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_des_ede_cbc
%xdefine _EVP_enc_null _ %+ BORINGSSL_PREFIX %+ _EVP_enc_null
%xdefine _EVP_final_with_secret_suffix_sha1 _ %+ BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha1
%xdefine _EVP_final_with_secret_suffix_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha256
%xdefine _EVP_final_with_secret_suffix_sha384 _ %+ BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha384
%xdefine _EVP_get_cipherbyname _ %+ BORINGSSL_PREFIX %+ _EVP_get_cipherbyname
%xdefine _EVP_get_cipherbynid _ %+ BORINGSSL_PREFIX %+ _EVP_get_cipherbynid
%xdefine _EVP_get_digestbyname _ %+ BORINGSSL_PREFIX %+ _EVP_get_digestbyname
%xdefine _EVP_get_digestbynid _ %+ BORINGSSL_PREFIX %+ _EVP_get_digestbynid
%xdefine _EVP_get_digestbyobj _ %+ BORINGSSL_PREFIX %+ _EVP_get_digestbyobj
%xdefine _EVP_get_pw_prompt _ %+ BORINGSSL_PREFIX %+ _EVP_get_pw_prompt
%xdefine _EVP_has_aes_hardware _ %+ BORINGSSL_PREFIX %+ _EVP_has_aes_hardware
%xdefine _EVP_hpke_aes_128_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_hpke_aes_128_gcm
%xdefine _EVP_hpke_aes_256_gcm _ %+ BORINGSSL_PREFIX %+ _EVP_hpke_aes_256_gcm
%xdefine _EVP_hpke_chacha20_poly1305 _ %+ BORINGSSL_PREFIX %+ _EVP_hpke_chacha20_poly1305
%xdefine _EVP_hpke_hkdf_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_hpke_hkdf_sha256
%xdefine _EVP_hpke_x25519_hkdf_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_hpke_x25519_hkdf_sha256
%xdefine _EVP_marshal_digest_algorithm _ %+ BORINGSSL_PREFIX %+ _EVP_marshal_digest_algorithm
%xdefine _EVP_marshal_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_marshal_private_key
%xdefine _EVP_marshal_private_key_v2 _ %+ BORINGSSL_PREFIX %+ _EVP_marshal_private_key_v2
%xdefine _EVP_marshal_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_marshal_public_key
%xdefine _EVP_md4 _ %+ BORINGSSL_PREFIX %+ _EVP_md4
%xdefine _EVP_md5 _ %+ BORINGSSL_PREFIX %+ _EVP_md5
%xdefine _EVP_md5_sha1 _ %+ BORINGSSL_PREFIX %+ _EVP_md5_sha1
%xdefine _EVP_md_null _ %+ BORINGSSL_PREFIX %+ _EVP_md_null
%xdefine _EVP_parse_digest_algorithm _ %+ BORINGSSL_PREFIX %+ _EVP_parse_digest_algorithm
%xdefine _EVP_parse_private_key _ %+ BORINGSSL_PREFIX %+ _EVP_parse_private_key
%xdefine _EVP_parse_public_key _ %+ BORINGSSL_PREFIX %+ _EVP_parse_public_key
%xdefine _EVP_rc2_40_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_rc2_40_cbc
%xdefine _EVP_rc2_cbc _ %+ BORINGSSL_PREFIX %+ _EVP_rc2_cbc
%xdefine _EVP_rc4 _ %+ BORINGSSL_PREFIX %+ _EVP_rc4
%xdefine _EVP_read_pw_string _ %+ BORINGSSL_PREFIX %+ _EVP_read_pw_string
%xdefine _EVP_read_pw_string_min _ %+ BORINGSSL_PREFIX %+ _EVP_read_pw_string_min
%xdefine _EVP_ripemd160 _ %+ BORINGSSL_PREFIX %+ _EVP_ripemd160
%xdefine _EVP_sha1 _ %+ BORINGSSL_PREFIX %+ _EVP_sha1
%xdefine _EVP_sha224 _ %+ BORINGSSL_PREFIX %+ _EVP_sha224
%xdefine _EVP_sha256 _ %+ BORINGSSL_PREFIX %+ _EVP_sha256
%xdefine _EVP_sha384 _ %+ BORINGSSL_PREFIX %+ _EVP_sha384
%xdefine _EVP_sha3_224 _ %+ BORINGSSL_PREFIX %+ _EVP_sha3_224
%xdefine _EVP_sha3_256 _ %+ BORINGSSL_PREFIX %+ _EVP_sha3_256
%xdefine _EVP_sha3_384 _ %+ BORINGSSL_PREFIX %+ _EVP_sha3_384
%xdefine _EVP_sha3_512 _ %+ BORINGSSL_PREFIX %+ _EVP_sha3_512
%xdefine _EVP_sha512 _ %+ BORINGSSL_PREFIX %+ _EVP_sha512
%xdefine _EVP_sha512_224 _ %+ BORINGSSL_PREFIX %+ _EVP_sha512_224
%xdefine _EVP_sha512_256 _ %+ BORINGSSL_PREFIX %+ _EVP_sha512_256
%xdefine _EVP_shake128 _ %+ BORINGSSL_PREFIX %+ _EVP_shake128
%xdefine _EVP_shake256 _ %+ BORINGSSL_PREFIX %+ _EVP_shake256
%xdefine _EVP_tls_cbc_copy_mac _ %+ BORINGSSL_PREFIX %+ _EVP_tls_cbc_copy_mac
%xdefine _EVP_tls_cbc_digest_record _ %+ BORINGSSL_PREFIX %+ _EVP_tls_cbc_digest_record
%xdefine _EVP_tls_cbc_record_digest_supported _ %+ BORINGSSL_PREFIX %+ _EVP_tls_cbc_record_digest_supported
%xdefine _EVP_tls_cbc_remove_padding _ %+ BORINGSSL_PREFIX %+ _EVP_tls_cbc_remove_padding
%xdefine _EXTENDED_KEY_USAGE_free _ %+ BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_free
%xdefine _EXTENDED_KEY_USAGE_it _ %+ BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_it
%xdefine _EXTENDED_KEY_USAGE_new _ %+ BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_new
%xdefine _FIPS_is_entropy_cpu_jitter _ %+ BORINGSSL_PREFIX %+ _FIPS_is_entropy_cpu_jitter
%xdefine _FIPS_mode _ %+ BORINGSSL_PREFIX %+ _FIPS_mode
%xdefine _FIPS_mode_set _ %+ BORINGSSL_PREFIX %+ _FIPS_mode_set
%xdefine _FIPS_read_counter _ %+ BORINGSSL_PREFIX %+ _FIPS_read_counter
%xdefine _FIPS_service_indicator_after_call _ %+ BORINGSSL_PREFIX %+ _FIPS_service_indicator_after_call
%xdefine _FIPS_service_indicator_before_call _ %+ BORINGSSL_PREFIX %+ _FIPS_service_indicator_before_call
%xdefine _GENERAL_NAMES_free _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAMES_free
%xdefine _GENERAL_NAMES_it _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAMES_it
%xdefine _GENERAL_NAMES_new _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAMES_new
%xdefine _GENERAL_NAME_cmp _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_cmp
%xdefine _GENERAL_NAME_dup _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_dup
%xdefine _GENERAL_NAME_free _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_free
%xdefine _GENERAL_NAME_get0_otherName _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_get0_otherName
%xdefine _GENERAL_NAME_get0_value _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_get0_value
%xdefine _GENERAL_NAME_it _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_it
%xdefine _GENERAL_NAME_new _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_new
%xdefine _GENERAL_NAME_print _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_print
%xdefine _GENERAL_NAME_set0_othername _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_set0_othername
%xdefine _GENERAL_NAME_set0_value _ %+ BORINGSSL_PREFIX %+ _GENERAL_NAME_set0_value
%xdefine _GENERAL_SUBTREE_free _ %+ BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_free
%xdefine _GENERAL_SUBTREE_it _ %+ BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_it
%xdefine _GENERAL_SUBTREE_new _ %+ BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_new
%xdefine _HKDF _ %+ BORINGSSL_PREFIX %+ _HKDF
%xdefine _HKDF_expand _ %+ BORINGSSL_PREFIX %+ _HKDF_expand
%xdefine _HKDF_extract _ %+ BORINGSSL_PREFIX %+ _HKDF_extract
%xdefine _HMAC _ %+ BORINGSSL_PREFIX %+ _HMAC
%xdefine _HMAC_CTX_cleanse _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_cleanse
%xdefine _HMAC_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_cleanup
%xdefine _HMAC_CTX_copy _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_copy
%xdefine _HMAC_CTX_copy_ex _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_copy_ex
%xdefine _HMAC_CTX_free _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_free
%xdefine _HMAC_CTX_get_md _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_get_md
%xdefine _HMAC_CTX_init _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_init
%xdefine _HMAC_CTX_new _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_new
%xdefine _HMAC_CTX_reset _ %+ BORINGSSL_PREFIX %+ _HMAC_CTX_reset
%xdefine _HMAC_Final _ %+ BORINGSSL_PREFIX %+ _HMAC_Final
%xdefine _HMAC_Init _ %+ BORINGSSL_PREFIX %+ _HMAC_Init
%xdefine _HMAC_Init_ex _ %+ BORINGSSL_PREFIX %+ _HMAC_Init_ex
%xdefine _HMAC_Init_from_precomputed_key _ %+ BORINGSSL_PREFIX %+ _HMAC_Init_from_precomputed_key
%xdefine _HMAC_KEY_copy _ %+ BORINGSSL_PREFIX %+ _HMAC_KEY_copy
%xdefine _HMAC_KEY_new _ %+ BORINGSSL_PREFIX %+ _HMAC_KEY_new
%xdefine _HMAC_KEY_set _ %+ BORINGSSL_PREFIX %+ _HMAC_KEY_set
%xdefine _HMAC_Update _ %+ BORINGSSL_PREFIX %+ _HMAC_Update
%xdefine _HMAC_get_precomputed_key _ %+ BORINGSSL_PREFIX %+ _HMAC_get_precomputed_key
%xdefine _HMAC_set_precomputed_key_export _ %+ BORINGSSL_PREFIX %+ _HMAC_set_precomputed_key_export
%xdefine _HMAC_size _ %+ BORINGSSL_PREFIX %+ _HMAC_size
%xdefine _HMAC_with_precompute _ %+ BORINGSSL_PREFIX %+ _HMAC_with_precompute
%xdefine _HRSS_decap _ %+ BORINGSSL_PREFIX %+ _HRSS_decap
%xdefine _HRSS_encap _ %+ BORINGSSL_PREFIX %+ _HRSS_encap
%xdefine _HRSS_generate_key _ %+ BORINGSSL_PREFIX %+ _HRSS_generate_key
%xdefine _HRSS_marshal_public_key _ %+ BORINGSSL_PREFIX %+ _HRSS_marshal_public_key
%xdefine _HRSS_parse_public_key _ %+ BORINGSSL_PREFIX %+ _HRSS_parse_public_key
%xdefine _HRSS_poly3_invert _ %+ BORINGSSL_PREFIX %+ _HRSS_poly3_invert
%xdefine _HRSS_poly3_mul _ %+ BORINGSSL_PREFIX %+ _HRSS_poly3_mul
%xdefine _ISSUING_DIST_POINT_free _ %+ BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_free
%xdefine _ISSUING_DIST_POINT_it _ %+ BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_it
%xdefine _ISSUING_DIST_POINT_new _ %+ BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_new
%xdefine _KBKDF_ctr_hmac _ %+ BORINGSSL_PREFIX %+ _KBKDF_ctr_hmac
%xdefine _KEM_KEY_free _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_free
%xdefine _KEM_KEY_get0_kem _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_get0_kem
%xdefine _KEM_KEY_init _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_init
%xdefine _KEM_KEY_new _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_new
%xdefine _KEM_KEY_set_raw_key _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_key
%xdefine _KEM_KEY_set_raw_keypair_from_seed _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_keypair_from_seed
%xdefine _KEM_KEY_set_raw_public_key _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_public_key
%xdefine _KEM_KEY_set_raw_secret_key _ %+ BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_secret_key
%xdefine _KEM_find_asn1_by_nid _ %+ BORINGSSL_PREFIX %+ _KEM_find_asn1_by_nid
%xdefine _KEM_find_kem_by_nid _ %+ BORINGSSL_PREFIX %+ _KEM_find_kem_by_nid
%xdefine _Keccak1600_Absorb _ %+ BORINGSSL_PREFIX %+ _Keccak1600_Absorb
%xdefine _Keccak1600_Absorb_once_x4 _ %+ BORINGSSL_PREFIX %+ _Keccak1600_Absorb_once_x4
%xdefine _Keccak1600_Squeeze _ %+ BORINGSSL_PREFIX %+ _Keccak1600_Squeeze
%xdefine _Keccak1600_Squeezeblocks_x4 _ %+ BORINGSSL_PREFIX %+ _Keccak1600_Squeezeblocks_x4
%xdefine _KeccakF1600 _ %+ BORINGSSL_PREFIX %+ _KeccakF1600
%xdefine _KeccakF1600_hw _ %+ BORINGSSL_PREFIX %+ _KeccakF1600_hw
%xdefine _MD4 _ %+ BORINGSSL_PREFIX %+ _MD4
%xdefine _MD4_Final _ %+ BORINGSSL_PREFIX %+ _MD4_Final
%xdefine _MD4_Init _ %+ BORINGSSL_PREFIX %+ _MD4_Init
%xdefine _MD4_Transform _ %+ BORINGSSL_PREFIX %+ _MD4_Transform
%xdefine _MD4_Update _ %+ BORINGSSL_PREFIX %+ _MD4_Update
%xdefine _MD5 _ %+ BORINGSSL_PREFIX %+ _MD5
%xdefine _MD5_Final _ %+ BORINGSSL_PREFIX %+ _MD5_Final
%xdefine _MD5_Init _ %+ BORINGSSL_PREFIX %+ _MD5_Init
%xdefine _MD5_Init_from_state _ %+ BORINGSSL_PREFIX %+ _MD5_Init_from_state
%xdefine _MD5_Transform _ %+ BORINGSSL_PREFIX %+ _MD5_Transform
%xdefine _MD5_Update _ %+ BORINGSSL_PREFIX %+ _MD5_Update
%xdefine _MD5_get_state _ %+ BORINGSSL_PREFIX %+ _MD5_get_state
%xdefine _MGF1 _ %+ BORINGSSL_PREFIX %+ _MGF1
%xdefine _NAME_CONSTRAINTS_check _ %+ BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_check
%xdefine _NAME_CONSTRAINTS_check_CN _ %+ BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_check_CN
%xdefine _NAME_CONSTRAINTS_free _ %+ BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_free
%xdefine _NAME_CONSTRAINTS_it _ %+ BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_it
%xdefine _NAME_CONSTRAINTS_new _ %+ BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_new
%xdefine _NCONF_free _ %+ BORINGSSL_PREFIX %+ _NCONF_free
%xdefine _NCONF_get_section _ %+ BORINGSSL_PREFIX %+ _NCONF_get_section
%xdefine _NCONF_get_string _ %+ BORINGSSL_PREFIX %+ _NCONF_get_string
%xdefine _NCONF_load _ %+ BORINGSSL_PREFIX %+ _NCONF_load
%xdefine _NCONF_load_bio _ %+ BORINGSSL_PREFIX %+ _NCONF_load_bio
%xdefine _NCONF_new _ %+ BORINGSSL_PREFIX %+ _NCONF_new
%xdefine _NETSCAPE_SPKAC_free _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_free
%xdefine _NETSCAPE_SPKAC_it _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_it
%xdefine _NETSCAPE_SPKAC_new _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_new
%xdefine _NETSCAPE_SPKI_b64_decode _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_b64_decode
%xdefine _NETSCAPE_SPKI_b64_encode _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_b64_encode
%xdefine _NETSCAPE_SPKI_free _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_free
%xdefine _NETSCAPE_SPKI_get_pubkey _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_get_pubkey
%xdefine _NETSCAPE_SPKI_it _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_it
%xdefine _NETSCAPE_SPKI_new _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_new
%xdefine _NETSCAPE_SPKI_print _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_print
%xdefine _NETSCAPE_SPKI_set_pubkey _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_set_pubkey
%xdefine _NETSCAPE_SPKI_sign _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_sign
%xdefine _NETSCAPE_SPKI_verify _ %+ BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_verify
%xdefine _NOTICEREF_free _ %+ BORINGSSL_PREFIX %+ _NOTICEREF_free
%xdefine _NOTICEREF_it _ %+ BORINGSSL_PREFIX %+ _NOTICEREF_it
%xdefine _NOTICEREF_new _ %+ BORINGSSL_PREFIX %+ _NOTICEREF_new
%xdefine _OBJ_NAME_do_all_sorted _ %+ BORINGSSL_PREFIX %+ _OBJ_NAME_do_all_sorted
%xdefine _OBJ_cbs2nid _ %+ BORINGSSL_PREFIX %+ _OBJ_cbs2nid
%xdefine _OBJ_cleanup _ %+ BORINGSSL_PREFIX %+ _OBJ_cleanup
%xdefine _OBJ_cmp _ %+ BORINGSSL_PREFIX %+ _OBJ_cmp
%xdefine _OBJ_create _ %+ BORINGSSL_PREFIX %+ _OBJ_create
%xdefine _OBJ_dup _ %+ BORINGSSL_PREFIX %+ _OBJ_dup
%xdefine _OBJ_find_sigid_algs _ %+ BORINGSSL_PREFIX %+ _OBJ_find_sigid_algs
%xdefine _OBJ_find_sigid_by_algs _ %+ BORINGSSL_PREFIX %+ _OBJ_find_sigid_by_algs
%xdefine _OBJ_get0_data _ %+ BORINGSSL_PREFIX %+ _OBJ_get0_data
%xdefine _OBJ_get_undef _ %+ BORINGSSL_PREFIX %+ _OBJ_get_undef
%xdefine _OBJ_length _ %+ BORINGSSL_PREFIX %+ _OBJ_length
%xdefine _OBJ_ln2nid _ %+ BORINGSSL_PREFIX %+ _OBJ_ln2nid
%xdefine _OBJ_nid2cbb _ %+ BORINGSSL_PREFIX %+ _OBJ_nid2cbb
%xdefine _OBJ_nid2ln _ %+ BORINGSSL_PREFIX %+ _OBJ_nid2ln
%xdefine _OBJ_nid2obj _ %+ BORINGSSL_PREFIX %+ _OBJ_nid2obj
%xdefine _OBJ_nid2sn _ %+ BORINGSSL_PREFIX %+ _OBJ_nid2sn
%xdefine _OBJ_obj2nid _ %+ BORINGSSL_PREFIX %+ _OBJ_obj2nid
%xdefine _OBJ_obj2txt _ %+ BORINGSSL_PREFIX %+ _OBJ_obj2txt
%xdefine _OBJ_sn2nid _ %+ BORINGSSL_PREFIX %+ _OBJ_sn2nid
%xdefine _OBJ_txt2nid _ %+ BORINGSSL_PREFIX %+ _OBJ_txt2nid
%xdefine _OBJ_txt2obj _ %+ BORINGSSL_PREFIX %+ _OBJ_txt2obj
%xdefine _OCSP_BASICRESP_add_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_add_ext
%xdefine _OCSP_BASICRESP_delete_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_delete_ext
%xdefine _OCSP_BASICRESP_free _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_free
%xdefine _OCSP_BASICRESP_get_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_get_ext
%xdefine _OCSP_BASICRESP_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_get_ext_by_NID
%xdefine _OCSP_BASICRESP_it _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_it
%xdefine _OCSP_BASICRESP_new _ %+ BORINGSSL_PREFIX %+ _OCSP_BASICRESP_new
%xdefine _OCSP_CERTID_dup _ %+ BORINGSSL_PREFIX %+ _OCSP_CERTID_dup
%xdefine _OCSP_CERTID_free _ %+ BORINGSSL_PREFIX %+ _OCSP_CERTID_free
%xdefine _OCSP_CERTID_it _ %+ BORINGSSL_PREFIX %+ _OCSP_CERTID_it
%xdefine _OCSP_CERTID_new _ %+ BORINGSSL_PREFIX %+ _OCSP_CERTID_new
%xdefine _OCSP_CERTSTATUS_it _ %+ BORINGSSL_PREFIX %+ _OCSP_CERTSTATUS_it
%xdefine _OCSP_ONEREQ_free _ %+ BORINGSSL_PREFIX %+ _OCSP_ONEREQ_free
%xdefine _OCSP_ONEREQ_it _ %+ BORINGSSL_PREFIX %+ _OCSP_ONEREQ_it
%xdefine _OCSP_ONEREQ_new _ %+ BORINGSSL_PREFIX %+ _OCSP_ONEREQ_new
%xdefine _OCSP_REQINFO_free _ %+ BORINGSSL_PREFIX %+ _OCSP_REQINFO_free
%xdefine _OCSP_REQINFO_it _ %+ BORINGSSL_PREFIX %+ _OCSP_REQINFO_it
%xdefine _OCSP_REQINFO_new _ %+ BORINGSSL_PREFIX %+ _OCSP_REQINFO_new
%xdefine _OCSP_REQUEST_free _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_free
%xdefine _OCSP_REQUEST_get_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_get_ext
%xdefine _OCSP_REQUEST_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_get_ext_by_NID
%xdefine _OCSP_REQUEST_it _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_it
%xdefine _OCSP_REQUEST_new _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_new
%xdefine _OCSP_REQUEST_print _ %+ BORINGSSL_PREFIX %+ _OCSP_REQUEST_print
%xdefine _OCSP_REQ_CTX_add1_header _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_add1_header
%xdefine _OCSP_REQ_CTX_free _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_free
%xdefine _OCSP_REQ_CTX_get0_mem_bio _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_get0_mem_bio
%xdefine _OCSP_REQ_CTX_http _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_http
%xdefine _OCSP_REQ_CTX_i2d _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_i2d
%xdefine _OCSP_REQ_CTX_nbio _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_nbio
%xdefine _OCSP_REQ_CTX_nbio_d2i _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_nbio_d2i
%xdefine _OCSP_REQ_CTX_new _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_new
%xdefine _OCSP_REQ_CTX_set1_req _ %+ BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_set1_req
%xdefine _OCSP_RESPBYTES_free _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_free
%xdefine _OCSP_RESPBYTES_it _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_it
%xdefine _OCSP_RESPBYTES_new _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_new
%xdefine _OCSP_RESPDATA_free _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPDATA_free
%xdefine _OCSP_RESPDATA_it _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPDATA_it
%xdefine _OCSP_RESPDATA_new _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPDATA_new
%xdefine _OCSP_RESPID_it _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPID_it
%xdefine _OCSP_RESPONSE_free _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPONSE_free
%xdefine _OCSP_RESPONSE_it _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPONSE_it
%xdefine _OCSP_RESPONSE_new _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPONSE_new
%xdefine _OCSP_RESPONSE_print _ %+ BORINGSSL_PREFIX %+ _OCSP_RESPONSE_print
%xdefine _OCSP_REVOKEDINFO_free _ %+ BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_free
%xdefine _OCSP_REVOKEDINFO_it _ %+ BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_it
%xdefine _OCSP_REVOKEDINFO_new _ %+ BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_new
%xdefine _OCSP_SIGNATURE_free _ %+ BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_free
%xdefine _OCSP_SIGNATURE_it _ %+ BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_it
%xdefine _OCSP_SIGNATURE_new _ %+ BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_new
%xdefine _OCSP_SINGLERESP_add_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_add_ext
%xdefine _OCSP_SINGLERESP_free _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_free
%xdefine _OCSP_SINGLERESP_get0_id _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get0_id
%xdefine _OCSP_SINGLERESP_get_ext _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get_ext
%xdefine _OCSP_SINGLERESP_get_ext_count _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get_ext_count
%xdefine _OCSP_SINGLERESP_it _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_it
%xdefine _OCSP_SINGLERESP_new _ %+ BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_new
%xdefine _OCSP_basic_add1_cert _ %+ BORINGSSL_PREFIX %+ _OCSP_basic_add1_cert
%xdefine _OCSP_basic_add1_nonce _ %+ BORINGSSL_PREFIX %+ _OCSP_basic_add1_nonce
%xdefine _OCSP_basic_add1_status _ %+ BORINGSSL_PREFIX %+ _OCSP_basic_add1_status
%xdefine _OCSP_basic_sign _ %+ BORINGSSL_PREFIX %+ _OCSP_basic_sign
%xdefine _OCSP_basic_verify _ %+ BORINGSSL_PREFIX %+ _OCSP_basic_verify
%xdefine _OCSP_cert_id_new _ %+ BORINGSSL_PREFIX %+ _OCSP_cert_id_new
%xdefine _OCSP_cert_status_str _ %+ BORINGSSL_PREFIX %+ _OCSP_cert_status_str
%xdefine _OCSP_cert_to_id _ %+ BORINGSSL_PREFIX %+ _OCSP_cert_to_id
%xdefine _OCSP_check_nonce _ %+ BORINGSSL_PREFIX %+ _OCSP_check_nonce
%xdefine _OCSP_check_validity _ %+ BORINGSSL_PREFIX %+ _OCSP_check_validity
%xdefine _OCSP_copy_nonce _ %+ BORINGSSL_PREFIX %+ _OCSP_copy_nonce
%xdefine _OCSP_crl_reason_str _ %+ BORINGSSL_PREFIX %+ _OCSP_crl_reason_str
%xdefine _OCSP_get_default_digest _ %+ BORINGSSL_PREFIX %+ _OCSP_get_default_digest
%xdefine _OCSP_id_cmp _ %+ BORINGSSL_PREFIX %+ _OCSP_id_cmp
%xdefine _OCSP_id_get0_info _ %+ BORINGSSL_PREFIX %+ _OCSP_id_get0_info
%xdefine _OCSP_id_issuer_cmp _ %+ BORINGSSL_PREFIX %+ _OCSP_id_issuer_cmp
%xdefine _OCSP_onereq_get0_id _ %+ BORINGSSL_PREFIX %+ _OCSP_onereq_get0_id
%xdefine _OCSP_parse_url _ %+ BORINGSSL_PREFIX %+ _OCSP_parse_url
%xdefine _OCSP_request_add0_id _ %+ BORINGSSL_PREFIX %+ _OCSP_request_add0_id
%xdefine _OCSP_request_add1_cert _ %+ BORINGSSL_PREFIX %+ _OCSP_request_add1_cert
%xdefine _OCSP_request_add1_nonce _ %+ BORINGSSL_PREFIX %+ _OCSP_request_add1_nonce
%xdefine _OCSP_request_is_signed _ %+ BORINGSSL_PREFIX %+ _OCSP_request_is_signed
%xdefine _OCSP_request_onereq_count _ %+ BORINGSSL_PREFIX %+ _OCSP_request_onereq_count
%xdefine _OCSP_request_onereq_get0 _ %+ BORINGSSL_PREFIX %+ _OCSP_request_onereq_get0
%xdefine _OCSP_request_set1_name _ %+ BORINGSSL_PREFIX %+ _OCSP_request_set1_name
%xdefine _OCSP_request_sign _ %+ BORINGSSL_PREFIX %+ _OCSP_request_sign
%xdefine _OCSP_request_verify _ %+ BORINGSSL_PREFIX %+ _OCSP_request_verify
%xdefine _OCSP_resp_count _ %+ BORINGSSL_PREFIX %+ _OCSP_resp_count
%xdefine _OCSP_resp_find _ %+ BORINGSSL_PREFIX %+ _OCSP_resp_find
%xdefine _OCSP_resp_find_status _ %+ BORINGSSL_PREFIX %+ _OCSP_resp_find_status
%xdefine _OCSP_resp_get0 _ %+ BORINGSSL_PREFIX %+ _OCSP_resp_get0
%xdefine _OCSP_response_create _ %+ BORINGSSL_PREFIX %+ _OCSP_response_create
%xdefine _OCSP_response_get1_basic _ %+ BORINGSSL_PREFIX %+ _OCSP_response_get1_basic
%xdefine _OCSP_response_status _ %+ BORINGSSL_PREFIX %+ _OCSP_response_status
%xdefine _OCSP_response_status_str _ %+ BORINGSSL_PREFIX %+ _OCSP_response_status_str
%xdefine _OCSP_sendreq_bio _ %+ BORINGSSL_PREFIX %+ _OCSP_sendreq_bio
%xdefine _OCSP_sendreq_nbio _ %+ BORINGSSL_PREFIX %+ _OCSP_sendreq_nbio
%xdefine _OCSP_sendreq_new _ %+ BORINGSSL_PREFIX %+ _OCSP_sendreq_new
%xdefine _OCSP_set_max_response_length _ %+ BORINGSSL_PREFIX %+ _OCSP_set_max_response_length
%xdefine _OCSP_single_get0_status _ %+ BORINGSSL_PREFIX %+ _OCSP_single_get0_status
%xdefine _OPENSSL_add_all_algorithms_conf _ %+ BORINGSSL_PREFIX %+ _OPENSSL_add_all_algorithms_conf
%xdefine _OPENSSL_armcap_P _ %+ BORINGSSL_PREFIX %+ _OPENSSL_armcap_P
%xdefine _OPENSSL_asprintf _ %+ BORINGSSL_PREFIX %+ _OPENSSL_asprintf
%xdefine _OPENSSL_calloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_calloc
%xdefine _OPENSSL_cleanse _ %+ BORINGSSL_PREFIX %+ _OPENSSL_cleanse
%xdefine _OPENSSL_cleanup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_cleanup
%xdefine _OPENSSL_clear_free _ %+ BORINGSSL_PREFIX %+ _OPENSSL_clear_free
%xdefine _OPENSSL_config _ %+ BORINGSSL_PREFIX %+ _OPENSSL_config
%xdefine _OPENSSL_cpucap_initialized _ %+ BORINGSSL_PREFIX %+ _OPENSSL_cpucap_initialized
%xdefine _OPENSSL_cpuid_setup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_cpuid_setup
%xdefine _OPENSSL_free _ %+ BORINGSSL_PREFIX %+ _OPENSSL_free
%xdefine _OPENSSL_fromxdigit _ %+ BORINGSSL_PREFIX %+ _OPENSSL_fromxdigit
%xdefine _OPENSSL_gmtime _ %+ BORINGSSL_PREFIX %+ _OPENSSL_gmtime
%xdefine _OPENSSL_gmtime_adj _ %+ BORINGSSL_PREFIX %+ _OPENSSL_gmtime_adj
%xdefine _OPENSSL_gmtime_diff _ %+ BORINGSSL_PREFIX %+ _OPENSSL_gmtime_diff
%xdefine _OPENSSL_hash32 _ %+ BORINGSSL_PREFIX %+ _OPENSSL_hash32
%xdefine _OPENSSL_hexstr2buf _ %+ BORINGSSL_PREFIX %+ _OPENSSL_hexstr2buf
%xdefine _OPENSSL_ia32cap_P _ %+ BORINGSSL_PREFIX %+ _OPENSSL_ia32cap_P
%xdefine _OPENSSL_init _ %+ BORINGSSL_PREFIX %+ _OPENSSL_init
%xdefine _OPENSSL_init_crypto _ %+ BORINGSSL_PREFIX %+ _OPENSSL_init_crypto
%xdefine _OPENSSL_isalnum _ %+ BORINGSSL_PREFIX %+ _OPENSSL_isalnum
%xdefine _OPENSSL_isalpha _ %+ BORINGSSL_PREFIX %+ _OPENSSL_isalpha
%xdefine _OPENSSL_isdigit _ %+ BORINGSSL_PREFIX %+ _OPENSSL_isdigit
%xdefine _OPENSSL_isspace _ %+ BORINGSSL_PREFIX %+ _OPENSSL_isspace
%xdefine _OPENSSL_isxdigit _ %+ BORINGSSL_PREFIX %+ _OPENSSL_isxdigit
%xdefine _OPENSSL_lh_delete _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_delete
%xdefine _OPENSSL_lh_doall_arg _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_doall_arg
%xdefine _OPENSSL_lh_free _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_free
%xdefine _OPENSSL_lh_insert _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_insert
%xdefine _OPENSSL_lh_new _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_new
%xdefine _OPENSSL_lh_num_items _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_num_items
%xdefine _OPENSSL_lh_retrieve _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_retrieve
%xdefine _OPENSSL_lh_retrieve_key _ %+ BORINGSSL_PREFIX %+ _OPENSSL_lh_retrieve_key
%xdefine _OPENSSL_load_builtin_modules _ %+ BORINGSSL_PREFIX %+ _OPENSSL_load_builtin_modules
%xdefine _OPENSSL_malloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_malloc
%xdefine _OPENSSL_malloc_init _ %+ BORINGSSL_PREFIX %+ _OPENSSL_malloc_init
%xdefine _OPENSSL_memdup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_memdup
%xdefine _OPENSSL_no_config _ %+ BORINGSSL_PREFIX %+ _OPENSSL_no_config
%xdefine _OPENSSL_posix_to_tm _ %+ BORINGSSL_PREFIX %+ _OPENSSL_posix_to_tm
%xdefine _OPENSSL_ppc64le_hwcap2 _ %+ BORINGSSL_PREFIX %+ _OPENSSL_ppc64le_hwcap2
%xdefine _OPENSSL_realloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_realloc
%xdefine _OPENSSL_secure_clear_free _ %+ BORINGSSL_PREFIX %+ _OPENSSL_secure_clear_free
%xdefine _OPENSSL_secure_malloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_secure_malloc
%xdefine _OPENSSL_secure_zalloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_secure_zalloc
%xdefine _OPENSSL_sk_deep_copy _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_deep_copy
%xdefine _OPENSSL_sk_delete _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_delete
%xdefine _OPENSSL_sk_delete_if _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_delete_if
%xdefine _OPENSSL_sk_delete_ptr _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_delete_ptr
%xdefine _OPENSSL_sk_dup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_dup
%xdefine _OPENSSL_sk_find _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_find
%xdefine _OPENSSL_sk_free _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_free
%xdefine _OPENSSL_sk_insert _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_insert
%xdefine _OPENSSL_sk_is_sorted _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_is_sorted
%xdefine _OPENSSL_sk_new _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_new
%xdefine _OPENSSL_sk_new_null _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_new_null
%xdefine _OPENSSL_sk_num _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_num
%xdefine _OPENSSL_sk_pop _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_pop
%xdefine _OPENSSL_sk_pop_free_ex _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_pop_free_ex
%xdefine _OPENSSL_sk_push _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_push
%xdefine _OPENSSL_sk_set _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_set
%xdefine _OPENSSL_sk_set_cmp_func _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_set_cmp_func
%xdefine _OPENSSL_sk_shift _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_shift
%xdefine _OPENSSL_sk_sort _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_sort
%xdefine _OPENSSL_sk_unshift _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_unshift
%xdefine _OPENSSL_sk_value _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_value
%xdefine _OPENSSL_sk_zero _ %+ BORINGSSL_PREFIX %+ _OPENSSL_sk_zero
%xdefine _OPENSSL_strcasecmp _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strcasecmp
%xdefine _OPENSSL_strdup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strdup
%xdefine _OPENSSL_strhash _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strhash
%xdefine _OPENSSL_strlcat _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strlcat
%xdefine _OPENSSL_strlcpy _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strlcpy
%xdefine _OPENSSL_strncasecmp _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strncasecmp
%xdefine _OPENSSL_strndup _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strndup
%xdefine _OPENSSL_strnlen _ %+ BORINGSSL_PREFIX %+ _OPENSSL_strnlen
%xdefine _OPENSSL_timegm _ %+ BORINGSSL_PREFIX %+ _OPENSSL_timegm
%xdefine _OPENSSL_tm_to_posix _ %+ BORINGSSL_PREFIX %+ _OPENSSL_tm_to_posix
%xdefine _OPENSSL_tolower _ %+ BORINGSSL_PREFIX %+ _OPENSSL_tolower
%xdefine _OPENSSL_vasprintf _ %+ BORINGSSL_PREFIX %+ _OPENSSL_vasprintf
%xdefine _OPENSSL_vasprintf_internal _ %+ BORINGSSL_PREFIX %+ _OPENSSL_vasprintf_internal
%xdefine _OPENSSL_zalloc _ %+ BORINGSSL_PREFIX %+ _OPENSSL_zalloc
%xdefine _OTHERNAME_free _ %+ BORINGSSL_PREFIX %+ _OTHERNAME_free
%xdefine _OTHERNAME_it _ %+ BORINGSSL_PREFIX %+ _OTHERNAME_it
%xdefine _OTHERNAME_new _ %+ BORINGSSL_PREFIX %+ _OTHERNAME_new
%xdefine _OpenSSL_add_all_algorithms _ %+ BORINGSSL_PREFIX %+ _OpenSSL_add_all_algorithms
%xdefine _OpenSSL_add_all_ciphers _ %+ BORINGSSL_PREFIX %+ _OpenSSL_add_all_ciphers
%xdefine _OpenSSL_add_all_digests _ %+ BORINGSSL_PREFIX %+ _OpenSSL_add_all_digests
%xdefine _OpenSSL_version _ %+ BORINGSSL_PREFIX %+ _OpenSSL_version
%xdefine _OpenSSL_version_num _ %+ BORINGSSL_PREFIX %+ _OpenSSL_version_num
%xdefine _PEM_ASN1_read _ %+ BORINGSSL_PREFIX %+ _PEM_ASN1_read
%xdefine _PEM_ASN1_read_bio _ %+ BORINGSSL_PREFIX %+ _PEM_ASN1_read_bio
%xdefine _PEM_ASN1_write _ %+ BORINGSSL_PREFIX %+ _PEM_ASN1_write
%xdefine _PEM_ASN1_write_bio _ %+ BORINGSSL_PREFIX %+ _PEM_ASN1_write_bio
%xdefine _PEM_X509_INFO_read _ %+ BORINGSSL_PREFIX %+ _PEM_X509_INFO_read
%xdefine _PEM_X509_INFO_read_bio _ %+ BORINGSSL_PREFIX %+ _PEM_X509_INFO_read_bio
%xdefine _PEM_X509_INFO_write_bio _ %+ BORINGSSL_PREFIX %+ _PEM_X509_INFO_write_bio
%xdefine _PEM_bytes_read_bio _ %+ BORINGSSL_PREFIX %+ _PEM_bytes_read_bio
%xdefine _PEM_def_callback _ %+ BORINGSSL_PREFIX %+ _PEM_def_callback
%xdefine _PEM_dek_info _ %+ BORINGSSL_PREFIX %+ _PEM_dek_info
%xdefine _PEM_do_header _ %+ BORINGSSL_PREFIX %+ _PEM_do_header
%xdefine _PEM_get_EVP_CIPHER_INFO _ %+ BORINGSSL_PREFIX %+ _PEM_get_EVP_CIPHER_INFO
%xdefine _PEM_proc_type _ %+ BORINGSSL_PREFIX %+ _PEM_proc_type
%xdefine _PEM_read _ %+ BORINGSSL_PREFIX %+ _PEM_read
%xdefine _PEM_read_DHparams _ %+ BORINGSSL_PREFIX %+ _PEM_read_DHparams
%xdefine _PEM_read_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_DSAPrivateKey
%xdefine _PEM_read_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_DSA_PUBKEY
%xdefine _PEM_read_DSAparams _ %+ BORINGSSL_PREFIX %+ _PEM_read_DSAparams
%xdefine _PEM_read_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_ECPrivateKey
%xdefine _PEM_read_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_EC_PUBKEY
%xdefine _PEM_read_PKCS7 _ %+ BORINGSSL_PREFIX %+ _PEM_read_PKCS7
%xdefine _PEM_read_PKCS8 _ %+ BORINGSSL_PREFIX %+ _PEM_read_PKCS8
%xdefine _PEM_read_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _PEM_read_PKCS8_PRIV_KEY_INFO
%xdefine _PEM_read_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_PUBKEY
%xdefine _PEM_read_PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_PrivateKey
%xdefine _PEM_read_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_RSAPrivateKey
%xdefine _PEM_read_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_RSAPublicKey
%xdefine _PEM_read_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_RSA_PUBKEY
%xdefine _PEM_read_X509 _ %+ BORINGSSL_PREFIX %+ _PEM_read_X509
%xdefine _PEM_read_X509_AUX _ %+ BORINGSSL_PREFIX %+ _PEM_read_X509_AUX
%xdefine _PEM_read_X509_CRL _ %+ BORINGSSL_PREFIX %+ _PEM_read_X509_CRL
%xdefine _PEM_read_X509_REQ _ %+ BORINGSSL_PREFIX %+ _PEM_read_X509_REQ
%xdefine _PEM_read_bio _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio
%xdefine _PEM_read_bio_DHparams _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_DHparams
%xdefine _PEM_read_bio_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_DSAPrivateKey
%xdefine _PEM_read_bio_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_DSA_PUBKEY
%xdefine _PEM_read_bio_DSAparams _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_DSAparams
%xdefine _PEM_read_bio_ECPKParameters _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_ECPKParameters
%xdefine _PEM_read_bio_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_ECPrivateKey
%xdefine _PEM_read_bio_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_EC_PUBKEY
%xdefine _PEM_read_bio_PKCS7 _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS7
%xdefine _PEM_read_bio_PKCS8 _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS8
%xdefine _PEM_read_bio_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS8_PRIV_KEY_INFO
%xdefine _PEM_read_bio_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_PUBKEY
%xdefine _PEM_read_bio_Parameters _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_Parameters
%xdefine _PEM_read_bio_PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_PrivateKey
%xdefine _PEM_read_bio_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_RSAPrivateKey
%xdefine _PEM_read_bio_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_RSAPublicKey
%xdefine _PEM_read_bio_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_RSA_PUBKEY
%xdefine _PEM_read_bio_X509 _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_X509
%xdefine _PEM_read_bio_X509_AUX _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_X509_AUX
%xdefine _PEM_read_bio_X509_CRL _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_X509_CRL
%xdefine _PEM_read_bio_X509_REQ _ %+ BORINGSSL_PREFIX %+ _PEM_read_bio_X509_REQ
%xdefine _PEM_write _ %+ BORINGSSL_PREFIX %+ _PEM_write
%xdefine _PEM_write_DHparams _ %+ BORINGSSL_PREFIX %+ _PEM_write_DHparams
%xdefine _PEM_write_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_DSAPrivateKey
%xdefine _PEM_write_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_DSA_PUBKEY
%xdefine _PEM_write_DSAparams _ %+ BORINGSSL_PREFIX %+ _PEM_write_DSAparams
%xdefine _PEM_write_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_ECPrivateKey
%xdefine _PEM_write_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_EC_PUBKEY
%xdefine _PEM_write_PKCS7 _ %+ BORINGSSL_PREFIX %+ _PEM_write_PKCS7
%xdefine _PEM_write_PKCS8 _ %+ BORINGSSL_PREFIX %+ _PEM_write_PKCS8
%xdefine _PEM_write_PKCS8PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_PKCS8PrivateKey
%xdefine _PEM_write_PKCS8PrivateKey_nid _ %+ BORINGSSL_PREFIX %+ _PEM_write_PKCS8PrivateKey_nid
%xdefine _PEM_write_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _PEM_write_PKCS8_PRIV_KEY_INFO
%xdefine _PEM_write_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_PUBKEY
%xdefine _PEM_write_PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_PrivateKey
%xdefine _PEM_write_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_RSAPrivateKey
%xdefine _PEM_write_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_RSAPublicKey
%xdefine _PEM_write_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_RSA_PUBKEY
%xdefine _PEM_write_X509 _ %+ BORINGSSL_PREFIX %+ _PEM_write_X509
%xdefine _PEM_write_X509_AUX _ %+ BORINGSSL_PREFIX %+ _PEM_write_X509_AUX
%xdefine _PEM_write_X509_CRL _ %+ BORINGSSL_PREFIX %+ _PEM_write_X509_CRL
%xdefine _PEM_write_X509_REQ _ %+ BORINGSSL_PREFIX %+ _PEM_write_X509_REQ
%xdefine _PEM_write_X509_REQ_NEW _ %+ BORINGSSL_PREFIX %+ _PEM_write_X509_REQ_NEW
%xdefine _PEM_write_bio _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio
%xdefine _PEM_write_bio_DHparams _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_DHparams
%xdefine _PEM_write_bio_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_DSAPrivateKey
%xdefine _PEM_write_bio_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_DSA_PUBKEY
%xdefine _PEM_write_bio_DSAparams _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_DSAparams
%xdefine _PEM_write_bio_ECPKParameters _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_ECPKParameters
%xdefine _PEM_write_bio_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_ECPrivateKey
%xdefine _PEM_write_bio_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_EC_PUBKEY
%xdefine _PEM_write_bio_PKCS7 _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS7
%xdefine _PEM_write_bio_PKCS8 _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8
%xdefine _PEM_write_bio_PKCS8PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8PrivateKey
%xdefine _PEM_write_bio_PKCS8PrivateKey_nid _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8PrivateKey_nid
%xdefine _PEM_write_bio_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8_PRIV_KEY_INFO
%xdefine _PEM_write_bio_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PUBKEY
%xdefine _PEM_write_bio_Parameters _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_Parameters
%xdefine _PEM_write_bio_PrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PrivateKey
%xdefine _PEM_write_bio_PrivateKey_traditional _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_PrivateKey_traditional
%xdefine _PEM_write_bio_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_RSAPrivateKey
%xdefine _PEM_write_bio_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_RSAPublicKey
%xdefine _PEM_write_bio_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_RSA_PUBKEY
%xdefine _PEM_write_bio_X509 _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_X509
%xdefine _PEM_write_bio_X509_AUX _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_X509_AUX
%xdefine _PEM_write_bio_X509_CRL _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_X509_CRL
%xdefine _PEM_write_bio_X509_REQ _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_X509_REQ
%xdefine _PEM_write_bio_X509_REQ_NEW _ %+ BORINGSSL_PREFIX %+ _PEM_write_bio_X509_REQ_NEW
%xdefine _PKCS12_PBE_add _ %+ BORINGSSL_PREFIX %+ _PKCS12_PBE_add
%xdefine _PKCS12_create _ %+ BORINGSSL_PREFIX %+ _PKCS12_create
%xdefine _PKCS12_free _ %+ BORINGSSL_PREFIX %+ _PKCS12_free
%xdefine _PKCS12_get_key_and_certs _ %+ BORINGSSL_PREFIX %+ _PKCS12_get_key_and_certs
%xdefine _PKCS12_new _ %+ BORINGSSL_PREFIX %+ _PKCS12_new
%xdefine _PKCS12_parse _ %+ BORINGSSL_PREFIX %+ _PKCS12_parse
%xdefine _PKCS12_set_mac _ %+ BORINGSSL_PREFIX %+ _PKCS12_set_mac
%xdefine _PKCS12_verify_mac _ %+ BORINGSSL_PREFIX %+ _PKCS12_verify_mac
%xdefine _PKCS1_MGF1 _ %+ BORINGSSL_PREFIX %+ _PKCS1_MGF1
%xdefine _PKCS5_PBKDF2_HMAC _ %+ BORINGSSL_PREFIX %+ _PKCS5_PBKDF2_HMAC
%xdefine _PKCS5_PBKDF2_HMAC_SHA1 _ %+ BORINGSSL_PREFIX %+ _PKCS5_PBKDF2_HMAC_SHA1
%xdefine _PKCS5_pbe2_decrypt_init _ %+ BORINGSSL_PREFIX %+ _PKCS5_pbe2_decrypt_init
%xdefine _PKCS5_pbe2_encrypt_init _ %+ BORINGSSL_PREFIX %+ _PKCS5_pbe2_encrypt_init
%xdefine _PKCS7_ATTR_VERIFY_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_ATTR_VERIFY_it
%xdefine _PKCS7_DIGEST_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_DIGEST_free
%xdefine _PKCS7_DIGEST_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_DIGEST_it
%xdefine _PKCS7_DIGEST_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_DIGEST_new
%xdefine _PKCS7_ENCRYPT_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_free
%xdefine _PKCS7_ENCRYPT_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_it
%xdefine _PKCS7_ENCRYPT_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_new
%xdefine _PKCS7_ENC_CONTENT_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_free
%xdefine _PKCS7_ENC_CONTENT_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_it
%xdefine _PKCS7_ENC_CONTENT_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_new
%xdefine _PKCS7_ENVELOPE_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_free
%xdefine _PKCS7_ENVELOPE_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_it
%xdefine _PKCS7_ENVELOPE_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_new
%xdefine _PKCS7_ISSUER_AND_SERIAL_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_free
%xdefine _PKCS7_ISSUER_AND_SERIAL_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_it
%xdefine _PKCS7_ISSUER_AND_SERIAL_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_new
%xdefine _PKCS7_RECIP_INFO_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_free
%xdefine _PKCS7_RECIP_INFO_get0_alg _ %+ BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_get0_alg
%xdefine _PKCS7_RECIP_INFO_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_it
%xdefine _PKCS7_RECIP_INFO_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_new
%xdefine _PKCS7_RECIP_INFO_set _ %+ BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_set
%xdefine _PKCS7_SIGNED_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNED_free
%xdefine _PKCS7_SIGNED_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNED_it
%xdefine _PKCS7_SIGNED_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNED_new
%xdefine _PKCS7_SIGNER_INFO_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_free
%xdefine _PKCS7_SIGNER_INFO_get0_algs _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_get0_algs
%xdefine _PKCS7_SIGNER_INFO_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_it
%xdefine _PKCS7_SIGNER_INFO_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_new
%xdefine _PKCS7_SIGNER_INFO_set _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_set
%xdefine _PKCS7_SIGN_ENVELOPE_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_free
%xdefine _PKCS7_SIGN_ENVELOPE_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_it
%xdefine _PKCS7_SIGN_ENVELOPE_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_new
%xdefine _PKCS7_add_certificate _ %+ BORINGSSL_PREFIX %+ _PKCS7_add_certificate
%xdefine _PKCS7_add_crl _ %+ BORINGSSL_PREFIX %+ _PKCS7_add_crl
%xdefine _PKCS7_add_recipient _ %+ BORINGSSL_PREFIX %+ _PKCS7_add_recipient
%xdefine _PKCS7_add_recipient_info _ %+ BORINGSSL_PREFIX %+ _PKCS7_add_recipient_info
%xdefine _PKCS7_add_signer _ %+ BORINGSSL_PREFIX %+ _PKCS7_add_signer
%xdefine _PKCS7_bundle_CRLs _ %+ BORINGSSL_PREFIX %+ _PKCS7_bundle_CRLs
%xdefine _PKCS7_bundle_certificates _ %+ BORINGSSL_PREFIX %+ _PKCS7_bundle_certificates
%xdefine _PKCS7_bundle_raw_certificates _ %+ BORINGSSL_PREFIX %+ _PKCS7_bundle_raw_certificates
%xdefine _PKCS7_content_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_content_new
%xdefine _PKCS7_dataFinal _ %+ BORINGSSL_PREFIX %+ _PKCS7_dataFinal
%xdefine _PKCS7_dataInit _ %+ BORINGSSL_PREFIX %+ _PKCS7_dataInit
%xdefine _PKCS7_decrypt _ %+ BORINGSSL_PREFIX %+ _PKCS7_decrypt
%xdefine _PKCS7_dup _ %+ BORINGSSL_PREFIX %+ _PKCS7_dup
%xdefine _PKCS7_encrypt _ %+ BORINGSSL_PREFIX %+ _PKCS7_encrypt
%xdefine _PKCS7_free _ %+ BORINGSSL_PREFIX %+ _PKCS7_free
%xdefine _PKCS7_get0_signers _ %+ BORINGSSL_PREFIX %+ _PKCS7_get0_signers
%xdefine _PKCS7_get_CRLs _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_CRLs
%xdefine _PKCS7_get_PEM_CRLs _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_PEM_CRLs
%xdefine _PKCS7_get_PEM_certificates _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_PEM_certificates
%xdefine _PKCS7_get_certificates _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_certificates
%xdefine _PKCS7_get_detached _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_detached
%xdefine _PKCS7_get_raw_certificates _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_raw_certificates
%xdefine _PKCS7_get_recipient_info _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_recipient_info
%xdefine _PKCS7_get_signed_attribute _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_signed_attribute
%xdefine _PKCS7_get_signer_info _ %+ BORINGSSL_PREFIX %+ _PKCS7_get_signer_info
%xdefine _PKCS7_is_detached _ %+ BORINGSSL_PREFIX %+ _PKCS7_is_detached
%xdefine _PKCS7_it _ %+ BORINGSSL_PREFIX %+ _PKCS7_it
%xdefine _PKCS7_new _ %+ BORINGSSL_PREFIX %+ _PKCS7_new
%xdefine _PKCS7_print_ctx _ %+ BORINGSSL_PREFIX %+ _PKCS7_print_ctx
%xdefine _PKCS7_set_cipher _ %+ BORINGSSL_PREFIX %+ _PKCS7_set_cipher
%xdefine _PKCS7_set_content _ %+ BORINGSSL_PREFIX %+ _PKCS7_set_content
%xdefine _PKCS7_set_detached _ %+ BORINGSSL_PREFIX %+ _PKCS7_set_detached
%xdefine _PKCS7_set_digest _ %+ BORINGSSL_PREFIX %+ _PKCS7_set_digest
%xdefine _PKCS7_set_type _ %+ BORINGSSL_PREFIX %+ _PKCS7_set_type
%xdefine _PKCS7_sign _ %+ BORINGSSL_PREFIX %+ _PKCS7_sign
%xdefine _PKCS7_type_is_data _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_data
%xdefine _PKCS7_type_is_digest _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_digest
%xdefine _PKCS7_type_is_encrypted _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_encrypted
%xdefine _PKCS7_type_is_enveloped _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_enveloped
%xdefine _PKCS7_type_is_signed _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_signed
%xdefine _PKCS7_type_is_signedAndEnveloped _ %+ BORINGSSL_PREFIX %+ _PKCS7_type_is_signedAndEnveloped
%xdefine _PKCS7_verify _ %+ BORINGSSL_PREFIX %+ _PKCS7_verify
%xdefine _PKCS8_PRIV_KEY_INFO_free _ %+ BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_free
%xdefine _PKCS8_PRIV_KEY_INFO_it _ %+ BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_it
%xdefine _PKCS8_PRIV_KEY_INFO_new _ %+ BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_new
%xdefine _PKCS8_decrypt _ %+ BORINGSSL_PREFIX %+ _PKCS8_decrypt
%xdefine _PKCS8_encrypt _ %+ BORINGSSL_PREFIX %+ _PKCS8_encrypt
%xdefine _PKCS8_marshal_encrypted_private_key _ %+ BORINGSSL_PREFIX %+ _PKCS8_marshal_encrypted_private_key
%xdefine _PKCS8_parse_encrypted_private_key _ %+ BORINGSSL_PREFIX %+ _PKCS8_parse_encrypted_private_key
%xdefine _POLICYINFO_free _ %+ BORINGSSL_PREFIX %+ _POLICYINFO_free
%xdefine _POLICYINFO_it _ %+ BORINGSSL_PREFIX %+ _POLICYINFO_it
%xdefine _POLICYINFO_new _ %+ BORINGSSL_PREFIX %+ _POLICYINFO_new
%xdefine _POLICYQUALINFO_free _ %+ BORINGSSL_PREFIX %+ _POLICYQUALINFO_free
%xdefine _POLICYQUALINFO_it _ %+ BORINGSSL_PREFIX %+ _POLICYQUALINFO_it
%xdefine _POLICYQUALINFO_new _ %+ BORINGSSL_PREFIX %+ _POLICYQUALINFO_new
%xdefine _POLICY_CONSTRAINTS_free _ %+ BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_free
%xdefine _POLICY_CONSTRAINTS_it _ %+ BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_it
%xdefine _POLICY_CONSTRAINTS_new _ %+ BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_new
%xdefine _POLICY_MAPPINGS_it _ %+ BORINGSSL_PREFIX %+ _POLICY_MAPPINGS_it
%xdefine _POLICY_MAPPING_free _ %+ BORINGSSL_PREFIX %+ _POLICY_MAPPING_free
%xdefine _POLICY_MAPPING_it _ %+ BORINGSSL_PREFIX %+ _POLICY_MAPPING_it
%xdefine _POLICY_MAPPING_new _ %+ BORINGSSL_PREFIX %+ _POLICY_MAPPING_new
%xdefine _PQDSA_KEY_free _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_free
%xdefine _PQDSA_KEY_get0_dsa _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_get0_dsa
%xdefine _PQDSA_KEY_init _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_init
%xdefine _PQDSA_KEY_new _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_new
%xdefine _PQDSA_KEY_set_raw_keypair_from_both _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_keypair_from_both
%xdefine _PQDSA_KEY_set_raw_keypair_from_seed _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_keypair_from_seed
%xdefine _PQDSA_KEY_set_raw_private_key _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_private_key
%xdefine _PQDSA_KEY_set_raw_public_key _ %+ BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_public_key
%xdefine _PQDSA_find_asn1_by_nid _ %+ BORINGSSL_PREFIX %+ _PQDSA_find_asn1_by_nid
%xdefine _PQDSA_find_dsa_by_nid _ %+ BORINGSSL_PREFIX %+ _PQDSA_find_dsa_by_nid
%xdefine _RAND_OpenSSL _ %+ BORINGSSL_PREFIX %+ _RAND_OpenSSL
%xdefine _RAND_SSLeay _ %+ BORINGSSL_PREFIX %+ _RAND_SSLeay
%xdefine _RAND_add _ %+ BORINGSSL_PREFIX %+ _RAND_add
%xdefine _RAND_bytes _ %+ BORINGSSL_PREFIX %+ _RAND_bytes
%xdefine _RAND_bytes_with_user_prediction_resistance _ %+ BORINGSSL_PREFIX %+ _RAND_bytes_with_user_prediction_resistance
%xdefine _RAND_cleanup _ %+ BORINGSSL_PREFIX %+ _RAND_cleanup
%xdefine _RAND_egd _ %+ BORINGSSL_PREFIX %+ _RAND_egd
%xdefine _RAND_egd_bytes _ %+ BORINGSSL_PREFIX %+ _RAND_egd_bytes
%xdefine _RAND_file_name _ %+ BORINGSSL_PREFIX %+ _RAND_file_name
%xdefine _RAND_get_rand_method _ %+ BORINGSSL_PREFIX %+ _RAND_get_rand_method
%xdefine _RAND_keep_random_devices_open _ %+ BORINGSSL_PREFIX %+ _RAND_keep_random_devices_open
%xdefine _RAND_load_file _ %+ BORINGSSL_PREFIX %+ _RAND_load_file
%xdefine _RAND_poll _ %+ BORINGSSL_PREFIX %+ _RAND_poll
%xdefine _RAND_priv_bytes _ %+ BORINGSSL_PREFIX %+ _RAND_priv_bytes
%xdefine _RAND_pseudo_bytes _ %+ BORINGSSL_PREFIX %+ _RAND_pseudo_bytes
%xdefine _RAND_seed _ %+ BORINGSSL_PREFIX %+ _RAND_seed
%xdefine _RAND_set_rand_method _ %+ BORINGSSL_PREFIX %+ _RAND_set_rand_method
%xdefine _RAND_status _ %+ BORINGSSL_PREFIX %+ _RAND_status
%xdefine _RAND_write_file _ %+ BORINGSSL_PREFIX %+ _RAND_write_file
%xdefine _RC4 _ %+ BORINGSSL_PREFIX %+ _RC4
%xdefine _RC4_options _ %+ BORINGSSL_PREFIX %+ _RC4_options
%xdefine _RC4_set_key _ %+ BORINGSSL_PREFIX %+ _RC4_set_key
%xdefine _RFC8032_DOM2_PREFIX _ %+ BORINGSSL_PREFIX %+ _RFC8032_DOM2_PREFIX
%xdefine _RIPEMD160 _ %+ BORINGSSL_PREFIX %+ _RIPEMD160
%xdefine _RIPEMD160_Final _ %+ BORINGSSL_PREFIX %+ _RIPEMD160_Final
%xdefine _RIPEMD160_Init _ %+ BORINGSSL_PREFIX %+ _RIPEMD160_Init
%xdefine _RIPEMD160_Update _ %+ BORINGSSL_PREFIX %+ _RIPEMD160_Update
%xdefine _RSAPrivateKey_dup _ %+ BORINGSSL_PREFIX %+ _RSAPrivateKey_dup
%xdefine _RSAPublicKey_dup _ %+ BORINGSSL_PREFIX %+ _RSAPublicKey_dup
%xdefine _RSASSA_PSS_PARAMS_create _ %+ BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_create
%xdefine _RSASSA_PSS_PARAMS_free _ %+ BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_free
%xdefine _RSASSA_PSS_PARAMS_get _ %+ BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_get
%xdefine _RSASSA_PSS_PARAMS_new _ %+ BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_new
%xdefine _RSASSA_PSS_parse_params _ %+ BORINGSSL_PREFIX %+ _RSASSA_PSS_parse_params
%xdefine _RSAZ_1024_mod_exp_avx2 _ %+ BORINGSSL_PREFIX %+ _RSAZ_1024_mod_exp_avx2
%xdefine _RSAZ_mod_exp_avx512_x2 _ %+ BORINGSSL_PREFIX %+ _RSAZ_mod_exp_avx512_x2
%xdefine _RSA_ALGOR_IDENTIFIER_free _ %+ BORINGSSL_PREFIX %+ _RSA_ALGOR_IDENTIFIER_free
%xdefine _RSA_ALGOR_IDENTIFIER_new _ %+ BORINGSSL_PREFIX %+ _RSA_ALGOR_IDENTIFIER_new
%xdefine _RSA_INTEGER_free _ %+ BORINGSSL_PREFIX %+ _RSA_INTEGER_free
%xdefine _RSA_INTEGER_new _ %+ BORINGSSL_PREFIX %+ _RSA_INTEGER_new
%xdefine _RSA_MGA_IDENTIFIER_free _ %+ BORINGSSL_PREFIX %+ _RSA_MGA_IDENTIFIER_free
%xdefine _RSA_MGA_IDENTIFIER_new _ %+ BORINGSSL_PREFIX %+ _RSA_MGA_IDENTIFIER_new
%xdefine _RSA_PSS_PARAMS_free _ %+ BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_free
%xdefine _RSA_PSS_PARAMS_it _ %+ BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_it
%xdefine _RSA_PSS_PARAMS_new _ %+ BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_new
%xdefine _RSA_add_pkcs1_prefix _ %+ BORINGSSL_PREFIX %+ _RSA_add_pkcs1_prefix
%xdefine _RSA_bits _ %+ BORINGSSL_PREFIX %+ _RSA_bits
%xdefine _RSA_blinding_off_temp_for_accp_compatibility _ %+ BORINGSSL_PREFIX %+ _RSA_blinding_off_temp_for_accp_compatibility
%xdefine _RSA_blinding_on _ %+ BORINGSSL_PREFIX %+ _RSA_blinding_on
%xdefine _RSA_check_fips _ %+ BORINGSSL_PREFIX %+ _RSA_check_fips
%xdefine _RSA_check_key _ %+ BORINGSSL_PREFIX %+ _RSA_check_key
%xdefine _RSA_decrypt _ %+ BORINGSSL_PREFIX %+ _RSA_decrypt
%xdefine _RSA_encrypt _ %+ BORINGSSL_PREFIX %+ _RSA_encrypt
%xdefine _RSA_flags _ %+ BORINGSSL_PREFIX %+ _RSA_flags
%xdefine _RSA_free _ %+ BORINGSSL_PREFIX %+ _RSA_free
%xdefine _RSA_generate_key _ %+ BORINGSSL_PREFIX %+ _RSA_generate_key
%xdefine _RSA_generate_key_ex _ %+ BORINGSSL_PREFIX %+ _RSA_generate_key_ex
%xdefine _RSA_generate_key_fips _ %+ BORINGSSL_PREFIX %+ _RSA_generate_key_fips
%xdefine _RSA_get0_crt_params _ %+ BORINGSSL_PREFIX %+ _RSA_get0_crt_params
%xdefine _RSA_get0_d _ %+ BORINGSSL_PREFIX %+ _RSA_get0_d
%xdefine _RSA_get0_dmp1 _ %+ BORINGSSL_PREFIX %+ _RSA_get0_dmp1
%xdefine _RSA_get0_dmq1 _ %+ BORINGSSL_PREFIX %+ _RSA_get0_dmq1
%xdefine _RSA_get0_e _ %+ BORINGSSL_PREFIX %+ _RSA_get0_e
%xdefine _RSA_get0_factors _ %+ BORINGSSL_PREFIX %+ _RSA_get0_factors
%xdefine _RSA_get0_iqmp _ %+ BORINGSSL_PREFIX %+ _RSA_get0_iqmp
%xdefine _RSA_get0_key _ %+ BORINGSSL_PREFIX %+ _RSA_get0_key
%xdefine _RSA_get0_n _ %+ BORINGSSL_PREFIX %+ _RSA_get0_n
%xdefine _RSA_get0_p _ %+ BORINGSSL_PREFIX %+ _RSA_get0_p
%xdefine _RSA_get0_pss_params _ %+ BORINGSSL_PREFIX %+ _RSA_get0_pss_params
%xdefine _RSA_get0_q _ %+ BORINGSSL_PREFIX %+ _RSA_get0_q
%xdefine _RSA_get_default_method _ %+ BORINGSSL_PREFIX %+ _RSA_get_default_method
%xdefine _RSA_get_ex_data _ %+ BORINGSSL_PREFIX %+ _RSA_get_ex_data
%xdefine _RSA_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _RSA_get_ex_new_index
%xdefine _RSA_get_method _ %+ BORINGSSL_PREFIX %+ _RSA_get_method
%xdefine _RSA_is_opaque _ %+ BORINGSSL_PREFIX %+ _RSA_is_opaque
%xdefine _RSA_marshal_private_key _ %+ BORINGSSL_PREFIX %+ _RSA_marshal_private_key
%xdefine _RSA_marshal_public_key _ %+ BORINGSSL_PREFIX %+ _RSA_marshal_public_key
%xdefine _RSA_meth_free _ %+ BORINGSSL_PREFIX %+ _RSA_meth_free
%xdefine _RSA_meth_new _ %+ BORINGSSL_PREFIX %+ _RSA_meth_new
%xdefine _RSA_meth_set0_app_data _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set0_app_data
%xdefine _RSA_meth_set_finish _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_finish
%xdefine _RSA_meth_set_init _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_init
%xdefine _RSA_meth_set_priv_dec _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_priv_dec
%xdefine _RSA_meth_set_priv_enc _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_priv_enc
%xdefine _RSA_meth_set_pub_dec _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_pub_dec
%xdefine _RSA_meth_set_pub_enc _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_pub_enc
%xdefine _RSA_meth_set_sign _ %+ BORINGSSL_PREFIX %+ _RSA_meth_set_sign
%xdefine _RSA_new _ %+ BORINGSSL_PREFIX %+ _RSA_new
%xdefine _RSA_new_method _ %+ BORINGSSL_PREFIX %+ _RSA_new_method
%xdefine _RSA_new_method_no_e _ %+ BORINGSSL_PREFIX %+ _RSA_new_method_no_e
%xdefine _RSA_new_private_key _ %+ BORINGSSL_PREFIX %+ _RSA_new_private_key
%xdefine _RSA_new_private_key_large_e _ %+ BORINGSSL_PREFIX %+ _RSA_new_private_key_large_e
%xdefine _RSA_new_private_key_no_crt _ %+ BORINGSSL_PREFIX %+ _RSA_new_private_key_no_crt
%xdefine _RSA_new_private_key_no_e _ %+ BORINGSSL_PREFIX %+ _RSA_new_private_key_no_e
%xdefine _RSA_new_public_key _ %+ BORINGSSL_PREFIX %+ _RSA_new_public_key
%xdefine _RSA_new_public_key_large_e _ %+ BORINGSSL_PREFIX %+ _RSA_new_public_key_large_e
%xdefine _RSA_padding_add_PKCS1_OAEP _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_OAEP
%xdefine _RSA_padding_add_PKCS1_OAEP_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_OAEP_mgf1
%xdefine _RSA_padding_add_PKCS1_PSS _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_PSS
%xdefine _RSA_padding_add_PKCS1_PSS_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_PSS_mgf1
%xdefine _RSA_padding_add_PKCS1_type_1 _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_type_1
%xdefine _RSA_padding_add_none _ %+ BORINGSSL_PREFIX %+ _RSA_padding_add_none
%xdefine _RSA_padding_check_PKCS1_OAEP_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_padding_check_PKCS1_OAEP_mgf1
%xdefine _RSA_padding_check_PKCS1_type_1 _ %+ BORINGSSL_PREFIX %+ _RSA_padding_check_PKCS1_type_1
%xdefine _RSA_parse_private_key _ %+ BORINGSSL_PREFIX %+ _RSA_parse_private_key
%xdefine _RSA_parse_public_key _ %+ BORINGSSL_PREFIX %+ _RSA_parse_public_key
%xdefine _RSA_pkey_ctx_ctrl _ %+ BORINGSSL_PREFIX %+ _RSA_pkey_ctx_ctrl
%xdefine _RSA_print _ %+ BORINGSSL_PREFIX %+ _RSA_print
%xdefine _RSA_print_fp _ %+ BORINGSSL_PREFIX %+ _RSA_print_fp
%xdefine _RSA_private_decrypt _ %+ BORINGSSL_PREFIX %+ _RSA_private_decrypt
%xdefine _RSA_private_encrypt _ %+ BORINGSSL_PREFIX %+ _RSA_private_encrypt
%xdefine _RSA_private_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _RSA_private_key_from_bytes
%xdefine _RSA_private_key_to_bytes _ %+ BORINGSSL_PREFIX %+ _RSA_private_key_to_bytes
%xdefine _RSA_public_decrypt _ %+ BORINGSSL_PREFIX %+ _RSA_public_decrypt
%xdefine _RSA_public_encrypt _ %+ BORINGSSL_PREFIX %+ _RSA_public_encrypt
%xdefine _RSA_public_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _RSA_public_key_from_bytes
%xdefine _RSA_public_key_to_bytes _ %+ BORINGSSL_PREFIX %+ _RSA_public_key_to_bytes
%xdefine _RSA_set0_crt_params _ %+ BORINGSSL_PREFIX %+ _RSA_set0_crt_params
%xdefine _RSA_set0_factors _ %+ BORINGSSL_PREFIX %+ _RSA_set0_factors
%xdefine _RSA_set0_key _ %+ BORINGSSL_PREFIX %+ _RSA_set0_key
%xdefine _RSA_set_ex_data _ %+ BORINGSSL_PREFIX %+ _RSA_set_ex_data
%xdefine _RSA_set_flags _ %+ BORINGSSL_PREFIX %+ _RSA_set_flags
%xdefine _RSA_set_method _ %+ BORINGSSL_PREFIX %+ _RSA_set_method
%xdefine _RSA_sign _ %+ BORINGSSL_PREFIX %+ _RSA_sign
%xdefine _RSA_sign_pss_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_sign_pss_mgf1
%xdefine _RSA_sign_raw _ %+ BORINGSSL_PREFIX %+ _RSA_sign_raw
%xdefine _RSA_size _ %+ BORINGSSL_PREFIX %+ _RSA_size
%xdefine _RSA_test_flags _ %+ BORINGSSL_PREFIX %+ _RSA_test_flags
%xdefine _RSA_up_ref _ %+ BORINGSSL_PREFIX %+ _RSA_up_ref
%xdefine _RSA_verify _ %+ BORINGSSL_PREFIX %+ _RSA_verify
%xdefine _RSA_verify_PKCS1_PSS _ %+ BORINGSSL_PREFIX %+ _RSA_verify_PKCS1_PSS
%xdefine _RSA_verify_PKCS1_PSS_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_verify_PKCS1_PSS_mgf1
%xdefine _RSA_verify_pss_mgf1 _ %+ BORINGSSL_PREFIX %+ _RSA_verify_pss_mgf1
%xdefine _RSA_verify_raw _ %+ BORINGSSL_PREFIX %+ _RSA_verify_raw
%xdefine _SHA1 _ %+ BORINGSSL_PREFIX %+ _SHA1
%xdefine _SHA1_Final _ %+ BORINGSSL_PREFIX %+ _SHA1_Final
%xdefine _SHA1_Init _ %+ BORINGSSL_PREFIX %+ _SHA1_Init
%xdefine _SHA1_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA1_Init_from_state
%xdefine _SHA1_Transform _ %+ BORINGSSL_PREFIX %+ _SHA1_Transform
%xdefine _SHA1_Update _ %+ BORINGSSL_PREFIX %+ _SHA1_Update
%xdefine _SHA1_get_state _ %+ BORINGSSL_PREFIX %+ _SHA1_get_state
%xdefine _SHA224 _ %+ BORINGSSL_PREFIX %+ _SHA224
%xdefine _SHA224_Final _ %+ BORINGSSL_PREFIX %+ _SHA224_Final
%xdefine _SHA224_Init _ %+ BORINGSSL_PREFIX %+ _SHA224_Init
%xdefine _SHA224_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA224_Init_from_state
%xdefine _SHA224_Update _ %+ BORINGSSL_PREFIX %+ _SHA224_Update
%xdefine _SHA224_get_state _ %+ BORINGSSL_PREFIX %+ _SHA224_get_state
%xdefine _SHA256 _ %+ BORINGSSL_PREFIX %+ _SHA256
%xdefine _SHA256_Final _ %+ BORINGSSL_PREFIX %+ _SHA256_Final
%xdefine _SHA256_Init _ %+ BORINGSSL_PREFIX %+ _SHA256_Init
%xdefine _SHA256_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA256_Init_from_state
%xdefine _SHA256_Transform _ %+ BORINGSSL_PREFIX %+ _SHA256_Transform
%xdefine _SHA256_TransformBlocks _ %+ BORINGSSL_PREFIX %+ _SHA256_TransformBlocks
%xdefine _SHA256_Update _ %+ BORINGSSL_PREFIX %+ _SHA256_Update
%xdefine _SHA256_get_state _ %+ BORINGSSL_PREFIX %+ _SHA256_get_state
%xdefine _SHA384 _ %+ BORINGSSL_PREFIX %+ _SHA384
%xdefine _SHA384_Final _ %+ BORINGSSL_PREFIX %+ _SHA384_Final
%xdefine _SHA384_Init _ %+ BORINGSSL_PREFIX %+ _SHA384_Init
%xdefine _SHA384_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA384_Init_from_state
%xdefine _SHA384_Update _ %+ BORINGSSL_PREFIX %+ _SHA384_Update
%xdefine _SHA384_get_state _ %+ BORINGSSL_PREFIX %+ _SHA384_get_state
%xdefine _SHA3_224 _ %+ BORINGSSL_PREFIX %+ _SHA3_224
%xdefine _SHA3_224_Final _ %+ BORINGSSL_PREFIX %+ _SHA3_224_Final
%xdefine _SHA3_224_Init _ %+ BORINGSSL_PREFIX %+ _SHA3_224_Init
%xdefine _SHA3_224_Update _ %+ BORINGSSL_PREFIX %+ _SHA3_224_Update
%xdefine _SHA3_256 _ %+ BORINGSSL_PREFIX %+ _SHA3_256
%xdefine _SHA3_256_Final _ %+ BORINGSSL_PREFIX %+ _SHA3_256_Final
%xdefine _SHA3_256_Init _ %+ BORINGSSL_PREFIX %+ _SHA3_256_Init
%xdefine _SHA3_256_Update _ %+ BORINGSSL_PREFIX %+ _SHA3_256_Update
%xdefine _SHA3_384 _ %+ BORINGSSL_PREFIX %+ _SHA3_384
%xdefine _SHA3_384_Final _ %+ BORINGSSL_PREFIX %+ _SHA3_384_Final
%xdefine _SHA3_384_Init _ %+ BORINGSSL_PREFIX %+ _SHA3_384_Init
%xdefine _SHA3_384_Update _ %+ BORINGSSL_PREFIX %+ _SHA3_384_Update
%xdefine _SHA3_512 _ %+ BORINGSSL_PREFIX %+ _SHA3_512
%xdefine _SHA3_512_Final _ %+ BORINGSSL_PREFIX %+ _SHA3_512_Final
%xdefine _SHA3_512_Init _ %+ BORINGSSL_PREFIX %+ _SHA3_512_Init
%xdefine _SHA3_512_Update _ %+ BORINGSSL_PREFIX %+ _SHA3_512_Update
%xdefine _SHA3_Final _ %+ BORINGSSL_PREFIX %+ _SHA3_Final
%xdefine _SHA3_Init _ %+ BORINGSSL_PREFIX %+ _SHA3_Init
%xdefine _SHA3_Update _ %+ BORINGSSL_PREFIX %+ _SHA3_Update
%xdefine _SHA512 _ %+ BORINGSSL_PREFIX %+ _SHA512
%xdefine _SHA512_224 _ %+ BORINGSSL_PREFIX %+ _SHA512_224
%xdefine _SHA512_224_Final _ %+ BORINGSSL_PREFIX %+ _SHA512_224_Final
%xdefine _SHA512_224_Init _ %+ BORINGSSL_PREFIX %+ _SHA512_224_Init
%xdefine _SHA512_224_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA512_224_Init_from_state
%xdefine _SHA512_224_Update _ %+ BORINGSSL_PREFIX %+ _SHA512_224_Update
%xdefine _SHA512_224_get_state _ %+ BORINGSSL_PREFIX %+ _SHA512_224_get_state
%xdefine _SHA512_256 _ %+ BORINGSSL_PREFIX %+ _SHA512_256
%xdefine _SHA512_256_Final _ %+ BORINGSSL_PREFIX %+ _SHA512_256_Final
%xdefine _SHA512_256_Init _ %+ BORINGSSL_PREFIX %+ _SHA512_256_Init
%xdefine _SHA512_256_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA512_256_Init_from_state
%xdefine _SHA512_256_Update _ %+ BORINGSSL_PREFIX %+ _SHA512_256_Update
%xdefine _SHA512_256_get_state _ %+ BORINGSSL_PREFIX %+ _SHA512_256_get_state
%xdefine _SHA512_Final _ %+ BORINGSSL_PREFIX %+ _SHA512_Final
%xdefine _SHA512_Init _ %+ BORINGSSL_PREFIX %+ _SHA512_Init
%xdefine _SHA512_Init_from_state _ %+ BORINGSSL_PREFIX %+ _SHA512_Init_from_state
%xdefine _SHA512_Transform _ %+ BORINGSSL_PREFIX %+ _SHA512_Transform
%xdefine _SHA512_Update _ %+ BORINGSSL_PREFIX %+ _SHA512_Update
%xdefine _SHA512_get_state _ %+ BORINGSSL_PREFIX %+ _SHA512_get_state
%xdefine _SHAKE128 _ %+ BORINGSSL_PREFIX %+ _SHAKE128
%xdefine _SHAKE128_Absorb_once_x4 _ %+ BORINGSSL_PREFIX %+ _SHAKE128_Absorb_once_x4
%xdefine _SHAKE128_Init_x4 _ %+ BORINGSSL_PREFIX %+ _SHAKE128_Init_x4
%xdefine _SHAKE128_Squeezeblocks_x4 _ %+ BORINGSSL_PREFIX %+ _SHAKE128_Squeezeblocks_x4
%xdefine _SHAKE256 _ %+ BORINGSSL_PREFIX %+ _SHAKE256
%xdefine _SHAKE256_x4 _ %+ BORINGSSL_PREFIX %+ _SHAKE256_x4
%xdefine _SHAKE_Absorb _ %+ BORINGSSL_PREFIX %+ _SHAKE_Absorb
%xdefine _SHAKE_Final _ %+ BORINGSSL_PREFIX %+ _SHAKE_Final
%xdefine _SHAKE_Init _ %+ BORINGSSL_PREFIX %+ _SHAKE_Init
%xdefine _SHAKE_Squeeze _ %+ BORINGSSL_PREFIX %+ _SHAKE_Squeeze
%xdefine _SIPHASH_24 _ %+ BORINGSSL_PREFIX %+ _SIPHASH_24
%xdefine _SMIME_read_PKCS7 _ %+ BORINGSSL_PREFIX %+ _SMIME_read_PKCS7
%xdefine _SMIME_write_PKCS7 _ %+ BORINGSSL_PREFIX %+ _SMIME_write_PKCS7
%xdefine _SPAKE2_CTX_free _ %+ BORINGSSL_PREFIX %+ _SPAKE2_CTX_free
%xdefine _SPAKE2_CTX_new _ %+ BORINGSSL_PREFIX %+ _SPAKE2_CTX_new
%xdefine _SPAKE2_generate_msg _ %+ BORINGSSL_PREFIX %+ _SPAKE2_generate_msg
%xdefine _SPAKE2_process_msg _ %+ BORINGSSL_PREFIX %+ _SPAKE2_process_msg
%xdefine _SSHKDF _ %+ BORINGSSL_PREFIX %+ _SSHKDF
%xdefine _SSKDF_digest _ %+ BORINGSSL_PREFIX %+ _SSKDF_digest
%xdefine _SSKDF_hmac _ %+ BORINGSSL_PREFIX %+ _SSKDF_hmac
%xdefine _SSLeay _ %+ BORINGSSL_PREFIX %+ _SSLeay
%xdefine _SSLeay_version _ %+ BORINGSSL_PREFIX %+ _SSLeay_version
%xdefine _TRUST_TOKEN_CLIENT_add_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_add_key
%xdefine _TRUST_TOKEN_CLIENT_begin_issuance _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_issuance
%xdefine _TRUST_TOKEN_CLIENT_begin_issuance_over_message _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_issuance_over_message
%xdefine _TRUST_TOKEN_CLIENT_begin_redemption _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_redemption
%xdefine _TRUST_TOKEN_CLIENT_finish_issuance _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_finish_issuance
%xdefine _TRUST_TOKEN_CLIENT_finish_redemption _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_finish_redemption
%xdefine _TRUST_TOKEN_CLIENT_free _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_free
%xdefine _TRUST_TOKEN_CLIENT_new _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_new
%xdefine _TRUST_TOKEN_CLIENT_set_srr_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_set_srr_key
%xdefine _TRUST_TOKEN_ISSUER_add_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_add_key
%xdefine _TRUST_TOKEN_ISSUER_free _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_free
%xdefine _TRUST_TOKEN_ISSUER_issue _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_issue
%xdefine _TRUST_TOKEN_ISSUER_new _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_new
%xdefine _TRUST_TOKEN_ISSUER_redeem _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_redeem
%xdefine _TRUST_TOKEN_ISSUER_redeem_over_message _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_redeem_over_message
%xdefine _TRUST_TOKEN_ISSUER_set_metadata_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_set_metadata_key
%xdefine _TRUST_TOKEN_ISSUER_set_srr_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_set_srr_key
%xdefine _TRUST_TOKEN_PRETOKEN_free _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_PRETOKEN_free
%xdefine _TRUST_TOKEN_decode_private_metadata _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_decode_private_metadata
%xdefine _TRUST_TOKEN_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_derive_key_from_secret
%xdefine _TRUST_TOKEN_experiment_v1 _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v1
%xdefine _TRUST_TOKEN_experiment_v2_pmb _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v2_pmb
%xdefine _TRUST_TOKEN_experiment_v2_voprf _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v2_voprf
%xdefine _TRUST_TOKEN_free _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_free
%xdefine _TRUST_TOKEN_generate_key _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_generate_key
%xdefine _TRUST_TOKEN_new _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_new
%xdefine _TRUST_TOKEN_pst_v1_pmb _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_pst_v1_pmb
%xdefine _TRUST_TOKEN_pst_v1_voprf _ %+ BORINGSSL_PREFIX %+ _TRUST_TOKEN_pst_v1_voprf
%xdefine _UI_add_info_string _ %+ BORINGSSL_PREFIX %+ _UI_add_info_string
%xdefine _UI_add_input_string _ %+ BORINGSSL_PREFIX %+ _UI_add_input_string
%xdefine _UI_add_verify_string _ %+ BORINGSSL_PREFIX %+ _UI_add_verify_string
%xdefine _UI_free _ %+ BORINGSSL_PREFIX %+ _UI_free
%xdefine _UI_new _ %+ BORINGSSL_PREFIX %+ _UI_new
%xdefine _UI_process _ %+ BORINGSSL_PREFIX %+ _UI_process
%xdefine _USERNOTICE_free _ %+ BORINGSSL_PREFIX %+ _USERNOTICE_free
%xdefine _USERNOTICE_it _ %+ BORINGSSL_PREFIX %+ _USERNOTICE_it
%xdefine _USERNOTICE_new _ %+ BORINGSSL_PREFIX %+ _USERNOTICE_new
%xdefine _UTF8_getc _ %+ BORINGSSL_PREFIX %+ _UTF8_getc
%xdefine _UTF8_putc _ %+ BORINGSSL_PREFIX %+ _UTF8_putc
%xdefine _X25519 _ %+ BORINGSSL_PREFIX %+ _X25519
%xdefine _X25519_keypair _ %+ BORINGSSL_PREFIX %+ _X25519_keypair
%xdefine _X25519_public_from_private _ %+ BORINGSSL_PREFIX %+ _X25519_public_from_private
%xdefine _X509V3_EXT_CRL_add_nconf _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_CRL_add_nconf
%xdefine _X509V3_EXT_REQ_add_nconf _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_REQ_add_nconf
%xdefine _X509V3_EXT_add _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_add
%xdefine _X509V3_EXT_add_alias _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_add_alias
%xdefine _X509V3_EXT_add_nconf _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_add_nconf
%xdefine _X509V3_EXT_add_nconf_sk _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_add_nconf_sk
%xdefine _X509V3_EXT_conf _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_conf
%xdefine _X509V3_EXT_conf_nid _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_conf_nid
%xdefine _X509V3_EXT_d2i _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_d2i
%xdefine _X509V3_EXT_free _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_free
%xdefine _X509V3_EXT_get _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_get
%xdefine _X509V3_EXT_get_nid _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_get_nid
%xdefine _X509V3_EXT_i2d _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_i2d
%xdefine _X509V3_EXT_nconf _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_nconf
%xdefine _X509V3_EXT_nconf_nid _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_nconf_nid
%xdefine _X509V3_EXT_print _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_print
%xdefine _X509V3_EXT_print_fp _ %+ BORINGSSL_PREFIX %+ _X509V3_EXT_print_fp
%xdefine _X509V3_NAME_from_section _ %+ BORINGSSL_PREFIX %+ _X509V3_NAME_from_section
%xdefine _X509V3_add1_i2d _ %+ BORINGSSL_PREFIX %+ _X509V3_add1_i2d
%xdefine _X509V3_add_standard_extensions _ %+ BORINGSSL_PREFIX %+ _X509V3_add_standard_extensions
%xdefine _X509V3_add_value _ %+ BORINGSSL_PREFIX %+ _X509V3_add_value
%xdefine _X509V3_add_value_bool _ %+ BORINGSSL_PREFIX %+ _X509V3_add_value_bool
%xdefine _X509V3_add_value_int _ %+ BORINGSSL_PREFIX %+ _X509V3_add_value_int
%xdefine _X509V3_bool_from_string _ %+ BORINGSSL_PREFIX %+ _X509V3_bool_from_string
%xdefine _X509V3_conf_free _ %+ BORINGSSL_PREFIX %+ _X509V3_conf_free
%xdefine _X509V3_extensions_print _ %+ BORINGSSL_PREFIX %+ _X509V3_extensions_print
%xdefine _X509V3_get_d2i _ %+ BORINGSSL_PREFIX %+ _X509V3_get_d2i
%xdefine _X509V3_get_section _ %+ BORINGSSL_PREFIX %+ _X509V3_get_section
%xdefine _X509V3_get_value_bool _ %+ BORINGSSL_PREFIX %+ _X509V3_get_value_bool
%xdefine _X509V3_get_value_int _ %+ BORINGSSL_PREFIX %+ _X509V3_get_value_int
%xdefine _X509V3_parse_list _ %+ BORINGSSL_PREFIX %+ _X509V3_parse_list
%xdefine _X509V3_set_ctx _ %+ BORINGSSL_PREFIX %+ _X509V3_set_ctx
%xdefine _X509V3_set_nconf _ %+ BORINGSSL_PREFIX %+ _X509V3_set_nconf
%xdefine _X509_ALGOR_cmp _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_cmp
%xdefine _X509_ALGOR_dup _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_dup
%xdefine _X509_ALGOR_free _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_free
%xdefine _X509_ALGOR_get0 _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_get0
%xdefine _X509_ALGOR_it _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_it
%xdefine _X509_ALGOR_new _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_new
%xdefine _X509_ALGOR_set0 _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_set0
%xdefine _X509_ALGOR_set_md _ %+ BORINGSSL_PREFIX %+ _X509_ALGOR_set_md
%xdefine _X509_ATTRIBUTE_count _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_count
%xdefine _X509_ATTRIBUTE_create _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create
%xdefine _X509_ATTRIBUTE_create_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_NID
%xdefine _X509_ATTRIBUTE_create_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_OBJ
%xdefine _X509_ATTRIBUTE_create_by_txt _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_txt
%xdefine _X509_ATTRIBUTE_dup _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_dup
%xdefine _X509_ATTRIBUTE_free _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_free
%xdefine _X509_ATTRIBUTE_get0_data _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_data
%xdefine _X509_ATTRIBUTE_get0_object _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_object
%xdefine _X509_ATTRIBUTE_get0_type _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_type
%xdefine _X509_ATTRIBUTE_it _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_it
%xdefine _X509_ATTRIBUTE_new _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_new
%xdefine _X509_ATTRIBUTE_set1_data _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_set1_data
%xdefine _X509_ATTRIBUTE_set1_object _ %+ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_set1_object
%xdefine _X509_CERT_AUX_free _ %+ BORINGSSL_PREFIX %+ _X509_CERT_AUX_free
%xdefine _X509_CERT_AUX_it _ %+ BORINGSSL_PREFIX %+ _X509_CERT_AUX_it
%xdefine _X509_CERT_AUX_new _ %+ BORINGSSL_PREFIX %+ _X509_CERT_AUX_new
%xdefine _X509_CERT_AUX_print _ %+ BORINGSSL_PREFIX %+ _X509_CERT_AUX_print
%xdefine _X509_CINF_free _ %+ BORINGSSL_PREFIX %+ _X509_CINF_free
%xdefine _X509_CINF_it _ %+ BORINGSSL_PREFIX %+ _X509_CINF_it
%xdefine _X509_CINF_new _ %+ BORINGSSL_PREFIX %+ _X509_CINF_new
%xdefine _X509_CRL_INFO_free _ %+ BORINGSSL_PREFIX %+ _X509_CRL_INFO_free
%xdefine _X509_CRL_INFO_it _ %+ BORINGSSL_PREFIX %+ _X509_CRL_INFO_it
%xdefine _X509_CRL_INFO_new _ %+ BORINGSSL_PREFIX %+ _X509_CRL_INFO_new
%xdefine _X509_CRL_add0_revoked _ %+ BORINGSSL_PREFIX %+ _X509_CRL_add0_revoked
%xdefine _X509_CRL_add1_ext_i2d _ %+ BORINGSSL_PREFIX %+ _X509_CRL_add1_ext_i2d
%xdefine _X509_CRL_add_ext _ %+ BORINGSSL_PREFIX %+ _X509_CRL_add_ext
%xdefine _X509_CRL_cmp _ %+ BORINGSSL_PREFIX %+ _X509_CRL_cmp
%xdefine _X509_CRL_delete_ext _ %+ BORINGSSL_PREFIX %+ _X509_CRL_delete_ext
%xdefine _X509_CRL_digest _ %+ BORINGSSL_PREFIX %+ _X509_CRL_digest
%xdefine _X509_CRL_dup _ %+ BORINGSSL_PREFIX %+ _X509_CRL_dup
%xdefine _X509_CRL_free _ %+ BORINGSSL_PREFIX %+ _X509_CRL_free
%xdefine _X509_CRL_get0_by_cert _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_by_cert
%xdefine _X509_CRL_get0_by_serial _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_by_serial
%xdefine _X509_CRL_get0_extensions _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_extensions
%xdefine _X509_CRL_get0_lastUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_lastUpdate
%xdefine _X509_CRL_get0_nextUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_nextUpdate
%xdefine _X509_CRL_get0_signature _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get0_signature
%xdefine _X509_CRL_get_REVOKED _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_REVOKED
%xdefine _X509_CRL_get_ext _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext
%xdefine _X509_CRL_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_NID
%xdefine _X509_CRL_get_ext_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_OBJ
%xdefine _X509_CRL_get_ext_by_critical _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_critical
%xdefine _X509_CRL_get_ext_count _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_count
%xdefine _X509_CRL_get_ext_d2i _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_d2i
%xdefine _X509_CRL_get_issuer _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_issuer
%xdefine _X509_CRL_get_lastUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_lastUpdate
%xdefine _X509_CRL_get_nextUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_nextUpdate
%xdefine _X509_CRL_get_signature_nid _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_signature_nid
%xdefine _X509_CRL_get_version _ %+ BORINGSSL_PREFIX %+ _X509_CRL_get_version
%xdefine _X509_CRL_http_nbio _ %+ BORINGSSL_PREFIX %+ _X509_CRL_http_nbio
%xdefine _X509_CRL_it _ %+ BORINGSSL_PREFIX %+ _X509_CRL_it
%xdefine _X509_CRL_match _ %+ BORINGSSL_PREFIX %+ _X509_CRL_match
%xdefine _X509_CRL_new _ %+ BORINGSSL_PREFIX %+ _X509_CRL_new
%xdefine _X509_CRL_print _ %+ BORINGSSL_PREFIX %+ _X509_CRL_print
%xdefine _X509_CRL_print_fp _ %+ BORINGSSL_PREFIX %+ _X509_CRL_print_fp
%xdefine _X509_CRL_set1_lastUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set1_lastUpdate
%xdefine _X509_CRL_set1_nextUpdate _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set1_nextUpdate
%xdefine _X509_CRL_set1_signature_algo _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set1_signature_algo
%xdefine _X509_CRL_set1_signature_value _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set1_signature_value
%xdefine _X509_CRL_set_issuer_name _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set_issuer_name
%xdefine _X509_CRL_set_version _ %+ BORINGSSL_PREFIX %+ _X509_CRL_set_version
%xdefine _X509_CRL_sign _ %+ BORINGSSL_PREFIX %+ _X509_CRL_sign
%xdefine _X509_CRL_sign_ctx _ %+ BORINGSSL_PREFIX %+ _X509_CRL_sign_ctx
%xdefine _X509_CRL_sort _ %+ BORINGSSL_PREFIX %+ _X509_CRL_sort
%xdefine _X509_CRL_up_ref _ %+ BORINGSSL_PREFIX %+ _X509_CRL_up_ref
%xdefine _X509_CRL_verify _ %+ BORINGSSL_PREFIX %+ _X509_CRL_verify
%xdefine _X509_EXTENSIONS_it _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSIONS_it
%xdefine _X509_EXTENSION_create_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_create_by_NID
%xdefine _X509_EXTENSION_create_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_create_by_OBJ
%xdefine _X509_EXTENSION_dup _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_dup
%xdefine _X509_EXTENSION_free _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_free
%xdefine _X509_EXTENSION_get_critical _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_get_critical
%xdefine _X509_EXTENSION_get_data _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_get_data
%xdefine _X509_EXTENSION_get_object _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_get_object
%xdefine _X509_EXTENSION_it _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_it
%xdefine _X509_EXTENSION_new _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_new
%xdefine _X509_EXTENSION_set_critical _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_set_critical
%xdefine _X509_EXTENSION_set_data _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_set_data
%xdefine _X509_EXTENSION_set_object _ %+ BORINGSSL_PREFIX %+ _X509_EXTENSION_set_object
%xdefine _X509_INFO_free _ %+ BORINGSSL_PREFIX %+ _X509_INFO_free
%xdefine _X509_LOOKUP_add_dir _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_add_dir
%xdefine _X509_LOOKUP_ctrl _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_ctrl
%xdefine _X509_LOOKUP_file _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_file
%xdefine _X509_LOOKUP_free _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_free
%xdefine _X509_LOOKUP_hash_dir _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_hash_dir
%xdefine _X509_LOOKUP_load_file _ %+ BORINGSSL_PREFIX %+ _X509_LOOKUP_load_file
%xdefine _X509_NAME_ENTRIES_it _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRIES_it
%xdefine _X509_NAME_ENTRY_create_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_NID
%xdefine _X509_NAME_ENTRY_create_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_OBJ
%xdefine _X509_NAME_ENTRY_create_by_txt _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_txt
%xdefine _X509_NAME_ENTRY_dup _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_dup
%xdefine _X509_NAME_ENTRY_free _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_free
%xdefine _X509_NAME_ENTRY_get_data _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_get_data
%xdefine _X509_NAME_ENTRY_get_object _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_get_object
%xdefine _X509_NAME_ENTRY_it _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_it
%xdefine _X509_NAME_ENTRY_new _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_new
%xdefine _X509_NAME_ENTRY_set _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set
%xdefine _X509_NAME_ENTRY_set_data _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set_data
%xdefine _X509_NAME_ENTRY_set_object _ %+ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set_object
%xdefine _X509_NAME_INTERNAL_it _ %+ BORINGSSL_PREFIX %+ _X509_NAME_INTERNAL_it
%xdefine _X509_NAME_add_entry _ %+ BORINGSSL_PREFIX %+ _X509_NAME_add_entry
%xdefine _X509_NAME_add_entry_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_NID
%xdefine _X509_NAME_add_entry_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_OBJ
%xdefine _X509_NAME_add_entry_by_txt _ %+ BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_txt
%xdefine _X509_NAME_cmp _ %+ BORINGSSL_PREFIX %+ _X509_NAME_cmp
%xdefine _X509_NAME_delete_entry _ %+ BORINGSSL_PREFIX %+ _X509_NAME_delete_entry
%xdefine _X509_NAME_digest _ %+ BORINGSSL_PREFIX %+ _X509_NAME_digest
%xdefine _X509_NAME_dup _ %+ BORINGSSL_PREFIX %+ _X509_NAME_dup
%xdefine _X509_NAME_entry_count _ %+ BORINGSSL_PREFIX %+ _X509_NAME_entry_count
%xdefine _X509_NAME_free _ %+ BORINGSSL_PREFIX %+ _X509_NAME_free
%xdefine _X509_NAME_get0_der _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get0_der
%xdefine _X509_NAME_get_entry _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get_entry
%xdefine _X509_NAME_get_index_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get_index_by_NID
%xdefine _X509_NAME_get_index_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get_index_by_OBJ
%xdefine _X509_NAME_get_text_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get_text_by_NID
%xdefine _X509_NAME_get_text_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_NAME_get_text_by_OBJ
%xdefine _X509_NAME_hash _ %+ BORINGSSL_PREFIX %+ _X509_NAME_hash
%xdefine _X509_NAME_hash_old _ %+ BORINGSSL_PREFIX %+ _X509_NAME_hash_old
%xdefine _X509_NAME_it _ %+ BORINGSSL_PREFIX %+ _X509_NAME_it
%xdefine _X509_NAME_new _ %+ BORINGSSL_PREFIX %+ _X509_NAME_new
%xdefine _X509_NAME_oneline _ %+ BORINGSSL_PREFIX %+ _X509_NAME_oneline
%xdefine _X509_NAME_print _ %+ BORINGSSL_PREFIX %+ _X509_NAME_print
%xdefine _X509_NAME_print_ex _ %+ BORINGSSL_PREFIX %+ _X509_NAME_print_ex
%xdefine _X509_NAME_print_ex_fp _ %+ BORINGSSL_PREFIX %+ _X509_NAME_print_ex_fp
%xdefine _X509_NAME_set _ %+ BORINGSSL_PREFIX %+ _X509_NAME_set
%xdefine _X509_OBJECT_free _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_free
%xdefine _X509_OBJECT_free_contents _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_free_contents
%xdefine _X509_OBJECT_get0_X509 _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_get0_X509
%xdefine _X509_OBJECT_get0_X509_CRL _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_get0_X509_CRL
%xdefine _X509_OBJECT_get_type _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_get_type
%xdefine _X509_OBJECT_new _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_new
%xdefine _X509_OBJECT_set1_X509 _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_set1_X509
%xdefine _X509_OBJECT_set1_X509_CRL _ %+ BORINGSSL_PREFIX %+ _X509_OBJECT_set1_X509_CRL
%xdefine _X509_PUBKEY_free _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_free
%xdefine _X509_PUBKEY_get _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_get
%xdefine _X509_PUBKEY_get0 _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_get0
%xdefine _X509_PUBKEY_get0_param _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_get0_param
%xdefine _X509_PUBKEY_get0_public_key _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_get0_public_key
%xdefine _X509_PUBKEY_it _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_it
%xdefine _X509_PUBKEY_new _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_new
%xdefine _X509_PUBKEY_set _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_set
%xdefine _X509_PUBKEY_set0_param _ %+ BORINGSSL_PREFIX %+ _X509_PUBKEY_set0_param
%xdefine _X509_PURPOSE_get0 _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get0
%xdefine _X509_PURPOSE_get0_name _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get0_name
%xdefine _X509_PURPOSE_get0_sname _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get0_sname
%xdefine _X509_PURPOSE_get_by_id _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get_by_id
%xdefine _X509_PURPOSE_get_by_sname _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get_by_sname
%xdefine _X509_PURPOSE_get_count _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get_count
%xdefine _X509_PURPOSE_get_id _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get_id
%xdefine _X509_PURPOSE_get_trust _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_get_trust
%xdefine _X509_PURPOSE_set _ %+ BORINGSSL_PREFIX %+ _X509_PURPOSE_set
%xdefine _X509_REQ_INFO_free _ %+ BORINGSSL_PREFIX %+ _X509_REQ_INFO_free
%xdefine _X509_REQ_INFO_it _ %+ BORINGSSL_PREFIX %+ _X509_REQ_INFO_it
%xdefine _X509_REQ_INFO_new _ %+ BORINGSSL_PREFIX %+ _X509_REQ_INFO_new
%xdefine _X509_REQ_add1_attr _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add1_attr
%xdefine _X509_REQ_add1_attr_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_NID
%xdefine _X509_REQ_add1_attr_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_OBJ
%xdefine _X509_REQ_add1_attr_by_txt _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_txt
%xdefine _X509_REQ_add_extensions _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add_extensions
%xdefine _X509_REQ_add_extensions_nid _ %+ BORINGSSL_PREFIX %+ _X509_REQ_add_extensions_nid
%xdefine _X509_REQ_check_private_key _ %+ BORINGSSL_PREFIX %+ _X509_REQ_check_private_key
%xdefine _X509_REQ_delete_attr _ %+ BORINGSSL_PREFIX %+ _X509_REQ_delete_attr
%xdefine _X509_REQ_digest _ %+ BORINGSSL_PREFIX %+ _X509_REQ_digest
%xdefine _X509_REQ_dup _ %+ BORINGSSL_PREFIX %+ _X509_REQ_dup
%xdefine _X509_REQ_extension_nid _ %+ BORINGSSL_PREFIX %+ _X509_REQ_extension_nid
%xdefine _X509_REQ_free _ %+ BORINGSSL_PREFIX %+ _X509_REQ_free
%xdefine _X509_REQ_get0_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get0_pubkey
%xdefine _X509_REQ_get0_signature _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get0_signature
%xdefine _X509_REQ_get1_email _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get1_email
%xdefine _X509_REQ_get_attr _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_attr
%xdefine _X509_REQ_get_attr_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_attr_by_NID
%xdefine _X509_REQ_get_attr_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_attr_by_OBJ
%xdefine _X509_REQ_get_attr_count _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_attr_count
%xdefine _X509_REQ_get_extensions _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_extensions
%xdefine _X509_REQ_get_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_pubkey
%xdefine _X509_REQ_get_signature_nid _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_signature_nid
%xdefine _X509_REQ_get_subject_name _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_subject_name
%xdefine _X509_REQ_get_version _ %+ BORINGSSL_PREFIX %+ _X509_REQ_get_version
%xdefine _X509_REQ_it _ %+ BORINGSSL_PREFIX %+ _X509_REQ_it
%xdefine _X509_REQ_new _ %+ BORINGSSL_PREFIX %+ _X509_REQ_new
%xdefine _X509_REQ_print _ %+ BORINGSSL_PREFIX %+ _X509_REQ_print
%xdefine _X509_REQ_print_ex _ %+ BORINGSSL_PREFIX %+ _X509_REQ_print_ex
%xdefine _X509_REQ_print_fp _ %+ BORINGSSL_PREFIX %+ _X509_REQ_print_fp
%xdefine _X509_REQ_set1_signature_algo _ %+ BORINGSSL_PREFIX %+ _X509_REQ_set1_signature_algo
%xdefine _X509_REQ_set1_signature_value _ %+ BORINGSSL_PREFIX %+ _X509_REQ_set1_signature_value
%xdefine _X509_REQ_set_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_REQ_set_pubkey
%xdefine _X509_REQ_set_subject_name _ %+ BORINGSSL_PREFIX %+ _X509_REQ_set_subject_name
%xdefine _X509_REQ_set_version _ %+ BORINGSSL_PREFIX %+ _X509_REQ_set_version
%xdefine _X509_REQ_sign _ %+ BORINGSSL_PREFIX %+ _X509_REQ_sign
%xdefine _X509_REQ_sign_ctx _ %+ BORINGSSL_PREFIX %+ _X509_REQ_sign_ctx
%xdefine _X509_REQ_verify _ %+ BORINGSSL_PREFIX %+ _X509_REQ_verify
%xdefine _X509_REVOKED_add1_ext_i2d _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_add1_ext_i2d
%xdefine _X509_REVOKED_add_ext _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_add_ext
%xdefine _X509_REVOKED_delete_ext _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_delete_ext
%xdefine _X509_REVOKED_dup _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_dup
%xdefine _X509_REVOKED_free _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_free
%xdefine _X509_REVOKED_get0_extensions _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get0_extensions
%xdefine _X509_REVOKED_get0_revocationDate _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get0_revocationDate
%xdefine _X509_REVOKED_get0_serialNumber _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get0_serialNumber
%xdefine _X509_REVOKED_get_ext _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext
%xdefine _X509_REVOKED_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_NID
%xdefine _X509_REVOKED_get_ext_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_OBJ
%xdefine _X509_REVOKED_get_ext_by_critical _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_critical
%xdefine _X509_REVOKED_get_ext_count _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_count
%xdefine _X509_REVOKED_get_ext_d2i _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_d2i
%xdefine _X509_REVOKED_it _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_it
%xdefine _X509_REVOKED_new _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_new
%xdefine _X509_REVOKED_set_revocationDate _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_set_revocationDate
%xdefine _X509_REVOKED_set_serialNumber _ %+ BORINGSSL_PREFIX %+ _X509_REVOKED_set_serialNumber
%xdefine _X509_SIG_free _ %+ BORINGSSL_PREFIX %+ _X509_SIG_free
%xdefine _X509_SIG_get0 _ %+ BORINGSSL_PREFIX %+ _X509_SIG_get0
%xdefine _X509_SIG_getm _ %+ BORINGSSL_PREFIX %+ _X509_SIG_getm
%xdefine _X509_SIG_it _ %+ BORINGSSL_PREFIX %+ _X509_SIG_it
%xdefine _X509_SIG_new _ %+ BORINGSSL_PREFIX %+ _X509_SIG_new
%xdefine _X509_STORE_CTX_add_custom_crit_oid _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_add_custom_crit_oid
%xdefine _X509_STORE_CTX_cleanup _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_cleanup
%xdefine _X509_STORE_CTX_free _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_free
%xdefine _X509_STORE_CTX_get0_cert _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_cert
%xdefine _X509_STORE_CTX_get0_chain _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_chain
%xdefine _X509_STORE_CTX_get0_current_crl _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_current_crl
%xdefine _X509_STORE_CTX_get0_current_issuer _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_current_issuer
%xdefine _X509_STORE_CTX_get0_param _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_param
%xdefine _X509_STORE_CTX_get0_parent_ctx _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_parent_ctx
%xdefine _X509_STORE_CTX_get0_store _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_store
%xdefine _X509_STORE_CTX_get0_untrusted _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_untrusted
%xdefine _X509_STORE_CTX_get1_certs _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_certs
%xdefine _X509_STORE_CTX_get1_chain _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_chain
%xdefine _X509_STORE_CTX_get1_crls _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_crls
%xdefine _X509_STORE_CTX_get1_issuer _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_issuer
%xdefine _X509_STORE_CTX_get_by_subject _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_by_subject
%xdefine _X509_STORE_CTX_get_chain _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_chain
%xdefine _X509_STORE_CTX_get_current_cert _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_current_cert
%xdefine _X509_STORE_CTX_get_error _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_error
%xdefine _X509_STORE_CTX_get_error_depth _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_error_depth
%xdefine _X509_STORE_CTX_get_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_ex_data
%xdefine _X509_STORE_CTX_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_ex_new_index
%xdefine _X509_STORE_CTX_init _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_init
%xdefine _X509_STORE_CTX_new _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_new
%xdefine _X509_STORE_CTX_set0_crls _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_crls
%xdefine _X509_STORE_CTX_set0_param _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_param
%xdefine _X509_STORE_CTX_set0_trusted_stack _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_trusted_stack
%xdefine _X509_STORE_CTX_set0_untrusted _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_untrusted
%xdefine _X509_STORE_CTX_set_cert _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_cert
%xdefine _X509_STORE_CTX_set_chain _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_chain
%xdefine _X509_STORE_CTX_set_default _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_default
%xdefine _X509_STORE_CTX_set_depth _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_depth
%xdefine _X509_STORE_CTX_set_error _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_error
%xdefine _X509_STORE_CTX_set_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_ex_data
%xdefine _X509_STORE_CTX_set_flags _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_flags
%xdefine _X509_STORE_CTX_set_purpose _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_purpose
%xdefine _X509_STORE_CTX_set_time _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_time
%xdefine _X509_STORE_CTX_set_time_posix _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_time_posix
%xdefine _X509_STORE_CTX_set_trust _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_trust
%xdefine _X509_STORE_CTX_set_verify_cb _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_verify_cb
%xdefine _X509_STORE_CTX_set_verify_crit_oids _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_verify_crit_oids
%xdefine _X509_STORE_CTX_trusted_stack _ %+ BORINGSSL_PREFIX %+ _X509_STORE_CTX_trusted_stack
%xdefine _X509_STORE_add_cert _ %+ BORINGSSL_PREFIX %+ _X509_STORE_add_cert
%xdefine _X509_STORE_add_crl _ %+ BORINGSSL_PREFIX %+ _X509_STORE_add_crl
%xdefine _X509_STORE_add_lookup _ %+ BORINGSSL_PREFIX %+ _X509_STORE_add_lookup
%xdefine _X509_STORE_free _ %+ BORINGSSL_PREFIX %+ _X509_STORE_free
%xdefine _X509_STORE_get0_objects _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get0_objects
%xdefine _X509_STORE_get0_param _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get0_param
%xdefine _X509_STORE_get_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get_ex_data
%xdefine _X509_STORE_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get_ex_new_index
%xdefine _X509_STORE_get_lookup_crls _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get_lookup_crls
%xdefine _X509_STORE_get_verify_cb _ %+ BORINGSSL_PREFIX %+ _X509_STORE_get_verify_cb
%xdefine _X509_STORE_load_locations _ %+ BORINGSSL_PREFIX %+ _X509_STORE_load_locations
%xdefine _X509_STORE_lock _ %+ BORINGSSL_PREFIX %+ _X509_STORE_lock
%xdefine _X509_STORE_new _ %+ BORINGSSL_PREFIX %+ _X509_STORE_new
%xdefine _X509_STORE_set1_param _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set1_param
%xdefine _X509_STORE_set_check_crl _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_check_crl
%xdefine _X509_STORE_set_default_paths _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_default_paths
%xdefine _X509_STORE_set_depth _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_depth
%xdefine _X509_STORE_set_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_ex_data
%xdefine _X509_STORE_set_flags _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_flags
%xdefine _X509_STORE_set_get_crl _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_get_crl
%xdefine _X509_STORE_set_lookup_crls _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_lookup_crls
%xdefine _X509_STORE_set_purpose _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_purpose
%xdefine _X509_STORE_set_trust _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_trust
%xdefine _X509_STORE_set_verify_cb _ %+ BORINGSSL_PREFIX %+ _X509_STORE_set_verify_cb
%xdefine _X509_STORE_unlock _ %+ BORINGSSL_PREFIX %+ _X509_STORE_unlock
%xdefine _X509_STORE_up_ref _ %+ BORINGSSL_PREFIX %+ _X509_STORE_up_ref
%xdefine _X509_TRUST_cleanup _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_cleanup
%xdefine _X509_TRUST_get0 _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get0
%xdefine _X509_TRUST_get0_name _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get0_name
%xdefine _X509_TRUST_get_by_id _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get_by_id
%xdefine _X509_TRUST_get_count _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get_count
%xdefine _X509_TRUST_get_flags _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get_flags
%xdefine _X509_TRUST_get_trust _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_get_trust
%xdefine _X509_TRUST_set _ %+ BORINGSSL_PREFIX %+ _X509_TRUST_set
%xdefine _X509_VAL_free _ %+ BORINGSSL_PREFIX %+ _X509_VAL_free
%xdefine _X509_VAL_it _ %+ BORINGSSL_PREFIX %+ _X509_VAL_it
%xdefine _X509_VAL_new _ %+ BORINGSSL_PREFIX %+ _X509_VAL_new
%xdefine _X509_VERIFY_PARAM_add0_policy _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_add0_policy
%xdefine _X509_VERIFY_PARAM_add1_host _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_add1_host
%xdefine _X509_VERIFY_PARAM_clear_flags _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_clear_flags
%xdefine _X509_VERIFY_PARAM_disable_ec_key_explicit_params _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_disable_ec_key_explicit_params
%xdefine _X509_VERIFY_PARAM_enable_ec_key_explicit_params _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_enable_ec_key_explicit_params
%xdefine _X509_VERIFY_PARAM_free _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_free
%xdefine _X509_VERIFY_PARAM_get_depth _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_depth
%xdefine _X509_VERIFY_PARAM_get_flags _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_flags
%xdefine _X509_VERIFY_PARAM_get_hostflags _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_hostflags
%xdefine _X509_VERIFY_PARAM_inherit _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_inherit
%xdefine _X509_VERIFY_PARAM_lookup _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_lookup
%xdefine _X509_VERIFY_PARAM_new _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_new
%xdefine _X509_VERIFY_PARAM_set1 _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1
%xdefine _X509_VERIFY_PARAM_set1_email _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_email
%xdefine _X509_VERIFY_PARAM_set1_host _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_host
%xdefine _X509_VERIFY_PARAM_set1_ip _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_ip
%xdefine _X509_VERIFY_PARAM_set1_ip_asc _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_ip_asc
%xdefine _X509_VERIFY_PARAM_set1_policies _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_policies
%xdefine _X509_VERIFY_PARAM_set_depth _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_depth
%xdefine _X509_VERIFY_PARAM_set_flags _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_flags
%xdefine _X509_VERIFY_PARAM_set_hostflags _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_hostflags
%xdefine _X509_VERIFY_PARAM_set_purpose _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_purpose
%xdefine _X509_VERIFY_PARAM_set_time _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_time
%xdefine _X509_VERIFY_PARAM_set_time_posix _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_time_posix
%xdefine _X509_VERIFY_PARAM_set_trust _ %+ BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_trust
%xdefine _X509_add1_ext_i2d _ %+ BORINGSSL_PREFIX %+ _X509_add1_ext_i2d
%xdefine _X509_add1_reject_object _ %+ BORINGSSL_PREFIX %+ _X509_add1_reject_object
%xdefine _X509_add1_trust_object _ %+ BORINGSSL_PREFIX %+ _X509_add1_trust_object
%xdefine _X509_add_ext _ %+ BORINGSSL_PREFIX %+ _X509_add_ext
%xdefine _X509_alias_get0 _ %+ BORINGSSL_PREFIX %+ _X509_alias_get0
%xdefine _X509_alias_set1 _ %+ BORINGSSL_PREFIX %+ _X509_alias_set1
%xdefine _X509_chain_up_ref _ %+ BORINGSSL_PREFIX %+ _X509_chain_up_ref
%xdefine _X509_check_akid _ %+ BORINGSSL_PREFIX %+ _X509_check_akid
%xdefine _X509_check_ca _ %+ BORINGSSL_PREFIX %+ _X509_check_ca
%xdefine _X509_check_email _ %+ BORINGSSL_PREFIX %+ _X509_check_email
%xdefine _X509_check_host _ %+ BORINGSSL_PREFIX %+ _X509_check_host
%xdefine _X509_check_ip _ %+ BORINGSSL_PREFIX %+ _X509_check_ip
%xdefine _X509_check_ip_asc _ %+ BORINGSSL_PREFIX %+ _X509_check_ip_asc
%xdefine _X509_check_issued _ %+ BORINGSSL_PREFIX %+ _X509_check_issued
%xdefine _X509_check_private_key _ %+ BORINGSSL_PREFIX %+ _X509_check_private_key
%xdefine _X509_check_purpose _ %+ BORINGSSL_PREFIX %+ _X509_check_purpose
%xdefine _X509_check_trust _ %+ BORINGSSL_PREFIX %+ _X509_check_trust
%xdefine _X509_cmp _ %+ BORINGSSL_PREFIX %+ _X509_cmp
%xdefine _X509_cmp_current_time _ %+ BORINGSSL_PREFIX %+ _X509_cmp_current_time
%xdefine _X509_cmp_time _ %+ BORINGSSL_PREFIX %+ _X509_cmp_time
%xdefine _X509_cmp_time_posix _ %+ BORINGSSL_PREFIX %+ _X509_cmp_time_posix
%xdefine _X509_delete_ext _ %+ BORINGSSL_PREFIX %+ _X509_delete_ext
%xdefine _X509_digest _ %+ BORINGSSL_PREFIX %+ _X509_digest
%xdefine _X509_dup _ %+ BORINGSSL_PREFIX %+ _X509_dup
%xdefine _X509_email_free _ %+ BORINGSSL_PREFIX %+ _X509_email_free
%xdefine _X509_find_by_issuer_and_serial _ %+ BORINGSSL_PREFIX %+ _X509_find_by_issuer_and_serial
%xdefine _X509_find_by_subject _ %+ BORINGSSL_PREFIX %+ _X509_find_by_subject
%xdefine _X509_free _ %+ BORINGSSL_PREFIX %+ _X509_free
%xdefine _X509_get0_authority_issuer _ %+ BORINGSSL_PREFIX %+ _X509_get0_authority_issuer
%xdefine _X509_get0_authority_key_id _ %+ BORINGSSL_PREFIX %+ _X509_get0_authority_key_id
%xdefine _X509_get0_authority_serial _ %+ BORINGSSL_PREFIX %+ _X509_get0_authority_serial
%xdefine _X509_get0_extensions _ %+ BORINGSSL_PREFIX %+ _X509_get0_extensions
%xdefine _X509_get0_notAfter _ %+ BORINGSSL_PREFIX %+ _X509_get0_notAfter
%xdefine _X509_get0_notBefore _ %+ BORINGSSL_PREFIX %+ _X509_get0_notBefore
%xdefine _X509_get0_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_get0_pubkey
%xdefine _X509_get0_pubkey_bitstr _ %+ BORINGSSL_PREFIX %+ _X509_get0_pubkey_bitstr
%xdefine _X509_get0_serialNumber _ %+ BORINGSSL_PREFIX %+ _X509_get0_serialNumber
%xdefine _X509_get0_signature _ %+ BORINGSSL_PREFIX %+ _X509_get0_signature
%xdefine _X509_get0_subject_key_id _ %+ BORINGSSL_PREFIX %+ _X509_get0_subject_key_id
%xdefine _X509_get0_tbs_sigalg _ %+ BORINGSSL_PREFIX %+ _X509_get0_tbs_sigalg
%xdefine _X509_get0_uids _ %+ BORINGSSL_PREFIX %+ _X509_get0_uids
%xdefine _X509_get1_email _ %+ BORINGSSL_PREFIX %+ _X509_get1_email
%xdefine _X509_get1_ocsp _ %+ BORINGSSL_PREFIX %+ _X509_get1_ocsp
%xdefine _X509_get_X509_PUBKEY _ %+ BORINGSSL_PREFIX %+ _X509_get_X509_PUBKEY
%xdefine _X509_get_default_cert_area _ %+ BORINGSSL_PREFIX %+ _X509_get_default_cert_area
%xdefine _X509_get_default_cert_dir _ %+ BORINGSSL_PREFIX %+ _X509_get_default_cert_dir
%xdefine _X509_get_default_cert_dir_env _ %+ BORINGSSL_PREFIX %+ _X509_get_default_cert_dir_env
%xdefine _X509_get_default_cert_file _ %+ BORINGSSL_PREFIX %+ _X509_get_default_cert_file
%xdefine _X509_get_default_cert_file_env _ %+ BORINGSSL_PREFIX %+ _X509_get_default_cert_file_env
%xdefine _X509_get_default_private_dir _ %+ BORINGSSL_PREFIX %+ _X509_get_default_private_dir
%xdefine _X509_get_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_get_ex_data
%xdefine _X509_get_ex_new_index _ %+ BORINGSSL_PREFIX %+ _X509_get_ex_new_index
%xdefine _X509_get_ext _ %+ BORINGSSL_PREFIX %+ _X509_get_ext
%xdefine _X509_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _X509_get_ext_by_NID
%xdefine _X509_get_ext_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509_get_ext_by_OBJ
%xdefine _X509_get_ext_by_critical _ %+ BORINGSSL_PREFIX %+ _X509_get_ext_by_critical
%xdefine _X509_get_ext_count _ %+ BORINGSSL_PREFIX %+ _X509_get_ext_count
%xdefine _X509_get_ext_d2i _ %+ BORINGSSL_PREFIX %+ _X509_get_ext_d2i
%xdefine _X509_get_extended_key_usage _ %+ BORINGSSL_PREFIX %+ _X509_get_extended_key_usage
%xdefine _X509_get_extension_flags _ %+ BORINGSSL_PREFIX %+ _X509_get_extension_flags
%xdefine _X509_get_issuer_name _ %+ BORINGSSL_PREFIX %+ _X509_get_issuer_name
%xdefine _X509_get_key_usage _ %+ BORINGSSL_PREFIX %+ _X509_get_key_usage
%xdefine _X509_get_notAfter _ %+ BORINGSSL_PREFIX %+ _X509_get_notAfter
%xdefine _X509_get_notBefore _ %+ BORINGSSL_PREFIX %+ _X509_get_notBefore
%xdefine _X509_get_pathlen _ %+ BORINGSSL_PREFIX %+ _X509_get_pathlen
%xdefine _X509_get_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_get_pubkey
%xdefine _X509_get_serialNumber _ %+ BORINGSSL_PREFIX %+ _X509_get_serialNumber
%xdefine _X509_get_signature_info _ %+ BORINGSSL_PREFIX %+ _X509_get_signature_info
%xdefine _X509_get_signature_nid _ %+ BORINGSSL_PREFIX %+ _X509_get_signature_nid
%xdefine _X509_get_subject_name _ %+ BORINGSSL_PREFIX %+ _X509_get_subject_name
%xdefine _X509_get_version _ %+ BORINGSSL_PREFIX %+ _X509_get_version
%xdefine _X509_getm_notAfter _ %+ BORINGSSL_PREFIX %+ _X509_getm_notAfter
%xdefine _X509_getm_notBefore _ %+ BORINGSSL_PREFIX %+ _X509_getm_notBefore
%xdefine _X509_gmtime_adj _ %+ BORINGSSL_PREFIX %+ _X509_gmtime_adj
%xdefine _X509_issuer_name_cmp _ %+ BORINGSSL_PREFIX %+ _X509_issuer_name_cmp
%xdefine _X509_issuer_name_hash _ %+ BORINGSSL_PREFIX %+ _X509_issuer_name_hash
%xdefine _X509_issuer_name_hash_old _ %+ BORINGSSL_PREFIX %+ _X509_issuer_name_hash_old
%xdefine _X509_it _ %+ BORINGSSL_PREFIX %+ _X509_it
%xdefine _X509_keyid_get0 _ %+ BORINGSSL_PREFIX %+ _X509_keyid_get0
%xdefine _X509_keyid_set1 _ %+ BORINGSSL_PREFIX %+ _X509_keyid_set1
%xdefine _X509_load_cert_crl_file _ %+ BORINGSSL_PREFIX %+ _X509_load_cert_crl_file
%xdefine _X509_load_cert_file _ %+ BORINGSSL_PREFIX %+ _X509_load_cert_file
%xdefine _X509_load_crl_file _ %+ BORINGSSL_PREFIX %+ _X509_load_crl_file
%xdefine _X509_new _ %+ BORINGSSL_PREFIX %+ _X509_new
%xdefine _X509_parse_from_buffer _ %+ BORINGSSL_PREFIX %+ _X509_parse_from_buffer
%xdefine _X509_policy_check _ %+ BORINGSSL_PREFIX %+ _X509_policy_check
%xdefine _X509_print _ %+ BORINGSSL_PREFIX %+ _X509_print
%xdefine _X509_print_ex _ %+ BORINGSSL_PREFIX %+ _X509_print_ex
%xdefine _X509_print_ex_fp _ %+ BORINGSSL_PREFIX %+ _X509_print_ex_fp
%xdefine _X509_print_fp _ %+ BORINGSSL_PREFIX %+ _X509_print_fp
%xdefine _X509_pubkey_digest _ %+ BORINGSSL_PREFIX %+ _X509_pubkey_digest
%xdefine _X509_reject_clear _ %+ BORINGSSL_PREFIX %+ _X509_reject_clear
%xdefine _X509_set1_notAfter _ %+ BORINGSSL_PREFIX %+ _X509_set1_notAfter
%xdefine _X509_set1_notBefore _ %+ BORINGSSL_PREFIX %+ _X509_set1_notBefore
%xdefine _X509_set1_signature_algo _ %+ BORINGSSL_PREFIX %+ _X509_set1_signature_algo
%xdefine _X509_set1_signature_value _ %+ BORINGSSL_PREFIX %+ _X509_set1_signature_value
%xdefine _X509_set_ex_data _ %+ BORINGSSL_PREFIX %+ _X509_set_ex_data
%xdefine _X509_set_issuer_name _ %+ BORINGSSL_PREFIX %+ _X509_set_issuer_name
%xdefine _X509_set_notAfter _ %+ BORINGSSL_PREFIX %+ _X509_set_notAfter
%xdefine _X509_set_notBefore _ %+ BORINGSSL_PREFIX %+ _X509_set_notBefore
%xdefine _X509_set_pubkey _ %+ BORINGSSL_PREFIX %+ _X509_set_pubkey
%xdefine _X509_set_serialNumber _ %+ BORINGSSL_PREFIX %+ _X509_set_serialNumber
%xdefine _X509_set_subject_name _ %+ BORINGSSL_PREFIX %+ _X509_set_subject_name
%xdefine _X509_set_version _ %+ BORINGSSL_PREFIX %+ _X509_set_version
%xdefine _X509_sign _ %+ BORINGSSL_PREFIX %+ _X509_sign
%xdefine _X509_sign_ctx _ %+ BORINGSSL_PREFIX %+ _X509_sign_ctx
%xdefine _X509_signature_dump _ %+ BORINGSSL_PREFIX %+ _X509_signature_dump
%xdefine _X509_signature_print _ %+ BORINGSSL_PREFIX %+ _X509_signature_print
%xdefine _X509_subject_name_cmp _ %+ BORINGSSL_PREFIX %+ _X509_subject_name_cmp
%xdefine _X509_subject_name_hash _ %+ BORINGSSL_PREFIX %+ _X509_subject_name_hash
%xdefine _X509_subject_name_hash_old _ %+ BORINGSSL_PREFIX %+ _X509_subject_name_hash_old
%xdefine _X509_supported_extension _ %+ BORINGSSL_PREFIX %+ _X509_supported_extension
%xdefine _X509_time_adj _ %+ BORINGSSL_PREFIX %+ _X509_time_adj
%xdefine _X509_time_adj_ex _ %+ BORINGSSL_PREFIX %+ _X509_time_adj_ex
%xdefine _X509_trust_clear _ %+ BORINGSSL_PREFIX %+ _X509_trust_clear
%xdefine _X509_up_ref _ %+ BORINGSSL_PREFIX %+ _X509_up_ref
%xdefine _X509_verify _ %+ BORINGSSL_PREFIX %+ _X509_verify
%xdefine _X509_verify_cert _ %+ BORINGSSL_PREFIX %+ _X509_verify_cert
%xdefine _X509_verify_cert_error_string _ %+ BORINGSSL_PREFIX %+ _X509_verify_cert_error_string
%xdefine _X509v3_add_ext _ %+ BORINGSSL_PREFIX %+ _X509v3_add_ext
%xdefine _X509v3_delete_ext _ %+ BORINGSSL_PREFIX %+ _X509v3_delete_ext
%xdefine _X509v3_get_ext _ %+ BORINGSSL_PREFIX %+ _X509v3_get_ext
%xdefine _X509v3_get_ext_by_NID _ %+ BORINGSSL_PREFIX %+ _X509v3_get_ext_by_NID
%xdefine _X509v3_get_ext_by_OBJ _ %+ BORINGSSL_PREFIX %+ _X509v3_get_ext_by_OBJ
%xdefine _X509v3_get_ext_by_critical _ %+ BORINGSSL_PREFIX %+ _X509v3_get_ext_by_critical
%xdefine _X509v3_get_ext_count _ %+ BORINGSSL_PREFIX %+ _X509v3_get_ext_count
%xdefine ___local_stdio_printf_options _ %+ BORINGSSL_PREFIX %+ ___local_stdio_printf_options
%xdefine ___local_stdio_scanf_options _ %+ BORINGSSL_PREFIX %+ ___local_stdio_scanf_options
%xdefine _a2i_IPADDRESS _ %+ BORINGSSL_PREFIX %+ _a2i_IPADDRESS
%xdefine _a2i_IPADDRESS_NC _ %+ BORINGSSL_PREFIX %+ _a2i_IPADDRESS_NC
%xdefine _abi_test_bad_unwind_epilog _ %+ BORINGSSL_PREFIX %+ _abi_test_bad_unwind_epilog
%xdefine _abi_test_bad_unwind_temporary _ %+ BORINGSSL_PREFIX %+ _abi_test_bad_unwind_temporary
%xdefine _abi_test_bad_unwind_wrong_register _ %+ BORINGSSL_PREFIX %+ _abi_test_bad_unwind_wrong_register
%xdefine _abi_test_clobber_cr0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr0
%xdefine _abi_test_clobber_cr1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr1
%xdefine _abi_test_clobber_cr2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr2
%xdefine _abi_test_clobber_cr3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr3
%xdefine _abi_test_clobber_cr4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr4
%xdefine _abi_test_clobber_cr5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr5
%xdefine _abi_test_clobber_cr6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr6
%xdefine _abi_test_clobber_cr7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_cr7
%xdefine _abi_test_clobber_ctr _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_ctr
%xdefine _abi_test_clobber_d0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d0
%xdefine _abi_test_clobber_d1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d1
%xdefine _abi_test_clobber_d10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d10
%xdefine _abi_test_clobber_d11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d11
%xdefine _abi_test_clobber_d12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d12
%xdefine _abi_test_clobber_d13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d13
%xdefine _abi_test_clobber_d14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d14
%xdefine _abi_test_clobber_d15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d15
%xdefine _abi_test_clobber_d16 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d16
%xdefine _abi_test_clobber_d17 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d17
%xdefine _abi_test_clobber_d18 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d18
%xdefine _abi_test_clobber_d19 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d19
%xdefine _abi_test_clobber_d2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d2
%xdefine _abi_test_clobber_d20 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d20
%xdefine _abi_test_clobber_d21 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d21
%xdefine _abi_test_clobber_d22 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d22
%xdefine _abi_test_clobber_d23 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d23
%xdefine _abi_test_clobber_d24 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d24
%xdefine _abi_test_clobber_d25 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d25
%xdefine _abi_test_clobber_d26 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d26
%xdefine _abi_test_clobber_d27 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d27
%xdefine _abi_test_clobber_d28 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d28
%xdefine _abi_test_clobber_d29 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d29
%xdefine _abi_test_clobber_d3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d3
%xdefine _abi_test_clobber_d30 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d30
%xdefine _abi_test_clobber_d31 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d31
%xdefine _abi_test_clobber_d4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d4
%xdefine _abi_test_clobber_d5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d5
%xdefine _abi_test_clobber_d6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d6
%xdefine _abi_test_clobber_d7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d7
%xdefine _abi_test_clobber_d8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d8
%xdefine _abi_test_clobber_d9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_d9
%xdefine _abi_test_clobber_eax _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_eax
%xdefine _abi_test_clobber_ebp _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_ebp
%xdefine _abi_test_clobber_ebx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_ebx
%xdefine _abi_test_clobber_ecx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_ecx
%xdefine _abi_test_clobber_edi _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_edi
%xdefine _abi_test_clobber_edx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_edx
%xdefine _abi_test_clobber_esi _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_esi
%xdefine _abi_test_clobber_f0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f0
%xdefine _abi_test_clobber_f1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f1
%xdefine _abi_test_clobber_f10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f10
%xdefine _abi_test_clobber_f11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f11
%xdefine _abi_test_clobber_f12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f12
%xdefine _abi_test_clobber_f13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f13
%xdefine _abi_test_clobber_f14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f14
%xdefine _abi_test_clobber_f15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f15
%xdefine _abi_test_clobber_f16 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f16
%xdefine _abi_test_clobber_f17 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f17
%xdefine _abi_test_clobber_f18 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f18
%xdefine _abi_test_clobber_f19 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f19
%xdefine _abi_test_clobber_f2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f2
%xdefine _abi_test_clobber_f20 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f20
%xdefine _abi_test_clobber_f21 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f21
%xdefine _abi_test_clobber_f22 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f22
%xdefine _abi_test_clobber_f23 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f23
%xdefine _abi_test_clobber_f24 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f24
%xdefine _abi_test_clobber_f25 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f25
%xdefine _abi_test_clobber_f26 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f26
%xdefine _abi_test_clobber_f27 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f27
%xdefine _abi_test_clobber_f28 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f28
%xdefine _abi_test_clobber_f29 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f29
%xdefine _abi_test_clobber_f3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f3
%xdefine _abi_test_clobber_f30 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f30
%xdefine _abi_test_clobber_f31 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f31
%xdefine _abi_test_clobber_f4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f4
%xdefine _abi_test_clobber_f5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f5
%xdefine _abi_test_clobber_f6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f6
%xdefine _abi_test_clobber_f7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f7
%xdefine _abi_test_clobber_f8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f8
%xdefine _abi_test_clobber_f9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_f9
%xdefine _abi_test_clobber_lr _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_lr
%xdefine _abi_test_clobber_r0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r0
%xdefine _abi_test_clobber_r1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r1
%xdefine _abi_test_clobber_r10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r10
%xdefine _abi_test_clobber_r11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r11
%xdefine _abi_test_clobber_r12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r12
%xdefine _abi_test_clobber_r13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r13
%xdefine _abi_test_clobber_r14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r14
%xdefine _abi_test_clobber_r15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r15
%xdefine _abi_test_clobber_r16 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r16
%xdefine _abi_test_clobber_r17 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r17
%xdefine _abi_test_clobber_r18 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r18
%xdefine _abi_test_clobber_r19 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r19
%xdefine _abi_test_clobber_r2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r2
%xdefine _abi_test_clobber_r20 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r20
%xdefine _abi_test_clobber_r21 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r21
%xdefine _abi_test_clobber_r22 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r22
%xdefine _abi_test_clobber_r23 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r23
%xdefine _abi_test_clobber_r24 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r24
%xdefine _abi_test_clobber_r25 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r25
%xdefine _abi_test_clobber_r26 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r26
%xdefine _abi_test_clobber_r27 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r27
%xdefine _abi_test_clobber_r28 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r28
%xdefine _abi_test_clobber_r29 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r29
%xdefine _abi_test_clobber_r3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r3
%xdefine _abi_test_clobber_r30 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r30
%xdefine _abi_test_clobber_r31 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r31
%xdefine _abi_test_clobber_r4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r4
%xdefine _abi_test_clobber_r5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r5
%xdefine _abi_test_clobber_r6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r6
%xdefine _abi_test_clobber_r7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r7
%xdefine _abi_test_clobber_r8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r8
%xdefine _abi_test_clobber_r9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_r9
%xdefine _abi_test_clobber_rax _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rax
%xdefine _abi_test_clobber_rbp _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rbp
%xdefine _abi_test_clobber_rbx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rbx
%xdefine _abi_test_clobber_rcx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rcx
%xdefine _abi_test_clobber_rdi _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rdi
%xdefine _abi_test_clobber_rdx _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rdx
%xdefine _abi_test_clobber_rsi _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_rsi
%xdefine _abi_test_clobber_v0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v0
%xdefine _abi_test_clobber_v1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v1
%xdefine _abi_test_clobber_v10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v10
%xdefine _abi_test_clobber_v10_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v10_upper
%xdefine _abi_test_clobber_v11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v11
%xdefine _abi_test_clobber_v11_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v11_upper
%xdefine _abi_test_clobber_v12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v12
%xdefine _abi_test_clobber_v12_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v12_upper
%xdefine _abi_test_clobber_v13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v13
%xdefine _abi_test_clobber_v13_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v13_upper
%xdefine _abi_test_clobber_v14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v14
%xdefine _abi_test_clobber_v14_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v14_upper
%xdefine _abi_test_clobber_v15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v15
%xdefine _abi_test_clobber_v15_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v15_upper
%xdefine _abi_test_clobber_v16 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v16
%xdefine _abi_test_clobber_v17 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v17
%xdefine _abi_test_clobber_v18 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v18
%xdefine _abi_test_clobber_v19 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v19
%xdefine _abi_test_clobber_v2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v2
%xdefine _abi_test_clobber_v20 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v20
%xdefine _abi_test_clobber_v21 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v21
%xdefine _abi_test_clobber_v22 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v22
%xdefine _abi_test_clobber_v23 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v23
%xdefine _abi_test_clobber_v24 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v24
%xdefine _abi_test_clobber_v25 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v25
%xdefine _abi_test_clobber_v26 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v26
%xdefine _abi_test_clobber_v27 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v27
%xdefine _abi_test_clobber_v28 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v28
%xdefine _abi_test_clobber_v29 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v29
%xdefine _abi_test_clobber_v3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v3
%xdefine _abi_test_clobber_v30 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v30
%xdefine _abi_test_clobber_v31 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v31
%xdefine _abi_test_clobber_v4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v4
%xdefine _abi_test_clobber_v5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v5
%xdefine _abi_test_clobber_v6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v6
%xdefine _abi_test_clobber_v7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v7
%xdefine _abi_test_clobber_v8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v8
%xdefine _abi_test_clobber_v8_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v8_upper
%xdefine _abi_test_clobber_v9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v9
%xdefine _abi_test_clobber_v9_upper _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_v9_upper
%xdefine _abi_test_clobber_x0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x0
%xdefine _abi_test_clobber_x1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x1
%xdefine _abi_test_clobber_x10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x10
%xdefine _abi_test_clobber_x11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x11
%xdefine _abi_test_clobber_x12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x12
%xdefine _abi_test_clobber_x13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x13
%xdefine _abi_test_clobber_x14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x14
%xdefine _abi_test_clobber_x15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x15
%xdefine _abi_test_clobber_x16 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x16
%xdefine _abi_test_clobber_x17 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x17
%xdefine _abi_test_clobber_x19 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x19
%xdefine _abi_test_clobber_x2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x2
%xdefine _abi_test_clobber_x20 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x20
%xdefine _abi_test_clobber_x21 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x21
%xdefine _abi_test_clobber_x22 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x22
%xdefine _abi_test_clobber_x23 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x23
%xdefine _abi_test_clobber_x24 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x24
%xdefine _abi_test_clobber_x25 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x25
%xdefine _abi_test_clobber_x26 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x26
%xdefine _abi_test_clobber_x27 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x27
%xdefine _abi_test_clobber_x28 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x28
%xdefine _abi_test_clobber_x29 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x29
%xdefine _abi_test_clobber_x3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x3
%xdefine _abi_test_clobber_x4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x4
%xdefine _abi_test_clobber_x5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x5
%xdefine _abi_test_clobber_x6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x6
%xdefine _abi_test_clobber_x7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x7
%xdefine _abi_test_clobber_x8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x8
%xdefine _abi_test_clobber_x9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_x9
%xdefine _abi_test_clobber_xmm0 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm0
%xdefine _abi_test_clobber_xmm1 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm1
%xdefine _abi_test_clobber_xmm10 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm10
%xdefine _abi_test_clobber_xmm11 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm11
%xdefine _abi_test_clobber_xmm12 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm12
%xdefine _abi_test_clobber_xmm13 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm13
%xdefine _abi_test_clobber_xmm14 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm14
%xdefine _abi_test_clobber_xmm15 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm15
%xdefine _abi_test_clobber_xmm2 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm2
%xdefine _abi_test_clobber_xmm3 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm3
%xdefine _abi_test_clobber_xmm4 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm4
%xdefine _abi_test_clobber_xmm5 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm5
%xdefine _abi_test_clobber_xmm6 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm6
%xdefine _abi_test_clobber_xmm7 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm7
%xdefine _abi_test_clobber_xmm8 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm8
%xdefine _abi_test_clobber_xmm9 _ %+ BORINGSSL_PREFIX %+ _abi_test_clobber_xmm9
%xdefine _abi_test_get_and_clear_direction_flag _ %+ BORINGSSL_PREFIX %+ _abi_test_get_and_clear_direction_flag
%xdefine _abi_test_set_direction_flag _ %+ BORINGSSL_PREFIX %+ _abi_test_set_direction_flag
%xdefine _abi_test_trampoline _ %+ BORINGSSL_PREFIX %+ _abi_test_trampoline
%xdefine _abi_test_unwind_return _ %+ BORINGSSL_PREFIX %+ _abi_test_unwind_return
%xdefine _abi_test_unwind_start _ %+ BORINGSSL_PREFIX %+ _abi_test_unwind_start
%xdefine _abi_test_unwind_stop _ %+ BORINGSSL_PREFIX %+ _abi_test_unwind_stop
%xdefine _aes128gcmsiv_aes_ks _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_aes_ks
%xdefine _aes128gcmsiv_aes_ks_enc_x1 _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_aes_ks_enc_x1
%xdefine _aes128gcmsiv_dec _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_dec
%xdefine _aes128gcmsiv_ecb_enc_block _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_ecb_enc_block
%xdefine _aes128gcmsiv_enc_msg_x4 _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_enc_msg_x4
%xdefine _aes128gcmsiv_enc_msg_x8 _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_enc_msg_x8
%xdefine _aes128gcmsiv_kdf _ %+ BORINGSSL_PREFIX %+ _aes128gcmsiv_kdf
%xdefine _aes256gcmsiv_aes_ks _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_aes_ks
%xdefine _aes256gcmsiv_aes_ks_enc_x1 _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_aes_ks_enc_x1
%xdefine _aes256gcmsiv_dec _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_dec
%xdefine _aes256gcmsiv_ecb_enc_block _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_ecb_enc_block
%xdefine _aes256gcmsiv_enc_msg_x4 _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_enc_msg_x4
%xdefine _aes256gcmsiv_enc_msg_x8 _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_enc_msg_x8
%xdefine _aes256gcmsiv_kdf _ %+ BORINGSSL_PREFIX %+ _aes256gcmsiv_kdf
%xdefine _aes_ctr_set_key _ %+ BORINGSSL_PREFIX %+ _aes_ctr_set_key
%xdefine _aes_gcm_dec_kernel _ %+ BORINGSSL_PREFIX %+ _aes_gcm_dec_kernel
%xdefine _aes_gcm_decrypt_avx512 _ %+ BORINGSSL_PREFIX %+ _aes_gcm_decrypt_avx512
%xdefine _aes_gcm_enc_kernel _ %+ BORINGSSL_PREFIX %+ _aes_gcm_enc_kernel
%xdefine _aes_gcm_encrypt_avx512 _ %+ BORINGSSL_PREFIX %+ _aes_gcm_encrypt_avx512
%xdefine _aes_hw_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_cbc_encrypt
%xdefine _aes_hw_ccm64_decrypt_blocks _ %+ BORINGSSL_PREFIX %+ _aes_hw_ccm64_decrypt_blocks
%xdefine _aes_hw_ccm64_encrypt_blocks _ %+ BORINGSSL_PREFIX %+ _aes_hw_ccm64_encrypt_blocks
%xdefine _aes_hw_ctr32_encrypt_blocks _ %+ BORINGSSL_PREFIX %+ _aes_hw_ctr32_encrypt_blocks
%xdefine _aes_hw_decrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_decrypt
%xdefine _aes_hw_ecb_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_ecb_encrypt
%xdefine _aes_hw_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_encrypt
%xdefine _aes_hw_set_decrypt_key _ %+ BORINGSSL_PREFIX %+ _aes_hw_set_decrypt_key
%xdefine _aes_hw_set_encrypt_key _ %+ BORINGSSL_PREFIX %+ _aes_hw_set_encrypt_key
%xdefine _aes_hw_xts_cipher _ %+ BORINGSSL_PREFIX %+ _aes_hw_xts_cipher
%xdefine _aes_hw_xts_decrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_xts_decrypt
%xdefine _aes_hw_xts_decrypt_avx512 _ %+ BORINGSSL_PREFIX %+ _aes_hw_xts_decrypt_avx512
%xdefine _aes_hw_xts_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_hw_xts_encrypt
%xdefine _aes_hw_xts_encrypt_avx512 _ %+ BORINGSSL_PREFIX %+ _aes_hw_xts_encrypt_avx512
%xdefine _aes_nohw_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_nohw_cbc_encrypt
%xdefine _aes_nohw_ctr32_encrypt_blocks _ %+ BORINGSSL_PREFIX %+ _aes_nohw_ctr32_encrypt_blocks
%xdefine _aes_nohw_decrypt _ %+ BORINGSSL_PREFIX %+ _aes_nohw_decrypt
%xdefine _aes_nohw_encrypt _ %+ BORINGSSL_PREFIX %+ _aes_nohw_encrypt
%xdefine _aes_nohw_set_decrypt_key _ %+ BORINGSSL_PREFIX %+ _aes_nohw_set_decrypt_key
%xdefine _aes_nohw_set_encrypt_key _ %+ BORINGSSL_PREFIX %+ _aes_nohw_set_encrypt_key
%xdefine _aesgcmsiv_htable6_init _ %+ BORINGSSL_PREFIX %+ _aesgcmsiv_htable6_init
%xdefine _aesgcmsiv_htable_init _ %+ BORINGSSL_PREFIX %+ _aesgcmsiv_htable_init
%xdefine _aesgcmsiv_htable_polyval _ %+ BORINGSSL_PREFIX %+ _aesgcmsiv_htable_polyval
%xdefine _aesgcmsiv_polyval_horner _ %+ BORINGSSL_PREFIX %+ _aesgcmsiv_polyval_horner
%xdefine _aesni_cbc_sha1_enc _ %+ BORINGSSL_PREFIX %+ _aesni_cbc_sha1_enc
%xdefine _aesni_cbc_sha256_enc _ %+ BORINGSSL_PREFIX %+ _aesni_cbc_sha256_enc
%xdefine _aesni_gcm_decrypt _ %+ BORINGSSL_PREFIX %+ _aesni_gcm_decrypt
%xdefine _aesni_gcm_encrypt _ %+ BORINGSSL_PREFIX %+ _aesni_gcm_encrypt
%xdefine _aesv8_gcm_8x_dec_128 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_128
%xdefine _aesv8_gcm_8x_dec_192 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_192
%xdefine _aesv8_gcm_8x_dec_256 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_256
%xdefine _aesv8_gcm_8x_enc_128 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_128
%xdefine _aesv8_gcm_8x_enc_192 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_192
%xdefine _aesv8_gcm_8x_enc_256 _ %+ BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_256
%xdefine _allow_mocked_ube_detection_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _allow_mocked_ube_detection_FOR_TESTING
%xdefine _armv8_disable_dit _ %+ BORINGSSL_PREFIX %+ _armv8_disable_dit
%xdefine _armv8_enable_dit _ %+ BORINGSSL_PREFIX %+ _armv8_enable_dit
%xdefine _armv8_get_dit _ %+ BORINGSSL_PREFIX %+ _armv8_get_dit
%xdefine _armv8_restore_dit _ %+ BORINGSSL_PREFIX %+ _armv8_restore_dit
%xdefine _armv8_set_dit _ %+ BORINGSSL_PREFIX %+ _armv8_set_dit
%xdefine _asn1_bit_string_length _ %+ BORINGSSL_PREFIX %+ _asn1_bit_string_length
%xdefine _asn1_do_adb _ %+ BORINGSSL_PREFIX %+ _asn1_do_adb
%xdefine _asn1_enc_free _ %+ BORINGSSL_PREFIX %+ _asn1_enc_free
%xdefine _asn1_enc_init _ %+ BORINGSSL_PREFIX %+ _asn1_enc_init
%xdefine _asn1_enc_restore _ %+ BORINGSSL_PREFIX %+ _asn1_enc_restore
%xdefine _asn1_enc_save _ %+ BORINGSSL_PREFIX %+ _asn1_enc_save
%xdefine _asn1_encoding_clear _ %+ BORINGSSL_PREFIX %+ _asn1_encoding_clear
%xdefine _asn1_evp_pkey_methods _ %+ BORINGSSL_PREFIX %+ _asn1_evp_pkey_methods
%xdefine _asn1_evp_pkey_methods_size _ %+ BORINGSSL_PREFIX %+ _asn1_evp_pkey_methods_size
%xdefine _asn1_generalizedtime_to_tm _ %+ BORINGSSL_PREFIX %+ _asn1_generalizedtime_to_tm
%xdefine _asn1_get_choice_selector _ %+ BORINGSSL_PREFIX %+ _asn1_get_choice_selector
%xdefine _asn1_get_field_ptr _ %+ BORINGSSL_PREFIX %+ _asn1_get_field_ptr
%xdefine _asn1_get_string_table_for_testing _ %+ BORINGSSL_PREFIX %+ _asn1_get_string_table_for_testing
%xdefine _asn1_is_printable _ %+ BORINGSSL_PREFIX %+ _asn1_is_printable
%xdefine _asn1_item_combine_free _ %+ BORINGSSL_PREFIX %+ _asn1_item_combine_free
%xdefine _asn1_refcount_dec_and_test_zero _ %+ BORINGSSL_PREFIX %+ _asn1_refcount_dec_and_test_zero
%xdefine _asn1_refcount_set_one _ %+ BORINGSSL_PREFIX %+ _asn1_refcount_set_one
%xdefine _asn1_set_choice_selector _ %+ BORINGSSL_PREFIX %+ _asn1_set_choice_selector
%xdefine _asn1_type_cleanup _ %+ BORINGSSL_PREFIX %+ _asn1_type_cleanup
%xdefine _asn1_type_set0_string _ %+ BORINGSSL_PREFIX %+ _asn1_type_set0_string
%xdefine _asn1_type_value_as_pointer _ %+ BORINGSSL_PREFIX %+ _asn1_type_value_as_pointer
%xdefine _asn1_utctime_to_tm _ %+ BORINGSSL_PREFIX %+ _asn1_utctime_to_tm
%xdefine _awslc_api_version_num _ %+ BORINGSSL_PREFIX %+ _awslc_api_version_num
%xdefine _awslc_version_string _ %+ BORINGSSL_PREFIX %+ _awslc_version_string
%xdefine _beeu_mod_inverse_vartime _ %+ BORINGSSL_PREFIX %+ _beeu_mod_inverse_vartime
%xdefine _bignum_add_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_add_p384
%xdefine _bignum_add_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_add_p521
%xdefine _bignum_copy_row_from_table _ %+ BORINGSSL_PREFIX %+ _bignum_copy_row_from_table
%xdefine _bignum_copy_row_from_table_16 _ %+ BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_16
%xdefine _bignum_copy_row_from_table_32 _ %+ BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_32
%xdefine _bignum_copy_row_from_table_8n _ %+ BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_8n
%xdefine _bignum_deamont_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_deamont_p384
%xdefine _bignum_deamont_p384_alt _ %+ BORINGSSL_PREFIX %+ _bignum_deamont_p384_alt
%xdefine _bignum_emontredc_8n _ %+ BORINGSSL_PREFIX %+ _bignum_emontredc_8n
%xdefine _bignum_fromlebytes_6 _ %+ BORINGSSL_PREFIX %+ _bignum_fromlebytes_6
%xdefine _bignum_fromlebytes_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_fromlebytes_p521
%xdefine _bignum_ge _ %+ BORINGSSL_PREFIX %+ _bignum_ge
%xdefine _bignum_inv_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_inv_p521
%xdefine _bignum_kmul_16_32 _ %+ BORINGSSL_PREFIX %+ _bignum_kmul_16_32
%xdefine _bignum_kmul_32_64 _ %+ BORINGSSL_PREFIX %+ _bignum_kmul_32_64
%xdefine _bignum_ksqr_16_32 _ %+ BORINGSSL_PREFIX %+ _bignum_ksqr_16_32
%xdefine _bignum_ksqr_32_64 _ %+ BORINGSSL_PREFIX %+ _bignum_ksqr_32_64
%xdefine _bignum_littleendian_6 _ %+ BORINGSSL_PREFIX %+ _bignum_littleendian_6
%xdefine _bignum_madd_n25519 _ %+ BORINGSSL_PREFIX %+ _bignum_madd_n25519
%xdefine _bignum_madd_n25519_alt _ %+ BORINGSSL_PREFIX %+ _bignum_madd_n25519_alt
%xdefine _bignum_mod_n25519 _ %+ BORINGSSL_PREFIX %+ _bignum_mod_n25519
%xdefine _bignum_montinv_p256 _ %+ BORINGSSL_PREFIX %+ _bignum_montinv_p256
%xdefine _bignum_montinv_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_montinv_p384
%xdefine _bignum_montmul_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_montmul_p384
%xdefine _bignum_montmul_p384_alt _ %+ BORINGSSL_PREFIX %+ _bignum_montmul_p384_alt
%xdefine _bignum_montsqr_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_montsqr_p384
%xdefine _bignum_montsqr_p384_alt _ %+ BORINGSSL_PREFIX %+ _bignum_montsqr_p384_alt
%xdefine _bignum_mul _ %+ BORINGSSL_PREFIX %+ _bignum_mul
%xdefine _bignum_mul_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_mul_p521
%xdefine _bignum_mul_p521_alt _ %+ BORINGSSL_PREFIX %+ _bignum_mul_p521_alt
%xdefine _bignum_neg_p25519 _ %+ BORINGSSL_PREFIX %+ _bignum_neg_p25519
%xdefine _bignum_neg_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_neg_p384
%xdefine _bignum_neg_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_neg_p521
%xdefine _bignum_nonzero_6 _ %+ BORINGSSL_PREFIX %+ _bignum_nonzero_6
%xdefine _bignum_optsub _ %+ BORINGSSL_PREFIX %+ _bignum_optsub
%xdefine _bignum_sqr _ %+ BORINGSSL_PREFIX %+ _bignum_sqr
%xdefine _bignum_sqr_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_sqr_p521
%xdefine _bignum_sqr_p521_alt _ %+ BORINGSSL_PREFIX %+ _bignum_sqr_p521_alt
%xdefine _bignum_sub_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_sub_p384
%xdefine _bignum_sub_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_sub_p521
%xdefine _bignum_tolebytes_6 _ %+ BORINGSSL_PREFIX %+ _bignum_tolebytes_6
%xdefine _bignum_tolebytes_p521 _ %+ BORINGSSL_PREFIX %+ _bignum_tolebytes_p521
%xdefine _bignum_tomont_p384 _ %+ BORINGSSL_PREFIX %+ _bignum_tomont_p384
%xdefine _bignum_tomont_p384_alt _ %+ BORINGSSL_PREFIX %+ _bignum_tomont_p384_alt
%xdefine _bio_clear_socket_error _ %+ BORINGSSL_PREFIX %+ _bio_clear_socket_error
%xdefine _bio_errno_should_retry _ %+ BORINGSSL_PREFIX %+ _bio_errno_should_retry
%xdefine _bio_ip_and_port_to_socket_and_addr _ %+ BORINGSSL_PREFIX %+ _bio_ip_and_port_to_socket_and_addr
%xdefine _bio_sock_error_get_and_clear _ %+ BORINGSSL_PREFIX %+ _bio_sock_error_get_and_clear
%xdefine _bio_socket_nbio _ %+ BORINGSSL_PREFIX %+ _bio_socket_nbio
%xdefine _bio_socket_should_retry _ %+ BORINGSSL_PREFIX %+ _bio_socket_should_retry
%xdefine _bn_abs_sub_consttime _ %+ BORINGSSL_PREFIX %+ _bn_abs_sub_consttime
%xdefine _bn_add_words _ %+ BORINGSSL_PREFIX %+ _bn_add_words
%xdefine _bn_assert_fits_in_bytes _ %+ BORINGSSL_PREFIX %+ _bn_assert_fits_in_bytes
%xdefine _bn_big_endian_to_words _ %+ BORINGSSL_PREFIX %+ _bn_big_endian_to_words
%xdefine _bn_copy_words _ %+ BORINGSSL_PREFIX %+ _bn_copy_words
%xdefine _bn_div_consttime _ %+ BORINGSSL_PREFIX %+ _bn_div_consttime
%xdefine _bn_div_words _ %+ BORINGSSL_PREFIX %+ _bn_div_words
%xdefine _bn_expand _ %+ BORINGSSL_PREFIX %+ _bn_expand
%xdefine _bn_fits_in_words _ %+ BORINGSSL_PREFIX %+ _bn_fits_in_words
%xdefine _bn_from_montgomery_small _ %+ BORINGSSL_PREFIX %+ _bn_from_montgomery_small
%xdefine _bn_gather5 _ %+ BORINGSSL_PREFIX %+ _bn_gather5
%xdefine _bn_in_range_words _ %+ BORINGSSL_PREFIX %+ _bn_in_range_words
%xdefine _bn_is_bit_set_words _ %+ BORINGSSL_PREFIX %+ _bn_is_bit_set_words
%xdefine _bn_is_relatively_prime _ %+ BORINGSSL_PREFIX %+ _bn_is_relatively_prime
%xdefine _bn_jacobi _ %+ BORINGSSL_PREFIX %+ _bn_jacobi
%xdefine _bn_lcm_consttime _ %+ BORINGSSL_PREFIX %+ _bn_lcm_consttime
%xdefine _bn_less_than_montgomery_R _ %+ BORINGSSL_PREFIX %+ _bn_less_than_montgomery_R
%xdefine _bn_less_than_words _ %+ BORINGSSL_PREFIX %+ _bn_less_than_words
%xdefine _bn_little_endian_to_words _ %+ BORINGSSL_PREFIX %+ _bn_little_endian_to_words
%xdefine _bn_miller_rabin_init _ %+ BORINGSSL_PREFIX %+ _bn_miller_rabin_init
%xdefine _bn_miller_rabin_iteration _ %+ BORINGSSL_PREFIX %+ _bn_miller_rabin_iteration
%xdefine _bn_minimal_width _ %+ BORINGSSL_PREFIX %+ _bn_minimal_width
%xdefine _bn_mod_add_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_add_consttime
%xdefine _bn_mod_add_words _ %+ BORINGSSL_PREFIX %+ _bn_mod_add_words
%xdefine _bn_mod_exp_mont_small _ %+ BORINGSSL_PREFIX %+ _bn_mod_exp_mont_small
%xdefine _bn_mod_inverse0_prime_mont_small _ %+ BORINGSSL_PREFIX %+ _bn_mod_inverse0_prime_mont_small
%xdefine _bn_mod_inverse_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_inverse_consttime
%xdefine _bn_mod_inverse_prime _ %+ BORINGSSL_PREFIX %+ _bn_mod_inverse_prime
%xdefine _bn_mod_inverse_secret_prime _ %+ BORINGSSL_PREFIX %+ _bn_mod_inverse_secret_prime
%xdefine _bn_mod_lshift1_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_lshift1_consttime
%xdefine _bn_mod_lshift_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_lshift_consttime
%xdefine _bn_mod_mul_montgomery_small _ %+ BORINGSSL_PREFIX %+ _bn_mod_mul_montgomery_small
%xdefine _bn_mod_sub_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_sub_consttime
%xdefine _bn_mod_sub_words _ %+ BORINGSSL_PREFIX %+ _bn_mod_sub_words
%xdefine _bn_mod_u16_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mod_u16_consttime
%xdefine _bn_mont_ctx_cleanup _ %+ BORINGSSL_PREFIX %+ _bn_mont_ctx_cleanup
%xdefine _bn_mont_ctx_init _ %+ BORINGSSL_PREFIX %+ _bn_mont_ctx_init
%xdefine _bn_mont_ctx_set_RR_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mont_ctx_set_RR_consttime
%xdefine _bn_mont_n0 _ %+ BORINGSSL_PREFIX %+ _bn_mont_n0
%xdefine _bn_mul4x_mont _ %+ BORINGSSL_PREFIX %+ _bn_mul4x_mont
%xdefine _bn_mul8x_mont_neon _ %+ BORINGSSL_PREFIX %+ _bn_mul8x_mont_neon
%xdefine _bn_mul_add_words _ %+ BORINGSSL_PREFIX %+ _bn_mul_add_words
%xdefine _bn_mul_comba4 _ %+ BORINGSSL_PREFIX %+ _bn_mul_comba4
%xdefine _bn_mul_comba8 _ %+ BORINGSSL_PREFIX %+ _bn_mul_comba8
%xdefine _bn_mul_consttime _ %+ BORINGSSL_PREFIX %+ _bn_mul_consttime
%xdefine _bn_mul_mont _ %+ BORINGSSL_PREFIX %+ _bn_mul_mont
%xdefine _bn_mul_mont_gather5 _ %+ BORINGSSL_PREFIX %+ _bn_mul_mont_gather5
%xdefine _bn_mul_mont_nohw _ %+ BORINGSSL_PREFIX %+ _bn_mul_mont_nohw
%xdefine _bn_mul_small _ %+ BORINGSSL_PREFIX %+ _bn_mul_small
%xdefine _bn_mul_words _ %+ BORINGSSL_PREFIX %+ _bn_mul_words
%xdefine _bn_mulx4x_mont _ %+ BORINGSSL_PREFIX %+ _bn_mulx4x_mont
%xdefine _bn_odd_number_is_obviously_composite _ %+ BORINGSSL_PREFIX %+ _bn_odd_number_is_obviously_composite
%xdefine _bn_one_to_montgomery _ %+ BORINGSSL_PREFIX %+ _bn_one_to_montgomery
%xdefine _bn_power5 _ %+ BORINGSSL_PREFIX %+ _bn_power5
%xdefine _bn_rand_range_words _ %+ BORINGSSL_PREFIX %+ _bn_rand_range_words
%xdefine _bn_rand_secret_range _ %+ BORINGSSL_PREFIX %+ _bn_rand_secret_range
%xdefine _bn_reduce_once _ %+ BORINGSSL_PREFIX %+ _bn_reduce_once
%xdefine _bn_reduce_once_in_place _ %+ BORINGSSL_PREFIX %+ _bn_reduce_once_in_place
%xdefine _bn_resize_words _ %+ BORINGSSL_PREFIX %+ _bn_resize_words
%xdefine _bn_rshift1_words _ %+ BORINGSSL_PREFIX %+ _bn_rshift1_words
%xdefine _bn_rshift_secret_shift _ %+ BORINGSSL_PREFIX %+ _bn_rshift_secret_shift
%xdefine _bn_rshift_words _ %+ BORINGSSL_PREFIX %+ _bn_rshift_words
%xdefine _bn_scatter5 _ %+ BORINGSSL_PREFIX %+ _bn_scatter5
%xdefine _bn_select_words _ %+ BORINGSSL_PREFIX %+ _bn_select_words
%xdefine _bn_set_minimal_width _ %+ BORINGSSL_PREFIX %+ _bn_set_minimal_width
%xdefine _bn_set_static_words _ %+ BORINGSSL_PREFIX %+ _bn_set_static_words
%xdefine _bn_set_words _ %+ BORINGSSL_PREFIX %+ _bn_set_words
%xdefine _bn_sqr8x_internal _ %+ BORINGSSL_PREFIX %+ _bn_sqr8x_internal
%xdefine _bn_sqr8x_mont _ %+ BORINGSSL_PREFIX %+ _bn_sqr8x_mont
%xdefine _bn_sqr_comba4 _ %+ BORINGSSL_PREFIX %+ _bn_sqr_comba4
%xdefine _bn_sqr_comba8 _ %+ BORINGSSL_PREFIX %+ _bn_sqr_comba8
%xdefine _bn_sqr_consttime _ %+ BORINGSSL_PREFIX %+ _bn_sqr_consttime
%xdefine _bn_sqr_small _ %+ BORINGSSL_PREFIX %+ _bn_sqr_small
%xdefine _bn_sqr_words _ %+ BORINGSSL_PREFIX %+ _bn_sqr_words
%xdefine _bn_sqrx8x_internal _ %+ BORINGSSL_PREFIX %+ _bn_sqrx8x_internal
%xdefine _bn_sub_words _ %+ BORINGSSL_PREFIX %+ _bn_sub_words
%xdefine _bn_to_montgomery_small _ %+ BORINGSSL_PREFIX %+ _bn_to_montgomery_small
%xdefine _bn_uadd_consttime _ %+ BORINGSSL_PREFIX %+ _bn_uadd_consttime
%xdefine _bn_usub_consttime _ %+ BORINGSSL_PREFIX %+ _bn_usub_consttime
%xdefine _bn_wexpand _ %+ BORINGSSL_PREFIX %+ _bn_wexpand
%xdefine _bn_words_to_big_endian _ %+ BORINGSSL_PREFIX %+ _bn_words_to_big_endian
%xdefine _bn_words_to_little_endian _ %+ BORINGSSL_PREFIX %+ _bn_words_to_little_endian
%xdefine _boringssl_self_test_hmac_sha256 _ %+ BORINGSSL_PREFIX %+ _boringssl_self_test_hmac_sha256
%xdefine _boringssl_self_test_sha256 _ %+ BORINGSSL_PREFIX %+ _boringssl_self_test_sha256
%xdefine _bsaes_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _bsaes_cbc_encrypt
%xdefine _bsaes_ctr32_encrypt_blocks _ %+ BORINGSSL_PREFIX %+ _bsaes_ctr32_encrypt_blocks
%xdefine _c2i_ASN1_BIT_STRING _ %+ BORINGSSL_PREFIX %+ _c2i_ASN1_BIT_STRING
%xdefine _c2i_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _c2i_ASN1_INTEGER
%xdefine _c2i_ASN1_OBJECT _ %+ BORINGSSL_PREFIX %+ _c2i_ASN1_OBJECT
%xdefine _cbb_add_latin1 _ %+ BORINGSSL_PREFIX %+ _cbb_add_latin1
%xdefine _cbb_add_ucs2_be _ %+ BORINGSSL_PREFIX %+ _cbb_add_ucs2_be
%xdefine _cbb_add_utf32_be _ %+ BORINGSSL_PREFIX %+ _cbb_add_utf32_be
%xdefine _cbb_add_utf8 _ %+ BORINGSSL_PREFIX %+ _cbb_add_utf8
%xdefine _cbb_get_utf8_len _ %+ BORINGSSL_PREFIX %+ _cbb_get_utf8_len
%xdefine _cbs_get_any_asn1_element _ %+ BORINGSSL_PREFIX %+ _cbs_get_any_asn1_element
%xdefine _cbs_get_latin1 _ %+ BORINGSSL_PREFIX %+ _cbs_get_latin1
%xdefine _cbs_get_ucs2_be _ %+ BORINGSSL_PREFIX %+ _cbs_get_ucs2_be
%xdefine _cbs_get_utf32_be _ %+ BORINGSSL_PREFIX %+ _cbs_get_utf32_be
%xdefine _cbs_get_utf8 _ %+ BORINGSSL_PREFIX %+ _cbs_get_utf8
%xdefine _chacha20_poly1305_open _ %+ BORINGSSL_PREFIX %+ _chacha20_poly1305_open
%xdefine _chacha20_poly1305_seal _ %+ BORINGSSL_PREFIX %+ _chacha20_poly1305_seal
%xdefine _cn2dnsid _ %+ BORINGSSL_PREFIX %+ _cn2dnsid
%xdefine _crypto_gcm_avx512_enabled _ %+ BORINGSSL_PREFIX %+ _crypto_gcm_avx512_enabled
%xdefine _crypto_gcm_clmul_enabled _ %+ BORINGSSL_PREFIX %+ _crypto_gcm_clmul_enabled
%xdefine _curve25519_x25519 _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519
%xdefine _curve25519_x25519_alt _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519_alt
%xdefine _curve25519_x25519_byte _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519_byte
%xdefine _curve25519_x25519_byte_alt _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519_byte_alt
%xdefine _curve25519_x25519base _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519base
%xdefine _curve25519_x25519base_alt _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519base_alt
%xdefine _curve25519_x25519base_byte _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519base_byte
%xdefine _curve25519_x25519base_byte_alt _ %+ BORINGSSL_PREFIX %+ _curve25519_x25519base_byte_alt
%xdefine _d2i_ASN1_BIT_STRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_BIT_STRING
%xdefine _d2i_ASN1_BMPSTRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_BMPSTRING
%xdefine _d2i_ASN1_BOOLEAN _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_BOOLEAN
%xdefine _d2i_ASN1_ENUMERATED _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_ENUMERATED
%xdefine _d2i_ASN1_GENERALIZEDTIME _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_GENERALIZEDTIME
%xdefine _d2i_ASN1_GENERALSTRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_GENERALSTRING
%xdefine _d2i_ASN1_IA5STRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_IA5STRING
%xdefine _d2i_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_INTEGER
%xdefine _d2i_ASN1_NULL _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_NULL
%xdefine _d2i_ASN1_OBJECT _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_OBJECT
%xdefine _d2i_ASN1_OCTET_STRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_OCTET_STRING
%xdefine _d2i_ASN1_PRINTABLE _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_PRINTABLE
%xdefine _d2i_ASN1_PRINTABLESTRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_PRINTABLESTRING
%xdefine _d2i_ASN1_SEQUENCE_ANY _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_SEQUENCE_ANY
%xdefine _d2i_ASN1_SET_ANY _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_SET_ANY
%xdefine _d2i_ASN1_T61STRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_T61STRING
%xdefine _d2i_ASN1_TIME _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_TIME
%xdefine _d2i_ASN1_TYPE _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_TYPE
%xdefine _d2i_ASN1_UNIVERSALSTRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_UNIVERSALSTRING
%xdefine _d2i_ASN1_UTCTIME _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_UTCTIME
%xdefine _d2i_ASN1_UTF8STRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_UTF8STRING
%xdefine _d2i_ASN1_VISIBLESTRING _ %+ BORINGSSL_PREFIX %+ _d2i_ASN1_VISIBLESTRING
%xdefine _d2i_AUTHORITY_INFO_ACCESS _ %+ BORINGSSL_PREFIX %+ _d2i_AUTHORITY_INFO_ACCESS
%xdefine _d2i_AUTHORITY_KEYID _ %+ BORINGSSL_PREFIX %+ _d2i_AUTHORITY_KEYID
%xdefine _d2i_AutoPrivateKey _ %+ BORINGSSL_PREFIX %+ _d2i_AutoPrivateKey
%xdefine _d2i_BASIC_CONSTRAINTS _ %+ BORINGSSL_PREFIX %+ _d2i_BASIC_CONSTRAINTS
%xdefine _d2i_CERTIFICATEPOLICIES _ %+ BORINGSSL_PREFIX %+ _d2i_CERTIFICATEPOLICIES
%xdefine _d2i_CRL_DIST_POINTS _ %+ BORINGSSL_PREFIX %+ _d2i_CRL_DIST_POINTS
%xdefine _d2i_DHparams _ %+ BORINGSSL_PREFIX %+ _d2i_DHparams
%xdefine _d2i_DHparams_bio _ %+ BORINGSSL_PREFIX %+ _d2i_DHparams_bio
%xdefine _d2i_DIRECTORYSTRING _ %+ BORINGSSL_PREFIX %+ _d2i_DIRECTORYSTRING
%xdefine _d2i_DISPLAYTEXT _ %+ BORINGSSL_PREFIX %+ _d2i_DISPLAYTEXT
%xdefine _d2i_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey
%xdefine _d2i_DSAPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey_bio
%xdefine _d2i_DSAPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey_fp
%xdefine _d2i_DSAPublicKey _ %+ BORINGSSL_PREFIX %+ _d2i_DSAPublicKey
%xdefine _d2i_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY
%xdefine _d2i_DSA_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY_bio
%xdefine _d2i_DSA_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY_fp
%xdefine _d2i_DSA_SIG _ %+ BORINGSSL_PREFIX %+ _d2i_DSA_SIG
%xdefine _d2i_DSAparams _ %+ BORINGSSL_PREFIX %+ _d2i_DSAparams
%xdefine _d2i_ECDSA_SIG _ %+ BORINGSSL_PREFIX %+ _d2i_ECDSA_SIG
%xdefine _d2i_ECPKParameters _ %+ BORINGSSL_PREFIX %+ _d2i_ECPKParameters
%xdefine _d2i_ECPKParameters_bio _ %+ BORINGSSL_PREFIX %+ _d2i_ECPKParameters_bio
%xdefine _d2i_ECParameters _ %+ BORINGSSL_PREFIX %+ _d2i_ECParameters
%xdefine _d2i_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _d2i_ECPrivateKey
%xdefine _d2i_ECPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_ECPrivateKey_bio
%xdefine _d2i_ECPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_ECPrivateKey_fp
%xdefine _d2i_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY
%xdefine _d2i_EC_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY_bio
%xdefine _d2i_EC_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY_fp
%xdefine _d2i_EXTENDED_KEY_USAGE _ %+ BORINGSSL_PREFIX %+ _d2i_EXTENDED_KEY_USAGE
%xdefine _d2i_GENERAL_NAME _ %+ BORINGSSL_PREFIX %+ _d2i_GENERAL_NAME
%xdefine _d2i_GENERAL_NAMES _ %+ BORINGSSL_PREFIX %+ _d2i_GENERAL_NAMES
%xdefine _d2i_ISSUING_DIST_POINT _ %+ BORINGSSL_PREFIX %+ _d2i_ISSUING_DIST_POINT
%xdefine _d2i_NETSCAPE_SPKAC _ %+ BORINGSSL_PREFIX %+ _d2i_NETSCAPE_SPKAC
%xdefine _d2i_NETSCAPE_SPKI _ %+ BORINGSSL_PREFIX %+ _d2i_NETSCAPE_SPKI
%xdefine _d2i_OCSP_BASICRESP _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_BASICRESP
%xdefine _d2i_OCSP_CERTID _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_CERTID
%xdefine _d2i_OCSP_ONEREQ _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_ONEREQ
%xdefine _d2i_OCSP_REQINFO _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_REQINFO
%xdefine _d2i_OCSP_REQUEST _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_REQUEST
%xdefine _d2i_OCSP_REQUEST_bio _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_REQUEST_bio
%xdefine _d2i_OCSP_RESPBYTES _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_RESPBYTES
%xdefine _d2i_OCSP_RESPDATA _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_RESPDATA
%xdefine _d2i_OCSP_RESPONSE _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_RESPONSE
%xdefine _d2i_OCSP_RESPONSE_bio _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_RESPONSE_bio
%xdefine _d2i_OCSP_REVOKEDINFO _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_REVOKEDINFO
%xdefine _d2i_OCSP_SIGNATURE _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_SIGNATURE
%xdefine _d2i_OCSP_SINGLERESP _ %+ BORINGSSL_PREFIX %+ _d2i_OCSP_SINGLERESP
%xdefine _d2i_PKCS12 _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS12
%xdefine _d2i_PKCS12_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS12_bio
%xdefine _d2i_PKCS12_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS12_fp
%xdefine _d2i_PKCS7 _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7
%xdefine _d2i_PKCS7_DIGEST _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_DIGEST
%xdefine _d2i_PKCS7_ENCRYPT _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_ENCRYPT
%xdefine _d2i_PKCS7_ENC_CONTENT _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_ENC_CONTENT
%xdefine _d2i_PKCS7_ENVELOPE _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_ENVELOPE
%xdefine _d2i_PKCS7_ISSUER_AND_SERIAL _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_ISSUER_AND_SERIAL
%xdefine _d2i_PKCS7_RECIP_INFO _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_RECIP_INFO
%xdefine _d2i_PKCS7_SIGNED _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGNED
%xdefine _d2i_PKCS7_SIGNER_INFO _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGNER_INFO
%xdefine _d2i_PKCS7_SIGN_ENVELOPE _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGN_ENVELOPE
%xdefine _d2i_PKCS7_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS7_bio
%xdefine _d2i_PKCS8PrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8PrivateKey_bio
%xdefine _d2i_PKCS8PrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8PrivateKey_fp
%xdefine _d2i_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO
%xdefine _d2i_PKCS8_PRIV_KEY_INFO_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO_bio
%xdefine _d2i_PKCS8_PRIV_KEY_INFO_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO_fp
%xdefine _d2i_PKCS8_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8_bio
%xdefine _d2i_PKCS8_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PKCS8_fp
%xdefine _d2i_PUBKEY _ %+ BORINGSSL_PREFIX %+ _d2i_PUBKEY
%xdefine _d2i_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PUBKEY_bio
%xdefine _d2i_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PUBKEY_fp
%xdefine _d2i_PrivateKey _ %+ BORINGSSL_PREFIX %+ _d2i_PrivateKey
%xdefine _d2i_PrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_PrivateKey_bio
%xdefine _d2i_PrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_PrivateKey_fp
%xdefine _d2i_PublicKey _ %+ BORINGSSL_PREFIX %+ _d2i_PublicKey
%xdefine _d2i_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey
%xdefine _d2i_RSAPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey_bio
%xdefine _d2i_RSAPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey_fp
%xdefine _d2i_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPublicKey
%xdefine _d2i_RSAPublicKey_bio _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPublicKey_bio
%xdefine _d2i_RSAPublicKey_fp _ %+ BORINGSSL_PREFIX %+ _d2i_RSAPublicKey_fp
%xdefine _d2i_RSA_PSS_PARAMS _ %+ BORINGSSL_PREFIX %+ _d2i_RSA_PSS_PARAMS
%xdefine _d2i_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY
%xdefine _d2i_RSA_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY_bio
%xdefine _d2i_RSA_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY_fp
%xdefine _d2i_X509 _ %+ BORINGSSL_PREFIX %+ _d2i_X509
%xdefine _d2i_X509_ALGOR _ %+ BORINGSSL_PREFIX %+ _d2i_X509_ALGOR
%xdefine _d2i_X509_ATTRIBUTE _ %+ BORINGSSL_PREFIX %+ _d2i_X509_ATTRIBUTE
%xdefine _d2i_X509_AUX _ %+ BORINGSSL_PREFIX %+ _d2i_X509_AUX
%xdefine _d2i_X509_CERT_AUX _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CERT_AUX
%xdefine _d2i_X509_CINF _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CINF
%xdefine _d2i_X509_CRL _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CRL
%xdefine _d2i_X509_CRL_INFO _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CRL_INFO
%xdefine _d2i_X509_CRL_bio _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CRL_bio
%xdefine _d2i_X509_CRL_fp _ %+ BORINGSSL_PREFIX %+ _d2i_X509_CRL_fp
%xdefine _d2i_X509_EXTENSION _ %+ BORINGSSL_PREFIX %+ _d2i_X509_EXTENSION
%xdefine _d2i_X509_EXTENSIONS _ %+ BORINGSSL_PREFIX %+ _d2i_X509_EXTENSIONS
%xdefine _d2i_X509_NAME _ %+ BORINGSSL_PREFIX %+ _d2i_X509_NAME
%xdefine _d2i_X509_NAME_ENTRY _ %+ BORINGSSL_PREFIX %+ _d2i_X509_NAME_ENTRY
%xdefine _d2i_X509_PUBKEY _ %+ BORINGSSL_PREFIX %+ _d2i_X509_PUBKEY
%xdefine _d2i_X509_REQ _ %+ BORINGSSL_PREFIX %+ _d2i_X509_REQ
%xdefine _d2i_X509_REQ_INFO _ %+ BORINGSSL_PREFIX %+ _d2i_X509_REQ_INFO
%xdefine _d2i_X509_REQ_bio _ %+ BORINGSSL_PREFIX %+ _d2i_X509_REQ_bio
%xdefine _d2i_X509_REQ_fp _ %+ BORINGSSL_PREFIX %+ _d2i_X509_REQ_fp
%xdefine _d2i_X509_REVOKED _ %+ BORINGSSL_PREFIX %+ _d2i_X509_REVOKED
%xdefine _d2i_X509_SIG _ %+ BORINGSSL_PREFIX %+ _d2i_X509_SIG
%xdefine _d2i_X509_VAL _ %+ BORINGSSL_PREFIX %+ _d2i_X509_VAL
%xdefine _d2i_X509_bio _ %+ BORINGSSL_PREFIX %+ _d2i_X509_bio
%xdefine _d2i_X509_fp _ %+ BORINGSSL_PREFIX %+ _d2i_X509_fp
%xdefine _dh_asn1_meth _ %+ BORINGSSL_PREFIX %+ _dh_asn1_meth
%xdefine _dh_check_params_fast _ %+ BORINGSSL_PREFIX %+ _dh_check_params_fast
%xdefine _dh_compute_key_padded_no_self_test _ %+ BORINGSSL_PREFIX %+ _dh_compute_key_padded_no_self_test
%xdefine _dh_pkey_meth _ %+ BORINGSSL_PREFIX %+ _dh_pkey_meth
%xdefine _disable_mocked_ube_detection_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _disable_mocked_ube_detection_FOR_TESTING
%xdefine _dsa_asn1_meth _ %+ BORINGSSL_PREFIX %+ _dsa_asn1_meth
%xdefine _dsa_check_key _ %+ BORINGSSL_PREFIX %+ _dsa_check_key
%xdefine _dsa_internal_paramgen _ %+ BORINGSSL_PREFIX %+ _dsa_internal_paramgen
%xdefine _dsa_pkey_meth _ %+ BORINGSSL_PREFIX %+ _dsa_pkey_meth
%xdefine _dummy_func_for_constructor _ %+ BORINGSSL_PREFIX %+ _dummy_func_for_constructor
%xdefine _ec_GFp_mont_add _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_add
%xdefine _ec_GFp_mont_dbl _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_dbl
%xdefine _ec_GFp_mont_felem_exp _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_exp
%xdefine _ec_GFp_mont_felem_from_bytes _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_from_bytes
%xdefine _ec_GFp_mont_felem_mul _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_mul
%xdefine _ec_GFp_mont_felem_reduce _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_reduce
%xdefine _ec_GFp_mont_felem_sqr _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_sqr
%xdefine _ec_GFp_mont_felem_to_bytes _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_to_bytes
%xdefine _ec_GFp_mont_init_precomp _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_init_precomp
%xdefine _ec_GFp_mont_mul _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_mul
%xdefine _ec_GFp_mont_mul_base _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_base
%xdefine _ec_GFp_mont_mul_batch _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_batch
%xdefine _ec_GFp_mont_mul_precomp _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_precomp
%xdefine _ec_GFp_mont_mul_public_batch _ %+ BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_public_batch
%xdefine _ec_GFp_nistp_recode_scalar_bits _ %+ BORINGSSL_PREFIX %+ _ec_GFp_nistp_recode_scalar_bits
%xdefine _ec_GFp_simple_cmp_x_coordinate _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_cmp_x_coordinate
%xdefine _ec_GFp_simple_felem_from_bytes _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_felem_from_bytes
%xdefine _ec_GFp_simple_felem_to_bytes _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_felem_to_bytes
%xdefine _ec_GFp_simple_group_get_curve _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_group_get_curve
%xdefine _ec_GFp_simple_group_set_curve _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_group_set_curve
%xdefine _ec_GFp_simple_invert _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_invert
%xdefine _ec_GFp_simple_is_at_infinity _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_is_at_infinity
%xdefine _ec_GFp_simple_is_on_curve _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_is_on_curve
%xdefine _ec_GFp_simple_point_copy _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_point_copy
%xdefine _ec_GFp_simple_point_init _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_point_init
%xdefine _ec_GFp_simple_point_set_to_infinity _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_point_set_to_infinity
%xdefine _ec_GFp_simple_points_equal _ %+ BORINGSSL_PREFIX %+ _ec_GFp_simple_points_equal
%xdefine _ec_affine_jacobian_equal _ %+ BORINGSSL_PREFIX %+ _ec_affine_jacobian_equal
%xdefine _ec_affine_select _ %+ BORINGSSL_PREFIX %+ _ec_affine_select
%xdefine _ec_affine_to_jacobian _ %+ BORINGSSL_PREFIX %+ _ec_affine_to_jacobian
%xdefine _ec_asn1_meth _ %+ BORINGSSL_PREFIX %+ _ec_asn1_meth
%xdefine _ec_bignum_to_felem _ %+ BORINGSSL_PREFIX %+ _ec_bignum_to_felem
%xdefine _ec_bignum_to_scalar _ %+ BORINGSSL_PREFIX %+ _ec_bignum_to_scalar
%xdefine _ec_cmp_x_coordinate _ %+ BORINGSSL_PREFIX %+ _ec_cmp_x_coordinate
%xdefine _ec_compute_wNAF _ %+ BORINGSSL_PREFIX %+ _ec_compute_wNAF
%xdefine _ec_felem_add _ %+ BORINGSSL_PREFIX %+ _ec_felem_add
%xdefine _ec_felem_equal _ %+ BORINGSSL_PREFIX %+ _ec_felem_equal
%xdefine _ec_felem_from_bytes _ %+ BORINGSSL_PREFIX %+ _ec_felem_from_bytes
%xdefine _ec_felem_neg _ %+ BORINGSSL_PREFIX %+ _ec_felem_neg
%xdefine _ec_felem_non_zero_mask _ %+ BORINGSSL_PREFIX %+ _ec_felem_non_zero_mask
%xdefine _ec_felem_one _ %+ BORINGSSL_PREFIX %+ _ec_felem_one
%xdefine _ec_felem_select _ %+ BORINGSSL_PREFIX %+ _ec_felem_select
%xdefine _ec_felem_sub _ %+ BORINGSSL_PREFIX %+ _ec_felem_sub
%xdefine _ec_felem_to_bignum _ %+ BORINGSSL_PREFIX %+ _ec_felem_to_bignum
%xdefine _ec_felem_to_bytes _ %+ BORINGSSL_PREFIX %+ _ec_felem_to_bytes
%xdefine _ec_get_x_coordinate_as_bytes _ %+ BORINGSSL_PREFIX %+ _ec_get_x_coordinate_as_bytes
%xdefine _ec_get_x_coordinate_as_scalar _ %+ BORINGSSL_PREFIX %+ _ec_get_x_coordinate_as_scalar
%xdefine _ec_hash_to_curve_p256_xmd_sha256_sswu _ %+ BORINGSSL_PREFIX %+ _ec_hash_to_curve_p256_xmd_sha256_sswu
%xdefine _ec_hash_to_curve_p384_xmd_sha384_sswu _ %+ BORINGSSL_PREFIX %+ _ec_hash_to_curve_p384_xmd_sha384_sswu
%xdefine _ec_hash_to_curve_p384_xmd_sha512_sswu_draft07 _ %+ BORINGSSL_PREFIX %+ _ec_hash_to_curve_p384_xmd_sha512_sswu_draft07
%xdefine _ec_hash_to_scalar_p384_xmd_sha384 _ %+ BORINGSSL_PREFIX %+ _ec_hash_to_scalar_p384_xmd_sha384
%xdefine _ec_hash_to_scalar_p384_xmd_sha512_draft07 _ %+ BORINGSSL_PREFIX %+ _ec_hash_to_scalar_p384_xmd_sha512_draft07
%xdefine _ec_init_precomp _ %+ BORINGSSL_PREFIX %+ _ec_init_precomp
%xdefine _ec_jacobian_to_affine _ %+ BORINGSSL_PREFIX %+ _ec_jacobian_to_affine
%xdefine _ec_jacobian_to_affine_batch _ %+ BORINGSSL_PREFIX %+ _ec_jacobian_to_affine_batch
%xdefine _ec_nistp_coordinates_to_point _ %+ BORINGSSL_PREFIX %+ _ec_nistp_coordinates_to_point
%xdefine _ec_nistp_point_add _ %+ BORINGSSL_PREFIX %+ _ec_nistp_point_add
%xdefine _ec_nistp_point_double _ %+ BORINGSSL_PREFIX %+ _ec_nistp_point_double
%xdefine _ec_nistp_point_to_coordinates _ %+ BORINGSSL_PREFIX %+ _ec_nistp_point_to_coordinates
%xdefine _ec_nistp_scalar_mul _ %+ BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul
%xdefine _ec_nistp_scalar_mul_base _ %+ BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul_base
%xdefine _ec_nistp_scalar_mul_public _ %+ BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul_public
%xdefine _ec_point_byte_len _ %+ BORINGSSL_PREFIX %+ _ec_point_byte_len
%xdefine _ec_point_from_uncompressed _ %+ BORINGSSL_PREFIX %+ _ec_point_from_uncompressed
%xdefine _ec_point_mul_no_self_test _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_no_self_test
%xdefine _ec_point_mul_scalar _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar
%xdefine _ec_point_mul_scalar_base _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar_base
%xdefine _ec_point_mul_scalar_batch _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar_batch
%xdefine _ec_point_mul_scalar_precomp _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar_precomp
%xdefine _ec_point_mul_scalar_public _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar_public
%xdefine _ec_point_mul_scalar_public_batch _ %+ BORINGSSL_PREFIX %+ _ec_point_mul_scalar_public_batch
%xdefine _ec_point_select _ %+ BORINGSSL_PREFIX %+ _ec_point_select
%xdefine _ec_point_set_affine_coordinates _ %+ BORINGSSL_PREFIX %+ _ec_point_set_affine_coordinates
%xdefine _ec_point_to_bytes _ %+ BORINGSSL_PREFIX %+ _ec_point_to_bytes
%xdefine _ec_precomp_select _ %+ BORINGSSL_PREFIX %+ _ec_precomp_select
%xdefine _ec_random_nonzero_scalar _ %+ BORINGSSL_PREFIX %+ _ec_random_nonzero_scalar
%xdefine _ec_scalar_add _ %+ BORINGSSL_PREFIX %+ _ec_scalar_add
%xdefine _ec_scalar_equal_vartime _ %+ BORINGSSL_PREFIX %+ _ec_scalar_equal_vartime
%xdefine _ec_scalar_from_bytes _ %+ BORINGSSL_PREFIX %+ _ec_scalar_from_bytes
%xdefine _ec_scalar_from_montgomery _ %+ BORINGSSL_PREFIX %+ _ec_scalar_from_montgomery
%xdefine _ec_scalar_inv0_montgomery _ %+ BORINGSSL_PREFIX %+ _ec_scalar_inv0_montgomery
%xdefine _ec_scalar_is_zero _ %+ BORINGSSL_PREFIX %+ _ec_scalar_is_zero
%xdefine _ec_scalar_mul_montgomery _ %+ BORINGSSL_PREFIX %+ _ec_scalar_mul_montgomery
%xdefine _ec_scalar_neg _ %+ BORINGSSL_PREFIX %+ _ec_scalar_neg
%xdefine _ec_scalar_reduce _ %+ BORINGSSL_PREFIX %+ _ec_scalar_reduce
%xdefine _ec_scalar_select _ %+ BORINGSSL_PREFIX %+ _ec_scalar_select
%xdefine _ec_scalar_sub _ %+ BORINGSSL_PREFIX %+ _ec_scalar_sub
%xdefine _ec_scalar_to_bytes _ %+ BORINGSSL_PREFIX %+ _ec_scalar_to_bytes
%xdefine _ec_scalar_to_montgomery _ %+ BORINGSSL_PREFIX %+ _ec_scalar_to_montgomery
%xdefine _ec_scalar_to_montgomery_inv_vartime _ %+ BORINGSSL_PREFIX %+ _ec_scalar_to_montgomery_inv_vartime
%xdefine _ec_set_to_safe_point _ %+ BORINGSSL_PREFIX %+ _ec_set_to_safe_point
%xdefine _ec_simple_scalar_inv0_montgomery _ %+ BORINGSSL_PREFIX %+ _ec_simple_scalar_inv0_montgomery
%xdefine _ec_simple_scalar_to_montgomery_inv_vartime _ %+ BORINGSSL_PREFIX %+ _ec_simple_scalar_to_montgomery_inv_vartime
%xdefine _ecdsa_digestsign_no_self_test _ %+ BORINGSSL_PREFIX %+ _ecdsa_digestsign_no_self_test
%xdefine _ecdsa_digestverify_no_self_test _ %+ BORINGSSL_PREFIX %+ _ecdsa_digestverify_no_self_test
%xdefine _ecdsa_do_verify_no_self_test _ %+ BORINGSSL_PREFIX %+ _ecdsa_do_verify_no_self_test
%xdefine _ecdsa_sign_with_nonce_for_known_answer_test _ %+ BORINGSSL_PREFIX %+ _ecdsa_sign_with_nonce_for_known_answer_test
%xdefine _ecp_nistz256_avx2_select_w7 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_avx2_select_w7
%xdefine _ecp_nistz256_div_by_2 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_div_by_2
%xdefine _ecp_nistz256_mul_by_2 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_mul_by_2
%xdefine _ecp_nistz256_mul_by_3 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_mul_by_3
%xdefine _ecp_nistz256_mul_mont _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_mul_mont
%xdefine _ecp_nistz256_neg _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_neg
%xdefine _ecp_nistz256_ord_mul_mont _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_ord_mul_mont
%xdefine _ecp_nistz256_ord_sqr_mont _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_ord_sqr_mont
%xdefine _ecp_nistz256_point_add _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_point_add
%xdefine _ecp_nistz256_point_add_affine _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_point_add_affine
%xdefine _ecp_nistz256_point_double _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_point_double
%xdefine _ecp_nistz256_select_w5 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_select_w5
%xdefine _ecp_nistz256_select_w7 _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_select_w7
%xdefine _ecp_nistz256_sqr_mont _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_sqr_mont
%xdefine _ecp_nistz256_sub _ %+ BORINGSSL_PREFIX %+ _ecp_nistz256_sub
%xdefine _ed25519_asn1_meth _ %+ BORINGSSL_PREFIX %+ _ed25519_asn1_meth
%xdefine _ed25519_check_public_key_nohw _ %+ BORINGSSL_PREFIX %+ _ed25519_check_public_key_nohw
%xdefine _ed25519_check_public_key_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _ed25519_check_public_key_s2n_bignum
%xdefine _ed25519_public_key_from_hashed_seed_nohw _ %+ BORINGSSL_PREFIX %+ _ed25519_public_key_from_hashed_seed_nohw
%xdefine _ed25519_public_key_from_hashed_seed_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _ed25519_public_key_from_hashed_seed_s2n_bignum
%xdefine _ed25519_sha512 _ %+ BORINGSSL_PREFIX %+ _ed25519_sha512
%xdefine _ed25519_sign_internal _ %+ BORINGSSL_PREFIX %+ _ed25519_sign_internal
%xdefine _ed25519_sign_nohw _ %+ BORINGSSL_PREFIX %+ _ed25519_sign_nohw
%xdefine _ed25519_sign_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _ed25519_sign_s2n_bignum
%xdefine _ed25519_verify_internal _ %+ BORINGSSL_PREFIX %+ _ed25519_verify_internal
%xdefine _ed25519_verify_nohw _ %+ BORINGSSL_PREFIX %+ _ed25519_verify_nohw
%xdefine _ed25519_verify_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _ed25519_verify_s2n_bignum
%xdefine _ed25519ph_asn1_meth _ %+ BORINGSSL_PREFIX %+ _ed25519ph_asn1_meth
%xdefine _edwards25519_decode _ %+ BORINGSSL_PREFIX %+ _edwards25519_decode
%xdefine _edwards25519_decode_alt _ %+ BORINGSSL_PREFIX %+ _edwards25519_decode_alt
%xdefine _edwards25519_encode _ %+ BORINGSSL_PREFIX %+ _edwards25519_encode
%xdefine _edwards25519_scalarmulbase _ %+ BORINGSSL_PREFIX %+ _edwards25519_scalarmulbase
%xdefine _edwards25519_scalarmulbase_alt _ %+ BORINGSSL_PREFIX %+ _edwards25519_scalarmulbase_alt
%xdefine _edwards25519_scalarmuldouble _ %+ BORINGSSL_PREFIX %+ _edwards25519_scalarmuldouble
%xdefine _edwards25519_scalarmuldouble_alt _ %+ BORINGSSL_PREFIX %+ _edwards25519_scalarmuldouble_alt
%xdefine _evp_pkey_set_cb_translate _ %+ BORINGSSL_PREFIX %+ _evp_pkey_set_cb_translate
%xdefine _evp_pkey_set_method _ %+ BORINGSSL_PREFIX %+ _evp_pkey_set_method
%xdefine _extract_multiplier_2x20_win5 _ %+ BORINGSSL_PREFIX %+ _extract_multiplier_2x20_win5
%xdefine _extract_multiplier_2x30_win5 _ %+ BORINGSSL_PREFIX %+ _extract_multiplier_2x30_win5
%xdefine _extract_multiplier_2x40_win5 _ %+ BORINGSSL_PREFIX %+ _extract_multiplier_2x40_win5
%xdefine _gcm_ghash_avx _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_avx
%xdefine _gcm_ghash_avx512 _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_avx512
%xdefine _gcm_ghash_clmul _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_clmul
%xdefine _gcm_ghash_neon _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_neon
%xdefine _gcm_ghash_nohw _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_nohw
%xdefine _gcm_ghash_p8 _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_p8
%xdefine _gcm_ghash_ssse3 _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_ssse3
%xdefine _gcm_ghash_v8 _ %+ BORINGSSL_PREFIX %+ _gcm_ghash_v8
%xdefine _gcm_gmult_avx _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_avx
%xdefine _gcm_gmult_avx512 _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_avx512
%xdefine _gcm_gmult_clmul _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_clmul
%xdefine _gcm_gmult_neon _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_neon
%xdefine _gcm_gmult_nohw _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_nohw
%xdefine _gcm_gmult_p8 _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_p8
%xdefine _gcm_gmult_ssse3 _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_ssse3
%xdefine _gcm_gmult_v8 _ %+ BORINGSSL_PREFIX %+ _gcm_gmult_v8
%xdefine _gcm_init_avx _ %+ BORINGSSL_PREFIX %+ _gcm_init_avx
%xdefine _gcm_init_avx512 _ %+ BORINGSSL_PREFIX %+ _gcm_init_avx512
%xdefine _gcm_init_clmul _ %+ BORINGSSL_PREFIX %+ _gcm_init_clmul
%xdefine _gcm_init_neon _ %+ BORINGSSL_PREFIX %+ _gcm_init_neon
%xdefine _gcm_init_nohw _ %+ BORINGSSL_PREFIX %+ _gcm_init_nohw
%xdefine _gcm_init_p8 _ %+ BORINGSSL_PREFIX %+ _gcm_init_p8
%xdefine _gcm_init_ssse3 _ %+ BORINGSSL_PREFIX %+ _gcm_init_ssse3
%xdefine _gcm_init_v8 _ %+ BORINGSSL_PREFIX %+ _gcm_init_v8
%xdefine _gcm_setiv_avx512 _ %+ BORINGSSL_PREFIX %+ _gcm_setiv_avx512
%xdefine _get_entropy_source _ %+ BORINGSSL_PREFIX %+ _get_entropy_source
%xdefine _get_entropy_source_method_id_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _get_entropy_source_method_id_FOR_TESTING
%xdefine _get_thread_and_global_tree_drbg_calls_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _get_thread_and_global_tree_drbg_calls_FOR_TESTING
%xdefine _get_thread_generate_calls_since_seed _ %+ BORINGSSL_PREFIX %+ _get_thread_generate_calls_since_seed
%xdefine _get_thread_reseed_calls_since_initialization _ %+ BORINGSSL_PREFIX %+ _get_thread_reseed_calls_since_initialization
%xdefine _handle_cpu_env _ %+ BORINGSSL_PREFIX %+ _handle_cpu_env
%xdefine _have_hw_rng_aarch64_for_testing _ %+ BORINGSSL_PREFIX %+ _have_hw_rng_aarch64_for_testing
%xdefine _have_hw_rng_x86_64_for_testing _ %+ BORINGSSL_PREFIX %+ _have_hw_rng_x86_64_for_testing
%xdefine _hmac_asn1_meth _ %+ BORINGSSL_PREFIX %+ _hmac_asn1_meth
%xdefine _i2a_ASN1_ENUMERATED _ %+ BORINGSSL_PREFIX %+ _i2a_ASN1_ENUMERATED
%xdefine _i2a_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _i2a_ASN1_INTEGER
%xdefine _i2a_ASN1_OBJECT _ %+ BORINGSSL_PREFIX %+ _i2a_ASN1_OBJECT
%xdefine _i2a_ASN1_STRING _ %+ BORINGSSL_PREFIX %+ _i2a_ASN1_STRING
%xdefine _i2c_ASN1_BIT_STRING _ %+ BORINGSSL_PREFIX %+ _i2c_ASN1_BIT_STRING
%xdefine _i2c_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _i2c_ASN1_INTEGER
%xdefine _i2d_ASN1_BIT_STRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_BIT_STRING
%xdefine _i2d_ASN1_BMPSTRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_BMPSTRING
%xdefine _i2d_ASN1_BOOLEAN _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_BOOLEAN
%xdefine _i2d_ASN1_ENUMERATED _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_ENUMERATED
%xdefine _i2d_ASN1_GENERALIZEDTIME _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_GENERALIZEDTIME
%xdefine _i2d_ASN1_GENERALSTRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_GENERALSTRING
%xdefine _i2d_ASN1_IA5STRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_IA5STRING
%xdefine _i2d_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_INTEGER
%xdefine _i2d_ASN1_NULL _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_NULL
%xdefine _i2d_ASN1_OBJECT _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_OBJECT
%xdefine _i2d_ASN1_OCTET_STRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_OCTET_STRING
%xdefine _i2d_ASN1_PRINTABLE _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_PRINTABLE
%xdefine _i2d_ASN1_PRINTABLESTRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_PRINTABLESTRING
%xdefine _i2d_ASN1_SEQUENCE_ANY _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_SEQUENCE_ANY
%xdefine _i2d_ASN1_SET_ANY _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_SET_ANY
%xdefine _i2d_ASN1_T61STRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_T61STRING
%xdefine _i2d_ASN1_TIME _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_TIME
%xdefine _i2d_ASN1_TYPE _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_TYPE
%xdefine _i2d_ASN1_UNIVERSALSTRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_UNIVERSALSTRING
%xdefine _i2d_ASN1_UTCTIME _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_UTCTIME
%xdefine _i2d_ASN1_UTF8STRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_UTF8STRING
%xdefine _i2d_ASN1_VISIBLESTRING _ %+ BORINGSSL_PREFIX %+ _i2d_ASN1_VISIBLESTRING
%xdefine _i2d_AUTHORITY_INFO_ACCESS _ %+ BORINGSSL_PREFIX %+ _i2d_AUTHORITY_INFO_ACCESS
%xdefine _i2d_AUTHORITY_KEYID _ %+ BORINGSSL_PREFIX %+ _i2d_AUTHORITY_KEYID
%xdefine _i2d_BASIC_CONSTRAINTS _ %+ BORINGSSL_PREFIX %+ _i2d_BASIC_CONSTRAINTS
%xdefine _i2d_CERTIFICATEPOLICIES _ %+ BORINGSSL_PREFIX %+ _i2d_CERTIFICATEPOLICIES
%xdefine _i2d_CRL_DIST_POINTS _ %+ BORINGSSL_PREFIX %+ _i2d_CRL_DIST_POINTS
%xdefine _i2d_DHparams _ %+ BORINGSSL_PREFIX %+ _i2d_DHparams
%xdefine _i2d_DHparams_bio _ %+ BORINGSSL_PREFIX %+ _i2d_DHparams_bio
%xdefine _i2d_DIRECTORYSTRING _ %+ BORINGSSL_PREFIX %+ _i2d_DIRECTORYSTRING
%xdefine _i2d_DISPLAYTEXT _ %+ BORINGSSL_PREFIX %+ _i2d_DISPLAYTEXT
%xdefine _i2d_DSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey
%xdefine _i2d_DSAPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey_bio
%xdefine _i2d_DSAPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey_fp
%xdefine _i2d_DSAPublicKey _ %+ BORINGSSL_PREFIX %+ _i2d_DSAPublicKey
%xdefine _i2d_DSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY
%xdefine _i2d_DSA_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY_bio
%xdefine _i2d_DSA_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY_fp
%xdefine _i2d_DSA_SIG _ %+ BORINGSSL_PREFIX %+ _i2d_DSA_SIG
%xdefine _i2d_DSAparams _ %+ BORINGSSL_PREFIX %+ _i2d_DSAparams
%xdefine _i2d_ECDSA_SIG _ %+ BORINGSSL_PREFIX %+ _i2d_ECDSA_SIG
%xdefine _i2d_ECPKParameters _ %+ BORINGSSL_PREFIX %+ _i2d_ECPKParameters
%xdefine _i2d_ECPKParameters_bio _ %+ BORINGSSL_PREFIX %+ _i2d_ECPKParameters_bio
%xdefine _i2d_ECParameters _ %+ BORINGSSL_PREFIX %+ _i2d_ECParameters
%xdefine _i2d_ECPrivateKey _ %+ BORINGSSL_PREFIX %+ _i2d_ECPrivateKey
%xdefine _i2d_ECPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_ECPrivateKey_bio
%xdefine _i2d_ECPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_ECPrivateKey_fp
%xdefine _i2d_EC_PUBKEY _ %+ BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY
%xdefine _i2d_EC_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY_bio
%xdefine _i2d_EC_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY_fp
%xdefine _i2d_EXTENDED_KEY_USAGE _ %+ BORINGSSL_PREFIX %+ _i2d_EXTENDED_KEY_USAGE
%xdefine _i2d_GENERAL_NAME _ %+ BORINGSSL_PREFIX %+ _i2d_GENERAL_NAME
%xdefine _i2d_GENERAL_NAMES _ %+ BORINGSSL_PREFIX %+ _i2d_GENERAL_NAMES
%xdefine _i2d_ISSUING_DIST_POINT _ %+ BORINGSSL_PREFIX %+ _i2d_ISSUING_DIST_POINT
%xdefine _i2d_NETSCAPE_SPKAC _ %+ BORINGSSL_PREFIX %+ _i2d_NETSCAPE_SPKAC
%xdefine _i2d_NETSCAPE_SPKI _ %+ BORINGSSL_PREFIX %+ _i2d_NETSCAPE_SPKI
%xdefine _i2d_OCSP_BASICRESP _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_BASICRESP
%xdefine _i2d_OCSP_CERTID _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_CERTID
%xdefine _i2d_OCSP_ONEREQ _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_ONEREQ
%xdefine _i2d_OCSP_REQINFO _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_REQINFO
%xdefine _i2d_OCSP_REQUEST _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_REQUEST
%xdefine _i2d_OCSP_REQUEST_bio _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_REQUEST_bio
%xdefine _i2d_OCSP_RESPBYTES _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_RESPBYTES
%xdefine _i2d_OCSP_RESPDATA _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_RESPDATA
%xdefine _i2d_OCSP_RESPONSE _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_RESPONSE
%xdefine _i2d_OCSP_RESPONSE_bio _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_RESPONSE_bio
%xdefine _i2d_OCSP_REVOKEDINFO _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_REVOKEDINFO
%xdefine _i2d_OCSP_SIGNATURE _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_SIGNATURE
%xdefine _i2d_OCSP_SINGLERESP _ %+ BORINGSSL_PREFIX %+ _i2d_OCSP_SINGLERESP
%xdefine _i2d_PKCS12 _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS12
%xdefine _i2d_PKCS12_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS12_bio
%xdefine _i2d_PKCS12_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS12_fp
%xdefine _i2d_PKCS7 _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7
%xdefine _i2d_PKCS7_DIGEST _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_DIGEST
%xdefine _i2d_PKCS7_ENCRYPT _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_ENCRYPT
%xdefine _i2d_PKCS7_ENC_CONTENT _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_ENC_CONTENT
%xdefine _i2d_PKCS7_ENVELOPE _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_ENVELOPE
%xdefine _i2d_PKCS7_ISSUER_AND_SERIAL _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_ISSUER_AND_SERIAL
%xdefine _i2d_PKCS7_RECIP_INFO _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_RECIP_INFO
%xdefine _i2d_PKCS7_SIGNED _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGNED
%xdefine _i2d_PKCS7_SIGNER_INFO _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGNER_INFO
%xdefine _i2d_PKCS7_SIGN_ENVELOPE _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGN_ENVELOPE
%xdefine _i2d_PKCS7_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS7_bio
%xdefine _i2d_PKCS8PrivateKeyInfo_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKeyInfo_bio
%xdefine _i2d_PKCS8PrivateKeyInfo_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKeyInfo_fp
%xdefine _i2d_PKCS8PrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_bio
%xdefine _i2d_PKCS8PrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_fp
%xdefine _i2d_PKCS8PrivateKey_nid_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_nid_bio
%xdefine _i2d_PKCS8PrivateKey_nid_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_nid_fp
%xdefine _i2d_PKCS8_PRIV_KEY_INFO _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO
%xdefine _i2d_PKCS8_PRIV_KEY_INFO_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO_bio
%xdefine _i2d_PKCS8_PRIV_KEY_INFO_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO_fp
%xdefine _i2d_PKCS8_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8_bio
%xdefine _i2d_PKCS8_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PKCS8_fp
%xdefine _i2d_PUBKEY _ %+ BORINGSSL_PREFIX %+ _i2d_PUBKEY
%xdefine _i2d_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PUBKEY_bio
%xdefine _i2d_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PUBKEY_fp
%xdefine _i2d_PrivateKey _ %+ BORINGSSL_PREFIX %+ _i2d_PrivateKey
%xdefine _i2d_PrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_PrivateKey_bio
%xdefine _i2d_PrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_PrivateKey_fp
%xdefine _i2d_PublicKey _ %+ BORINGSSL_PREFIX %+ _i2d_PublicKey
%xdefine _i2d_RSAPrivateKey _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey
%xdefine _i2d_RSAPrivateKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey_bio
%xdefine _i2d_RSAPrivateKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey_fp
%xdefine _i2d_RSAPublicKey _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPublicKey
%xdefine _i2d_RSAPublicKey_bio _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPublicKey_bio
%xdefine _i2d_RSAPublicKey_fp _ %+ BORINGSSL_PREFIX %+ _i2d_RSAPublicKey_fp
%xdefine _i2d_RSA_PSS_PARAMS _ %+ BORINGSSL_PREFIX %+ _i2d_RSA_PSS_PARAMS
%xdefine _i2d_RSA_PUBKEY _ %+ BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY
%xdefine _i2d_RSA_PUBKEY_bio _ %+ BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY_bio
%xdefine _i2d_RSA_PUBKEY_fp _ %+ BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY_fp
%xdefine _i2d_X509 _ %+ BORINGSSL_PREFIX %+ _i2d_X509
%xdefine _i2d_X509_ALGOR _ %+ BORINGSSL_PREFIX %+ _i2d_X509_ALGOR
%xdefine _i2d_X509_ATTRIBUTE _ %+ BORINGSSL_PREFIX %+ _i2d_X509_ATTRIBUTE
%xdefine _i2d_X509_AUX _ %+ BORINGSSL_PREFIX %+ _i2d_X509_AUX
%xdefine _i2d_X509_CERT_AUX _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CERT_AUX
%xdefine _i2d_X509_CINF _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CINF
%xdefine _i2d_X509_CRL _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CRL
%xdefine _i2d_X509_CRL_INFO _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CRL_INFO
%xdefine _i2d_X509_CRL_bio _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CRL_bio
%xdefine _i2d_X509_CRL_fp _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CRL_fp
%xdefine _i2d_X509_CRL_tbs _ %+ BORINGSSL_PREFIX %+ _i2d_X509_CRL_tbs
%xdefine _i2d_X509_EXTENSION _ %+ BORINGSSL_PREFIX %+ _i2d_X509_EXTENSION
%xdefine _i2d_X509_EXTENSIONS _ %+ BORINGSSL_PREFIX %+ _i2d_X509_EXTENSIONS
%xdefine _i2d_X509_NAME _ %+ BORINGSSL_PREFIX %+ _i2d_X509_NAME
%xdefine _i2d_X509_NAME_ENTRY _ %+ BORINGSSL_PREFIX %+ _i2d_X509_NAME_ENTRY
%xdefine _i2d_X509_PUBKEY _ %+ BORINGSSL_PREFIX %+ _i2d_X509_PUBKEY
%xdefine _i2d_X509_REQ _ %+ BORINGSSL_PREFIX %+ _i2d_X509_REQ
%xdefine _i2d_X509_REQ_INFO _ %+ BORINGSSL_PREFIX %+ _i2d_X509_REQ_INFO
%xdefine _i2d_X509_REQ_bio _ %+ BORINGSSL_PREFIX %+ _i2d_X509_REQ_bio
%xdefine _i2d_X509_REQ_fp _ %+ BORINGSSL_PREFIX %+ _i2d_X509_REQ_fp
%xdefine _i2d_X509_REVOKED _ %+ BORINGSSL_PREFIX %+ _i2d_X509_REVOKED
%xdefine _i2d_X509_SIG _ %+ BORINGSSL_PREFIX %+ _i2d_X509_SIG
%xdefine _i2d_X509_VAL _ %+ BORINGSSL_PREFIX %+ _i2d_X509_VAL
%xdefine _i2d_X509_bio _ %+ BORINGSSL_PREFIX %+ _i2d_X509_bio
%xdefine _i2d_X509_fp _ %+ BORINGSSL_PREFIX %+ _i2d_X509_fp
%xdefine _i2d_X509_tbs _ %+ BORINGSSL_PREFIX %+ _i2d_X509_tbs
%xdefine _i2d_re_X509_CRL_tbs _ %+ BORINGSSL_PREFIX %+ _i2d_re_X509_CRL_tbs
%xdefine _i2d_re_X509_REQ_tbs _ %+ BORINGSSL_PREFIX %+ _i2d_re_X509_REQ_tbs
%xdefine _i2d_re_X509_tbs _ %+ BORINGSSL_PREFIX %+ _i2d_re_X509_tbs
%xdefine _i2o_ECPublicKey _ %+ BORINGSSL_PREFIX %+ _i2o_ECPublicKey
%xdefine _i2s_ASN1_ENUMERATED _ %+ BORINGSSL_PREFIX %+ _i2s_ASN1_ENUMERATED
%xdefine _i2s_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _i2s_ASN1_INTEGER
%xdefine _i2s_ASN1_OCTET_STRING _ %+ BORINGSSL_PREFIX %+ _i2s_ASN1_OCTET_STRING
%xdefine _i2t_ASN1_OBJECT _ %+ BORINGSSL_PREFIX %+ _i2t_ASN1_OBJECT
%xdefine _i2v_GENERAL_NAME _ %+ BORINGSSL_PREFIX %+ _i2v_GENERAL_NAME
%xdefine _i2v_GENERAL_NAMES _ %+ BORINGSSL_PREFIX %+ _i2v_GENERAL_NAMES
%xdefine _is_a_tty _ %+ BORINGSSL_PREFIX %+ _is_a_tty
%xdefine _is_fips_build _ %+ BORINGSSL_PREFIX %+ _is_fips_build
%xdefine _is_public_component_of_rsa_key_good _ %+ BORINGSSL_PREFIX %+ _is_public_component_of_rsa_key_good
%xdefine _jent_apt_init _ %+ BORINGSSL_PREFIX %+ _jent_apt_init
%xdefine _jent_apt_reinit _ %+ BORINGSSL_PREFIX %+ _jent_apt_reinit
%xdefine _jent_entropy_collector_alloc _ %+ BORINGSSL_PREFIX %+ _jent_entropy_collector_alloc
%xdefine _jent_entropy_collector_free _ %+ BORINGSSL_PREFIX %+ _jent_entropy_collector_free
%xdefine _jent_entropy_init _ %+ BORINGSSL_PREFIX %+ _jent_entropy_init
%xdefine _jent_entropy_init_ex _ %+ BORINGSSL_PREFIX %+ _jent_entropy_init_ex
%xdefine _jent_entropy_switch_notime_impl _ %+ BORINGSSL_PREFIX %+ _jent_entropy_switch_notime_impl
%xdefine _jent_gcd_analyze _ %+ BORINGSSL_PREFIX %+ _jent_gcd_analyze
%xdefine _jent_gcd_fini _ %+ BORINGSSL_PREFIX %+ _jent_gcd_fini
%xdefine _jent_gcd_get _ %+ BORINGSSL_PREFIX %+ _jent_gcd_get
%xdefine _jent_gcd_init _ %+ BORINGSSL_PREFIX %+ _jent_gcd_init
%xdefine _jent_gcd_selftest _ %+ BORINGSSL_PREFIX %+ _jent_gcd_selftest
%xdefine _jent_health_cb_block_switch _ %+ BORINGSSL_PREFIX %+ _jent_health_cb_block_switch
%xdefine _jent_health_failure _ %+ BORINGSSL_PREFIX %+ _jent_health_failure
%xdefine _jent_lag_init _ %+ BORINGSSL_PREFIX %+ _jent_lag_init
%xdefine _jent_measure_jitter _ %+ BORINGSSL_PREFIX %+ _jent_measure_jitter
%xdefine _jent_random_data _ %+ BORINGSSL_PREFIX %+ _jent_random_data
%xdefine _jent_read_entropy _ %+ BORINGSSL_PREFIX %+ _jent_read_entropy
%xdefine _jent_read_entropy_safe _ %+ BORINGSSL_PREFIX %+ _jent_read_entropy_safe
%xdefine _jent_read_random_block _ %+ BORINGSSL_PREFIX %+ _jent_read_random_block
%xdefine _jent_set_fips_failure_callback _ %+ BORINGSSL_PREFIX %+ _jent_set_fips_failure_callback
%xdefine _jent_set_fips_failure_callback_internal _ %+ BORINGSSL_PREFIX %+ _jent_set_fips_failure_callback_internal
%xdefine _jent_sha3_256_init _ %+ BORINGSSL_PREFIX %+ _jent_sha3_256_init
%xdefine _jent_sha3_alloc _ %+ BORINGSSL_PREFIX %+ _jent_sha3_alloc
%xdefine _jent_sha3_dealloc _ %+ BORINGSSL_PREFIX %+ _jent_sha3_dealloc
%xdefine _jent_sha3_final _ %+ BORINGSSL_PREFIX %+ _jent_sha3_final
%xdefine _jent_sha3_tester _ %+ BORINGSSL_PREFIX %+ _jent_sha3_tester
%xdefine _jent_sha3_update _ %+ BORINGSSL_PREFIX %+ _jent_sha3_update
%xdefine _jent_stuck _ %+ BORINGSSL_PREFIX %+ _jent_stuck
%xdefine _jent_time_entropy_init _ %+ BORINGSSL_PREFIX %+ _jent_time_entropy_init
%xdefine _jent_version _ %+ BORINGSSL_PREFIX %+ _jent_version
%xdefine _kBoringSSLRSASqrtTwo _ %+ BORINGSSL_PREFIX %+ _kBoringSSLRSASqrtTwo
%xdefine _kBoringSSLRSASqrtTwoLen _ %+ BORINGSSL_PREFIX %+ _kBoringSSLRSASqrtTwoLen
%xdefine _kOpenSSLReasonStringData _ %+ BORINGSSL_PREFIX %+ _kOpenSSLReasonStringData
%xdefine _kOpenSSLReasonValues _ %+ BORINGSSL_PREFIX %+ _kOpenSSLReasonValues
%xdefine _kOpenSSLReasonValuesLen _ %+ BORINGSSL_PREFIX %+ _kOpenSSLReasonValuesLen
%xdefine _kem_asn1_meth _ %+ BORINGSSL_PREFIX %+ _kem_asn1_meth
%xdefine _lh_doall_arg _ %+ BORINGSSL_PREFIX %+ _lh_doall_arg
%xdefine _library_init_constructor _ %+ BORINGSSL_PREFIX %+ _library_init_constructor
%xdefine _md4_block_data_order _ %+ BORINGSSL_PREFIX %+ _md4_block_data_order
%xdefine _md5_block_asm_data_order _ %+ BORINGSSL_PREFIX %+ _md5_block_asm_data_order
%xdefine _ml_dsa_44_keypair _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_keypair
%xdefine _ml_dsa_44_keypair_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_keypair_internal
%xdefine _ml_dsa_44_keypair_internal_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_keypair_internal_no_self_test
%xdefine _ml_dsa_44_pack_pk_from_sk _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_pack_pk_from_sk
%xdefine _ml_dsa_44_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_sign
%xdefine _ml_dsa_44_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_sign_internal
%xdefine _ml_dsa_44_sign_internal_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_sign_internal_no_self_test
%xdefine _ml_dsa_44_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_verify
%xdefine _ml_dsa_44_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_verify_internal
%xdefine _ml_dsa_44_verify_internal_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_dsa_44_verify_internal_no_self_test
%xdefine _ml_dsa_65_keypair _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_keypair
%xdefine _ml_dsa_65_keypair_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_keypair_internal
%xdefine _ml_dsa_65_pack_pk_from_sk _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_pack_pk_from_sk
%xdefine _ml_dsa_65_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_sign
%xdefine _ml_dsa_65_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_sign_internal
%xdefine _ml_dsa_65_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_verify
%xdefine _ml_dsa_65_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_65_verify_internal
%xdefine _ml_dsa_87_keypair _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_keypair
%xdefine _ml_dsa_87_keypair_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_keypair_internal
%xdefine _ml_dsa_87_pack_pk_from_sk _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_pack_pk_from_sk
%xdefine _ml_dsa_87_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_sign
%xdefine _ml_dsa_87_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_sign_internal
%xdefine _ml_dsa_87_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_verify
%xdefine _ml_dsa_87_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_87_verify_internal
%xdefine _ml_dsa_extmu_44_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_sign
%xdefine _ml_dsa_extmu_44_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_sign_internal
%xdefine _ml_dsa_extmu_44_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_verify
%xdefine _ml_dsa_extmu_44_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_verify_internal
%xdefine _ml_dsa_extmu_65_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_sign
%xdefine _ml_dsa_extmu_65_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_sign_internal
%xdefine _ml_dsa_extmu_65_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_verify
%xdefine _ml_dsa_extmu_65_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_verify_internal
%xdefine _ml_dsa_extmu_87_sign _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_sign
%xdefine _ml_dsa_extmu_87_sign_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_sign_internal
%xdefine _ml_dsa_extmu_87_verify _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_verify
%xdefine _ml_dsa_extmu_87_verify_internal _ %+ BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_verify_internal
%xdefine _ml_kem_1024_check_pk _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_check_pk
%xdefine _ml_kem_1024_check_sk _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_check_sk
%xdefine _ml_kem_1024_decapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_decapsulate
%xdefine _ml_kem_1024_decapsulate_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_decapsulate_no_self_test
%xdefine _ml_kem_1024_encapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate
%xdefine _ml_kem_1024_encapsulate_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate_deterministic
%xdefine _ml_kem_1024_encapsulate_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate_deterministic_no_self_test
%xdefine _ml_kem_1024_keypair _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_keypair
%xdefine _ml_kem_1024_keypair_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_keypair_deterministic
%xdefine _ml_kem_1024_keypair_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_1024_keypair_deterministic_no_self_test
%xdefine _ml_kem_512_check_pk _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_check_pk
%xdefine _ml_kem_512_check_sk _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_check_sk
%xdefine _ml_kem_512_decapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_decapsulate
%xdefine _ml_kem_512_decapsulate_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_decapsulate_no_self_test
%xdefine _ml_kem_512_encapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate
%xdefine _ml_kem_512_encapsulate_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate_deterministic
%xdefine _ml_kem_512_encapsulate_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate_deterministic_no_self_test
%xdefine _ml_kem_512_keypair _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_keypair
%xdefine _ml_kem_512_keypair_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_keypair_deterministic
%xdefine _ml_kem_512_keypair_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_512_keypair_deterministic_no_self_test
%xdefine _ml_kem_768_check_pk _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_check_pk
%xdefine _ml_kem_768_check_sk _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_check_sk
%xdefine _ml_kem_768_decapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_decapsulate
%xdefine _ml_kem_768_decapsulate_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_decapsulate_no_self_test
%xdefine _ml_kem_768_encapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate
%xdefine _ml_kem_768_encapsulate_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate_deterministic
%xdefine _ml_kem_768_encapsulate_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate_deterministic_no_self_test
%xdefine _ml_kem_768_keypair _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_keypair
%xdefine _ml_kem_768_keypair_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_keypair_deterministic
%xdefine _ml_kem_768_keypair_deterministic_no_self_test _ %+ BORINGSSL_PREFIX %+ _ml_kem_768_keypair_deterministic_no_self_test
%xdefine _ml_kem_common_decapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_common_decapsulate
%xdefine _ml_kem_common_encapsulate _ %+ BORINGSSL_PREFIX %+ _ml_kem_common_encapsulate
%xdefine _ml_kem_common_encapsulate_deterministic _ %+ BORINGSSL_PREFIX %+ _ml_kem_common_encapsulate_deterministic
%xdefine _ml_kem_common_keypair _ %+ BORINGSSL_PREFIX %+ _ml_kem_common_keypair
%xdefine _mldsa_ct_opt_blocker_u64 _ %+ BORINGSSL_PREFIX %+ _mldsa_ct_opt_blocker_u64
%xdefine _mlkem_aarch64_invntt_zetas_layer12345 _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_invntt_zetas_layer12345
%xdefine _mlkem_aarch64_invntt_zetas_layer67 _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_invntt_zetas_layer67
%xdefine _mlkem_aarch64_ntt_zetas_layer12345 _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_ntt_zetas_layer12345
%xdefine _mlkem_aarch64_ntt_zetas_layer67 _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_ntt_zetas_layer67
%xdefine _mlkem_aarch64_zetas_mulcache_native _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_zetas_mulcache_native
%xdefine _mlkem_aarch64_zetas_mulcache_twisted_native _ %+ BORINGSSL_PREFIX %+ _mlkem_aarch64_zetas_mulcache_twisted_native
%xdefine _mlkem_ct_opt_blocker_u64 _ %+ BORINGSSL_PREFIX %+ _mlkem_ct_opt_blocker_u64
%xdefine _mlkem_intt_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_intt_asm
%xdefine _mlkem_invntt_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_invntt_avx2
%xdefine _mlkem_ntt_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_ntt_asm
%xdefine _mlkem_ntt_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_ntt_avx2
%xdefine _mlkem_nttfrombytes_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_nttfrombytes_avx2
%xdefine _mlkem_ntttobytes_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_ntttobytes_avx2
%xdefine _mlkem_nttunpack_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_nttunpack_avx2
%xdefine _mlkem_poly_mulcache_compute_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_poly_mulcache_compute_asm
%xdefine _mlkem_poly_mulcache_compute_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_poly_mulcache_compute_avx2
%xdefine _mlkem_poly_reduce_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_poly_reduce_asm
%xdefine _mlkem_poly_tobytes_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_poly_tobytes_asm
%xdefine _mlkem_poly_tomont_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_poly_tomont_asm
%xdefine _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k2 _ %+ BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k2
%xdefine _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k3 _ %+ BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k3
%xdefine _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k4 _ %+ BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k4
%xdefine _mlkem_qdata _ %+ BORINGSSL_PREFIX %+ _mlkem_qdata
%xdefine _mlkem_reduce_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_reduce_avx2
%xdefine _mlkem_rej_uniform_asm _ %+ BORINGSSL_PREFIX %+ _mlkem_rej_uniform_asm
%xdefine _mlkem_rej_uniform_table _ %+ BORINGSSL_PREFIX %+ _mlkem_rej_uniform_table
%xdefine _mlkem_tomont_avx2 _ %+ BORINGSSL_PREFIX %+ _mlkem_tomont_avx2
%xdefine _o2i_ECPublicKey _ %+ BORINGSSL_PREFIX %+ _o2i_ECPublicKey
%xdefine _openssl_console_acquire_mutex _ %+ BORINGSSL_PREFIX %+ _openssl_console_acquire_mutex
%xdefine _openssl_console_close _ %+ BORINGSSL_PREFIX %+ _openssl_console_close
%xdefine _openssl_console_open _ %+ BORINGSSL_PREFIX %+ _openssl_console_open
%xdefine _openssl_console_read _ %+ BORINGSSL_PREFIX %+ _openssl_console_read
%xdefine _openssl_console_release_mutex _ %+ BORINGSSL_PREFIX %+ _openssl_console_release_mutex
%xdefine _openssl_console_write _ %+ BORINGSSL_PREFIX %+ _openssl_console_write
%xdefine _openssl_poly1305_neon2_addmulmod _ %+ BORINGSSL_PREFIX %+ _openssl_poly1305_neon2_addmulmod
%xdefine _openssl_poly1305_neon2_blocks _ %+ BORINGSSL_PREFIX %+ _openssl_poly1305_neon2_blocks
%xdefine _override_entropy_source_method_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _override_entropy_source_method_FOR_TESTING
%xdefine _p256_methods _ %+ BORINGSSL_PREFIX %+ _p256_methods
%xdefine _p256_montjscalarmul _ %+ BORINGSSL_PREFIX %+ _p256_montjscalarmul
%xdefine _p256_montjscalarmul_alt _ %+ BORINGSSL_PREFIX %+ _p256_montjscalarmul_alt
%xdefine _p384_methods _ %+ BORINGSSL_PREFIX %+ _p384_methods
%xdefine _p384_montjdouble _ %+ BORINGSSL_PREFIX %+ _p384_montjdouble
%xdefine _p384_montjdouble_alt _ %+ BORINGSSL_PREFIX %+ _p384_montjdouble_alt
%xdefine _p384_montjscalarmul _ %+ BORINGSSL_PREFIX %+ _p384_montjscalarmul
%xdefine _p384_montjscalarmul_alt _ %+ BORINGSSL_PREFIX %+ _p384_montjscalarmul_alt
%xdefine _p521_jdouble _ %+ BORINGSSL_PREFIX %+ _p521_jdouble
%xdefine _p521_jdouble_alt _ %+ BORINGSSL_PREFIX %+ _p521_jdouble_alt
%xdefine _p521_jscalarmul _ %+ BORINGSSL_PREFIX %+ _p521_jscalarmul
%xdefine _p521_jscalarmul_alt _ %+ BORINGSSL_PREFIX %+ _p521_jscalarmul_alt
%xdefine _p521_methods _ %+ BORINGSSL_PREFIX %+ _p521_methods
%xdefine _p_thread_callback_boringssl _ %+ BORINGSSL_PREFIX %+ _p_thread_callback_boringssl
%xdefine _pkcs12_iterations_acceptable _ %+ BORINGSSL_PREFIX %+ _pkcs12_iterations_acceptable
%xdefine _pkcs12_key_gen _ %+ BORINGSSL_PREFIX %+ _pkcs12_key_gen
%xdefine _pkcs12_pbe_encrypt_init _ %+ BORINGSSL_PREFIX %+ _pkcs12_pbe_encrypt_init
%xdefine _pkcs7_add_signed_data _ %+ BORINGSSL_PREFIX %+ _pkcs7_add_signed_data
%xdefine _pkcs7_final _ %+ BORINGSSL_PREFIX %+ _pkcs7_final
%xdefine _pkcs7_parse_header _ %+ BORINGSSL_PREFIX %+ _pkcs7_parse_header
%xdefine _pkcs8_pbe_decrypt _ %+ BORINGSSL_PREFIX %+ _pkcs8_pbe_decrypt
%xdefine _pmbtoken_exp1_blind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_blind
%xdefine _pmbtoken_exp1_client_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_client_key_from_bytes
%xdefine _pmbtoken_exp1_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_derive_key_from_secret
%xdefine _pmbtoken_exp1_generate_key _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_generate_key
%xdefine _pmbtoken_exp1_get_h_for_testing _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_get_h_for_testing
%xdefine _pmbtoken_exp1_issuer_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_issuer_key_from_bytes
%xdefine _pmbtoken_exp1_read _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_read
%xdefine _pmbtoken_exp1_sign _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_sign
%xdefine _pmbtoken_exp1_unblind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp1_unblind
%xdefine _pmbtoken_exp2_blind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_blind
%xdefine _pmbtoken_exp2_client_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_client_key_from_bytes
%xdefine _pmbtoken_exp2_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_derive_key_from_secret
%xdefine _pmbtoken_exp2_generate_key _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_generate_key
%xdefine _pmbtoken_exp2_get_h_for_testing _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_get_h_for_testing
%xdefine _pmbtoken_exp2_issuer_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_issuer_key_from_bytes
%xdefine _pmbtoken_exp2_read _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_read
%xdefine _pmbtoken_exp2_sign _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_sign
%xdefine _pmbtoken_exp2_unblind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_exp2_unblind
%xdefine _pmbtoken_pst1_blind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_blind
%xdefine _pmbtoken_pst1_client_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_client_key_from_bytes
%xdefine _pmbtoken_pst1_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_derive_key_from_secret
%xdefine _pmbtoken_pst1_generate_key _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_generate_key
%xdefine _pmbtoken_pst1_get_h_for_testing _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_get_h_for_testing
%xdefine _pmbtoken_pst1_issuer_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_issuer_key_from_bytes
%xdefine _pmbtoken_pst1_read _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_read
%xdefine _pmbtoken_pst1_sign _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_sign
%xdefine _pmbtoken_pst1_unblind _ %+ BORINGSSL_PREFIX %+ _pmbtoken_pst1_unblind
%xdefine _poly_Rq_mul _ %+ BORINGSSL_PREFIX %+ _poly_Rq_mul
%xdefine _pqdsa_asn1_meth _ %+ BORINGSSL_PREFIX %+ _pqdsa_asn1_meth
%xdefine _rand_fips_library_destructor _ %+ BORINGSSL_PREFIX %+ _rand_fips_library_destructor
%xdefine _rdrand_multiple8 _ %+ BORINGSSL_PREFIX %+ _rdrand_multiple8
%xdefine _rndr_multiple8 _ %+ BORINGSSL_PREFIX %+ _rndr_multiple8
%xdefine _rsa_asn1_meth _ %+ BORINGSSL_PREFIX %+ _rsa_asn1_meth
%xdefine _rsa_default_private_transform _ %+ BORINGSSL_PREFIX %+ _rsa_default_private_transform
%xdefine _rsa_default_sign_raw _ %+ BORINGSSL_PREFIX %+ _rsa_default_sign_raw
%xdefine _rsa_default_size _ %+ BORINGSSL_PREFIX %+ _rsa_default_size
%xdefine _rsa_digestsign_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_digestsign_no_self_test
%xdefine _rsa_digestverify_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_digestverify_no_self_test
%xdefine _rsa_invalidate_key _ %+ BORINGSSL_PREFIX %+ _rsa_invalidate_key
%xdefine _rsa_private_transform _ %+ BORINGSSL_PREFIX %+ _rsa_private_transform
%xdefine _rsa_private_transform_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_private_transform_no_self_test
%xdefine _rsa_pss_asn1_meth _ %+ BORINGSSL_PREFIX %+ _rsa_pss_asn1_meth
%xdefine _rsa_sign_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_sign_no_self_test
%xdefine _rsa_verify_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_verify_no_self_test
%xdefine _rsa_verify_raw_no_self_test _ %+ BORINGSSL_PREFIX %+ _rsa_verify_raw_no_self_test
%xdefine _rsaz_1024_gather5_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_gather5_avx2
%xdefine _rsaz_1024_mul_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_mul_avx2
%xdefine _rsaz_1024_norm2red_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_norm2red_avx2
%xdefine _rsaz_1024_red2norm_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_red2norm_avx2
%xdefine _rsaz_1024_scatter5_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_scatter5_avx2
%xdefine _rsaz_1024_sqr_avx2 _ %+ BORINGSSL_PREFIX %+ _rsaz_1024_sqr_avx2
%xdefine _rsaz_amm52x20_x1_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x20_x1_ifma256
%xdefine _rsaz_amm52x20_x2_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x20_x2_ifma256
%xdefine _rsaz_amm52x30_x1_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x30_x1_ifma256
%xdefine _rsaz_amm52x30_x2_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x30_x2_ifma256
%xdefine _rsaz_amm52x40_x1_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x40_x1_ifma256
%xdefine _rsaz_amm52x40_x2_ifma256 _ %+ BORINGSSL_PREFIX %+ _rsaz_amm52x40_x2_ifma256
%xdefine _s2i_ASN1_INTEGER _ %+ BORINGSSL_PREFIX %+ _s2i_ASN1_INTEGER
%xdefine _s2i_ASN1_OCTET_STRING _ %+ BORINGSSL_PREFIX %+ _s2i_ASN1_OCTET_STRING
%xdefine _set_fork_ube_generation_number_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _set_fork_ube_generation_number_FOR_TESTING
%xdefine _set_thread_and_global_tree_drbg_reseed_counter_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _set_thread_and_global_tree_drbg_reseed_counter_FOR_TESTING
%xdefine _set_vm_ube_generation_number_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _set_vm_ube_generation_number_FOR_TESTING
%xdefine _sha1_block_data_order _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order
%xdefine _sha1_block_data_order_avx _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_avx
%xdefine _sha1_block_data_order_avx2 _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_avx2
%xdefine _sha1_block_data_order_hw _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_hw
%xdefine _sha1_block_data_order_neon _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_neon
%xdefine _sha1_block_data_order_nohw _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_nohw
%xdefine _sha1_block_data_order_ssse3 _ %+ BORINGSSL_PREFIX %+ _sha1_block_data_order_ssse3
%xdefine _sha1_func _ %+ BORINGSSL_PREFIX %+ _sha1_func
%xdefine _sha224_func _ %+ BORINGSSL_PREFIX %+ _sha224_func
%xdefine _sha256_block_data_order_avx _ %+ BORINGSSL_PREFIX %+ _sha256_block_data_order_avx
%xdefine _sha256_block_data_order_hw _ %+ BORINGSSL_PREFIX %+ _sha256_block_data_order_hw
%xdefine _sha256_block_data_order_neon _ %+ BORINGSSL_PREFIX %+ _sha256_block_data_order_neon
%xdefine _sha256_block_data_order_nohw _ %+ BORINGSSL_PREFIX %+ _sha256_block_data_order_nohw
%xdefine _sha256_block_data_order_ssse3 _ %+ BORINGSSL_PREFIX %+ _sha256_block_data_order_ssse3
%xdefine _sha256_func _ %+ BORINGSSL_PREFIX %+ _sha256_func
%xdefine _sha384_func _ %+ BORINGSSL_PREFIX %+ _sha384_func
%xdefine _sha3_keccak2_f1600 _ %+ BORINGSSL_PREFIX %+ _sha3_keccak2_f1600
%xdefine _sha3_keccak4_f1600_alt _ %+ BORINGSSL_PREFIX %+ _sha3_keccak4_f1600_alt
%xdefine _sha3_keccak4_f1600_alt2 _ %+ BORINGSSL_PREFIX %+ _sha3_keccak4_f1600_alt2
%xdefine _sha3_keccak_f1600 _ %+ BORINGSSL_PREFIX %+ _sha3_keccak_f1600
%xdefine _sha3_keccak_f1600_alt _ %+ BORINGSSL_PREFIX %+ _sha3_keccak_f1600_alt
%xdefine _sha512_block_data_order _ %+ BORINGSSL_PREFIX %+ _sha512_block_data_order
%xdefine _sha512_block_data_order_avx _ %+ BORINGSSL_PREFIX %+ _sha512_block_data_order_avx
%xdefine _sha512_block_data_order_hw _ %+ BORINGSSL_PREFIX %+ _sha512_block_data_order_hw
%xdefine _sha512_block_data_order_neon _ %+ BORINGSSL_PREFIX %+ _sha512_block_data_order_neon
%xdefine _sha512_block_data_order_nohw _ %+ BORINGSSL_PREFIX %+ _sha512_block_data_order_nohw
%xdefine _sha512_func _ %+ BORINGSSL_PREFIX %+ _sha512_func
%xdefine _sk_pop_free _ %+ BORINGSSL_PREFIX %+ _sk_pop_free
%xdefine _sskdf_variant_digest _ %+ BORINGSSL_PREFIX %+ _sskdf_variant_digest
%xdefine _sskdf_variant_hmac _ %+ BORINGSSL_PREFIX %+ _sskdf_variant_hmac
%xdefine _tree_jitter_drbg_destructor _ %+ BORINGSSL_PREFIX %+ _tree_jitter_drbg_destructor
%xdefine _tree_jitter_free_thread_drbg _ %+ BORINGSSL_PREFIX %+ _tree_jitter_free_thread_drbg
%xdefine _tree_jitter_get_seed _ %+ BORINGSSL_PREFIX %+ _tree_jitter_get_seed
%xdefine _tree_jitter_initialize _ %+ BORINGSSL_PREFIX %+ _tree_jitter_initialize
%xdefine _tree_jitter_zeroize_thread_drbg _ %+ BORINGSSL_PREFIX %+ _tree_jitter_zeroize_thread_drbg
%xdefine _tty_in _ %+ BORINGSSL_PREFIX %+ _tty_in
%xdefine _tty_new _ %+ BORINGSSL_PREFIX %+ _tty_new
%xdefine _tty_orig _ %+ BORINGSSL_PREFIX %+ _tty_orig
%xdefine _tty_out _ %+ BORINGSSL_PREFIX %+ _tty_out
%xdefine _used_for_hmac _ %+ BORINGSSL_PREFIX %+ _used_for_hmac
%xdefine _v2i_GENERAL_NAME _ %+ BORINGSSL_PREFIX %+ _v2i_GENERAL_NAME
%xdefine _v2i_GENERAL_NAMES _ %+ BORINGSSL_PREFIX %+ _v2i_GENERAL_NAMES
%xdefine _v2i_GENERAL_NAME_ex _ %+ BORINGSSL_PREFIX %+ _v2i_GENERAL_NAME_ex
%xdefine _v3_akey_id _ %+ BORINGSSL_PREFIX %+ _v3_akey_id
%xdefine _v3_alt _ %+ BORINGSSL_PREFIX %+ _v3_alt
%xdefine _v3_bcons _ %+ BORINGSSL_PREFIX %+ _v3_bcons
%xdefine _v3_cpols _ %+ BORINGSSL_PREFIX %+ _v3_cpols
%xdefine _v3_crl_invdate _ %+ BORINGSSL_PREFIX %+ _v3_crl_invdate
%xdefine _v3_crl_num _ %+ BORINGSSL_PREFIX %+ _v3_crl_num
%xdefine _v3_crl_reason _ %+ BORINGSSL_PREFIX %+ _v3_crl_reason
%xdefine _v3_crld _ %+ BORINGSSL_PREFIX %+ _v3_crld
%xdefine _v3_delta_crl _ %+ BORINGSSL_PREFIX %+ _v3_delta_crl
%xdefine _v3_ext_ku _ %+ BORINGSSL_PREFIX %+ _v3_ext_ku
%xdefine _v3_freshest_crl _ %+ BORINGSSL_PREFIX %+ _v3_freshest_crl
%xdefine _v3_idp _ %+ BORINGSSL_PREFIX %+ _v3_idp
%xdefine _v3_info _ %+ BORINGSSL_PREFIX %+ _v3_info
%xdefine _v3_inhibit_anyp _ %+ BORINGSSL_PREFIX %+ _v3_inhibit_anyp
%xdefine _v3_key_usage _ %+ BORINGSSL_PREFIX %+ _v3_key_usage
%xdefine _v3_name_constraints _ %+ BORINGSSL_PREFIX %+ _v3_name_constraints
%xdefine _v3_ns_ia5_list _ %+ BORINGSSL_PREFIX %+ _v3_ns_ia5_list
%xdefine _v3_nscert _ %+ BORINGSSL_PREFIX %+ _v3_nscert
%xdefine _v3_ocsp_accresp _ %+ BORINGSSL_PREFIX %+ _v3_ocsp_accresp
%xdefine _v3_ocsp_nocheck _ %+ BORINGSSL_PREFIX %+ _v3_ocsp_nocheck
%xdefine _v3_ocsp_nonce _ %+ BORINGSSL_PREFIX %+ _v3_ocsp_nonce
%xdefine _v3_policy_constraints _ %+ BORINGSSL_PREFIX %+ _v3_policy_constraints
%xdefine _v3_policy_mappings _ %+ BORINGSSL_PREFIX %+ _v3_policy_mappings
%xdefine _v3_sinfo _ %+ BORINGSSL_PREFIX %+ _v3_sinfo
%xdefine _v3_skey_id _ %+ BORINGSSL_PREFIX %+ _v3_skey_id
%xdefine _validate_cidr_mask _ %+ BORINGSSL_PREFIX %+ _validate_cidr_mask
%xdefine _vm_ube_fallback_get_seed _ %+ BORINGSSL_PREFIX %+ _vm_ube_fallback_get_seed
%xdefine _voprf_exp2_blind _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_blind
%xdefine _voprf_exp2_client_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_client_key_from_bytes
%xdefine _voprf_exp2_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_derive_key_from_secret
%xdefine _voprf_exp2_generate_key _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_generate_key
%xdefine _voprf_exp2_issuer_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_issuer_key_from_bytes
%xdefine _voprf_exp2_read _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_read
%xdefine _voprf_exp2_sign _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_sign
%xdefine _voprf_exp2_unblind _ %+ BORINGSSL_PREFIX %+ _voprf_exp2_unblind
%xdefine _voprf_pst1_blind _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_blind
%xdefine _voprf_pst1_client_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_client_key_from_bytes
%xdefine _voprf_pst1_derive_key_from_secret _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_derive_key_from_secret
%xdefine _voprf_pst1_generate_key _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_generate_key
%xdefine _voprf_pst1_issuer_key_from_bytes _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_issuer_key_from_bytes
%xdefine _voprf_pst1_read _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_read
%xdefine _voprf_pst1_sign _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_sign
%xdefine _voprf_pst1_sign_with_proof_scalar_for_testing _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_sign_with_proof_scalar_for_testing
%xdefine _voprf_pst1_unblind _ %+ BORINGSSL_PREFIX %+ _voprf_pst1_unblind
%xdefine _vpaes_cbc_encrypt _ %+ BORINGSSL_PREFIX %+ _vpaes_cbc_encrypt
%xdefine _vpaes_ctr32_encrypt_blocks _ %+ BORINGSSL_PREFIX %+ _vpaes_ctr32_encrypt_blocks
%xdefine _vpaes_decrypt _ %+ BORINGSSL_PREFIX %+ _vpaes_decrypt
%xdefine _vpaes_decrypt_key_to_bsaes _ %+ BORINGSSL_PREFIX %+ _vpaes_decrypt_key_to_bsaes
%xdefine _vpaes_encrypt _ %+ BORINGSSL_PREFIX %+ _vpaes_encrypt
%xdefine _vpaes_encrypt_key_to_bsaes _ %+ BORINGSSL_PREFIX %+ _vpaes_encrypt_key_to_bsaes
%xdefine _vpaes_set_decrypt_key _ %+ BORINGSSL_PREFIX %+ _vpaes_set_decrypt_key
%xdefine _vpaes_set_encrypt_key _ %+ BORINGSSL_PREFIX %+ _vpaes_set_encrypt_key
%xdefine _x25519_asn1_meth _ %+ BORINGSSL_PREFIX %+ _x25519_asn1_meth
%xdefine _x25519_ge_add _ %+ BORINGSSL_PREFIX %+ _x25519_ge_add
%xdefine _x25519_ge_frombytes_vartime _ %+ BORINGSSL_PREFIX %+ _x25519_ge_frombytes_vartime
%xdefine _x25519_ge_p1p1_to_p2 _ %+ BORINGSSL_PREFIX %+ _x25519_ge_p1p1_to_p2
%xdefine _x25519_ge_p1p1_to_p3 _ %+ BORINGSSL_PREFIX %+ _x25519_ge_p1p1_to_p3
%xdefine _x25519_ge_p3_to_cached _ %+ BORINGSSL_PREFIX %+ _x25519_ge_p3_to_cached
%xdefine _x25519_ge_scalarmult _ %+ BORINGSSL_PREFIX %+ _x25519_ge_scalarmult
%xdefine _x25519_ge_scalarmult_base _ %+ BORINGSSL_PREFIX %+ _x25519_ge_scalarmult_base
%xdefine _x25519_ge_scalarmult_small_precomp _ %+ BORINGSSL_PREFIX %+ _x25519_ge_scalarmult_small_precomp
%xdefine _x25519_ge_sub _ %+ BORINGSSL_PREFIX %+ _x25519_ge_sub
%xdefine _x25519_ge_tobytes _ %+ BORINGSSL_PREFIX %+ _x25519_ge_tobytes
%xdefine _x25519_pkey_meth _ %+ BORINGSSL_PREFIX %+ _x25519_pkey_meth
%xdefine _x25519_public_from_private_nohw _ %+ BORINGSSL_PREFIX %+ _x25519_public_from_private_nohw
%xdefine _x25519_public_from_private_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _x25519_public_from_private_s2n_bignum
%xdefine _x25519_sc_reduce _ %+ BORINGSSL_PREFIX %+ _x25519_sc_reduce
%xdefine _x25519_scalar_mult_generic_nohw _ %+ BORINGSSL_PREFIX %+ _x25519_scalar_mult_generic_nohw
%xdefine _x25519_scalar_mult_generic_s2n_bignum _ %+ BORINGSSL_PREFIX %+ _x25519_scalar_mult_generic_s2n_bignum
%xdefine _x509V3_add_value_asn1_string _ %+ BORINGSSL_PREFIX %+ _x509V3_add_value_asn1_string
%xdefine _x509_check_cert_time _ %+ BORINGSSL_PREFIX %+ _x509_check_cert_time
%xdefine _x509_check_issued_with_callback _ %+ BORINGSSL_PREFIX %+ _x509_check_issued_with_callback
%xdefine _x509_digest_sign_algorithm _ %+ BORINGSSL_PREFIX %+ _x509_digest_sign_algorithm
%xdefine _x509_digest_verify_init _ %+ BORINGSSL_PREFIX %+ _x509_digest_verify_init
%xdefine _x509_init_signature_info _ %+ BORINGSSL_PREFIX %+ _x509_init_signature_info
%xdefine _x509_print_rsa_pss_params _ %+ BORINGSSL_PREFIX %+ _x509_print_rsa_pss_params
%xdefine _x509_rsa_ctx_to_pss _ %+ BORINGSSL_PREFIX %+ _x509_rsa_ctx_to_pss
%xdefine _x509_rsa_pss_to_ctx _ %+ BORINGSSL_PREFIX %+ _x509_rsa_pss_to_ctx
%xdefine _x509v3_a2i_ipadd _ %+ BORINGSSL_PREFIX %+ _x509v3_a2i_ipadd
%xdefine _x509v3_bytes_to_hex _ %+ BORINGSSL_PREFIX %+ _x509v3_bytes_to_hex
%xdefine _x509v3_cache_extensions _ %+ BORINGSSL_PREFIX %+ _x509v3_cache_extensions
%xdefine _x509v3_conf_name_matches _ %+ BORINGSSL_PREFIX %+ _x509v3_conf_name_matches
%xdefine _x509v3_ext_free_with_method _ %+ BORINGSSL_PREFIX %+ _x509v3_ext_free_with_method
%xdefine _x509v3_hex_to_bytes _ %+ BORINGSSL_PREFIX %+ _x509v3_hex_to_bytes
%xdefine _x86_64_assembly_implementation_FOR_TESTING _ %+ BORINGSSL_PREFIX %+ _x86_64_assembly_implementation_FOR_TESTING
%else
%xdefine ACCESS_DESCRIPTION_free BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_free
%xdefine ACCESS_DESCRIPTION_it BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_it
%xdefine ACCESS_DESCRIPTION_new BORINGSSL_PREFIX %+ _ACCESS_DESCRIPTION_new
%xdefine AES_CMAC BORINGSSL_PREFIX %+ _AES_CMAC
%xdefine AES_cbc_encrypt BORINGSSL_PREFIX %+ _AES_cbc_encrypt
%xdefine AES_cfb128_encrypt BORINGSSL_PREFIX %+ _AES_cfb128_encrypt
%xdefine AES_cfb1_encrypt BORINGSSL_PREFIX %+ _AES_cfb1_encrypt
%xdefine AES_cfb8_encrypt BORINGSSL_PREFIX %+ _AES_cfb8_encrypt
%xdefine AES_ctr128_encrypt BORINGSSL_PREFIX %+ _AES_ctr128_encrypt
%xdefine AES_decrypt BORINGSSL_PREFIX %+ _AES_decrypt
%xdefine AES_ecb_encrypt BORINGSSL_PREFIX %+ _AES_ecb_encrypt
%xdefine AES_encrypt BORINGSSL_PREFIX %+ _AES_encrypt
%xdefine AES_ofb128_encrypt BORINGSSL_PREFIX %+ _AES_ofb128_encrypt
%xdefine AES_set_decrypt_key BORINGSSL_PREFIX %+ _AES_set_decrypt_key
%xdefine AES_set_encrypt_key BORINGSSL_PREFIX %+ _AES_set_encrypt_key
%xdefine AES_unwrap_key BORINGSSL_PREFIX %+ _AES_unwrap_key
%xdefine AES_unwrap_key_padded BORINGSSL_PREFIX %+ _AES_unwrap_key_padded
%xdefine AES_wrap_key BORINGSSL_PREFIX %+ _AES_wrap_key
%xdefine AES_wrap_key_padded BORINGSSL_PREFIX %+ _AES_wrap_key_padded
%xdefine ASN1_ANY_it BORINGSSL_PREFIX %+ _ASN1_ANY_it
%xdefine ASN1_BIT_STRING_check BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_check
%xdefine ASN1_BIT_STRING_free BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_free
%xdefine ASN1_BIT_STRING_get_bit BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_get_bit
%xdefine ASN1_BIT_STRING_it BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_it
%xdefine ASN1_BIT_STRING_new BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_new
%xdefine ASN1_BIT_STRING_num_bytes BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_num_bytes
%xdefine ASN1_BIT_STRING_set BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_set
%xdefine ASN1_BIT_STRING_set_bit BORINGSSL_PREFIX %+ _ASN1_BIT_STRING_set_bit
%xdefine ASN1_BMPSTRING_free BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_free
%xdefine ASN1_BMPSTRING_it BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_it
%xdefine ASN1_BMPSTRING_new BORINGSSL_PREFIX %+ _ASN1_BMPSTRING_new
%xdefine ASN1_BOOLEAN_it BORINGSSL_PREFIX %+ _ASN1_BOOLEAN_it
%xdefine ASN1_ENUMERATED_free BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_free
%xdefine ASN1_ENUMERATED_get BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get
%xdefine ASN1_ENUMERATED_get_int64 BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get_int64
%xdefine ASN1_ENUMERATED_get_uint64 BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_get_uint64
%xdefine ASN1_ENUMERATED_it BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_it
%xdefine ASN1_ENUMERATED_new BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_new
%xdefine ASN1_ENUMERATED_set BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set
%xdefine ASN1_ENUMERATED_set_int64 BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set_int64
%xdefine ASN1_ENUMERATED_set_uint64 BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_set_uint64
%xdefine ASN1_ENUMERATED_to_BN BORINGSSL_PREFIX %+ _ASN1_ENUMERATED_to_BN
%xdefine ASN1_FBOOLEAN_it BORINGSSL_PREFIX %+ _ASN1_FBOOLEAN_it
%xdefine ASN1_GENERALIZEDTIME_adj BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_adj
%xdefine ASN1_GENERALIZEDTIME_check BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_check
%xdefine ASN1_GENERALIZEDTIME_free BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_free
%xdefine ASN1_GENERALIZEDTIME_it BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_it
%xdefine ASN1_GENERALIZEDTIME_new BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_new
%xdefine ASN1_GENERALIZEDTIME_print BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_print
%xdefine ASN1_GENERALIZEDTIME_set BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_set
%xdefine ASN1_GENERALIZEDTIME_set_string BORINGSSL_PREFIX %+ _ASN1_GENERALIZEDTIME_set_string
%xdefine ASN1_GENERALSTRING_free BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_free
%xdefine ASN1_GENERALSTRING_it BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_it
%xdefine ASN1_GENERALSTRING_new BORINGSSL_PREFIX %+ _ASN1_GENERALSTRING_new
%xdefine ASN1_IA5STRING_free BORINGSSL_PREFIX %+ _ASN1_IA5STRING_free
%xdefine ASN1_IA5STRING_it BORINGSSL_PREFIX %+ _ASN1_IA5STRING_it
%xdefine ASN1_IA5STRING_new BORINGSSL_PREFIX %+ _ASN1_IA5STRING_new
%xdefine ASN1_INTEGER_cmp BORINGSSL_PREFIX %+ _ASN1_INTEGER_cmp
%xdefine ASN1_INTEGER_dup BORINGSSL_PREFIX %+ _ASN1_INTEGER_dup
%xdefine ASN1_INTEGER_free BORINGSSL_PREFIX %+ _ASN1_INTEGER_free
%xdefine ASN1_INTEGER_get BORINGSSL_PREFIX %+ _ASN1_INTEGER_get
%xdefine ASN1_INTEGER_get_int64 BORINGSSL_PREFIX %+ _ASN1_INTEGER_get_int64
%xdefine ASN1_INTEGER_get_uint64 BORINGSSL_PREFIX %+ _ASN1_INTEGER_get_uint64
%xdefine ASN1_INTEGER_it BORINGSSL_PREFIX %+ _ASN1_INTEGER_it
%xdefine ASN1_INTEGER_new BORINGSSL_PREFIX %+ _ASN1_INTEGER_new
%xdefine ASN1_INTEGER_set BORINGSSL_PREFIX %+ _ASN1_INTEGER_set
%xdefine ASN1_INTEGER_set_int64 BORINGSSL_PREFIX %+ _ASN1_INTEGER_set_int64
%xdefine ASN1_INTEGER_set_uint64 BORINGSSL_PREFIX %+ _ASN1_INTEGER_set_uint64
%xdefine ASN1_INTEGER_to_BN BORINGSSL_PREFIX %+ _ASN1_INTEGER_to_BN
%xdefine ASN1_NULL_free BORINGSSL_PREFIX %+ _ASN1_NULL_free
%xdefine ASN1_NULL_it BORINGSSL_PREFIX %+ _ASN1_NULL_it
%xdefine ASN1_NULL_new BORINGSSL_PREFIX %+ _ASN1_NULL_new
%xdefine ASN1_OBJECT_create BORINGSSL_PREFIX %+ _ASN1_OBJECT_create
%xdefine ASN1_OBJECT_free BORINGSSL_PREFIX %+ _ASN1_OBJECT_free
%xdefine ASN1_OBJECT_it BORINGSSL_PREFIX %+ _ASN1_OBJECT_it
%xdefine ASN1_OBJECT_new BORINGSSL_PREFIX %+ _ASN1_OBJECT_new
%xdefine ASN1_OCTET_STRING_cmp BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_cmp
%xdefine ASN1_OCTET_STRING_dup BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_dup
%xdefine ASN1_OCTET_STRING_free BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_free
%xdefine ASN1_OCTET_STRING_it BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_it
%xdefine ASN1_OCTET_STRING_new BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_new
%xdefine ASN1_OCTET_STRING_set BORINGSSL_PREFIX %+ _ASN1_OCTET_STRING_set
%xdefine ASN1_PRINTABLESTRING_free BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_free
%xdefine ASN1_PRINTABLESTRING_it BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_it
%xdefine ASN1_PRINTABLESTRING_new BORINGSSL_PREFIX %+ _ASN1_PRINTABLESTRING_new
%xdefine ASN1_PRINTABLE_free BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_free
%xdefine ASN1_PRINTABLE_it BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_it
%xdefine ASN1_PRINTABLE_new BORINGSSL_PREFIX %+ _ASN1_PRINTABLE_new
%xdefine ASN1_SEQUENCE_ANY_it BORINGSSL_PREFIX %+ _ASN1_SEQUENCE_ANY_it
%xdefine ASN1_SEQUENCE_it BORINGSSL_PREFIX %+ _ASN1_SEQUENCE_it
%xdefine ASN1_SET_ANY_it BORINGSSL_PREFIX %+ _ASN1_SET_ANY_it
%xdefine ASN1_STRING_TABLE_add BORINGSSL_PREFIX %+ _ASN1_STRING_TABLE_add
%xdefine ASN1_STRING_TABLE_cleanup BORINGSSL_PREFIX %+ _ASN1_STRING_TABLE_cleanup
%xdefine ASN1_STRING_clear_free BORINGSSL_PREFIX %+ _ASN1_STRING_clear_free
%xdefine ASN1_STRING_cmp BORINGSSL_PREFIX %+ _ASN1_STRING_cmp
%xdefine ASN1_STRING_copy BORINGSSL_PREFIX %+ _ASN1_STRING_copy
%xdefine ASN1_STRING_data BORINGSSL_PREFIX %+ _ASN1_STRING_data
%xdefine ASN1_STRING_dup BORINGSSL_PREFIX %+ _ASN1_STRING_dup
%xdefine ASN1_STRING_free BORINGSSL_PREFIX %+ _ASN1_STRING_free
%xdefine ASN1_STRING_get0_data BORINGSSL_PREFIX %+ _ASN1_STRING_get0_data
%xdefine ASN1_STRING_get_default_mask BORINGSSL_PREFIX %+ _ASN1_STRING_get_default_mask
%xdefine ASN1_STRING_length BORINGSSL_PREFIX %+ _ASN1_STRING_length
%xdefine ASN1_STRING_new BORINGSSL_PREFIX %+ _ASN1_STRING_new
%xdefine ASN1_STRING_print BORINGSSL_PREFIX %+ _ASN1_STRING_print
%xdefine ASN1_STRING_print_ex BORINGSSL_PREFIX %+ _ASN1_STRING_print_ex
%xdefine ASN1_STRING_print_ex_fp BORINGSSL_PREFIX %+ _ASN1_STRING_print_ex_fp
%xdefine ASN1_STRING_set BORINGSSL_PREFIX %+ _ASN1_STRING_set
%xdefine ASN1_STRING_set0 BORINGSSL_PREFIX %+ _ASN1_STRING_set0
%xdefine ASN1_STRING_set_by_NID BORINGSSL_PREFIX %+ _ASN1_STRING_set_by_NID
%xdefine ASN1_STRING_set_default_mask BORINGSSL_PREFIX %+ _ASN1_STRING_set_default_mask
%xdefine ASN1_STRING_set_default_mask_asc BORINGSSL_PREFIX %+ _ASN1_STRING_set_default_mask_asc
%xdefine ASN1_STRING_to_UTF8 BORINGSSL_PREFIX %+ _ASN1_STRING_to_UTF8
%xdefine ASN1_STRING_type BORINGSSL_PREFIX %+ _ASN1_STRING_type
%xdefine ASN1_STRING_type_new BORINGSSL_PREFIX %+ _ASN1_STRING_type_new
%xdefine ASN1_T61STRING_free BORINGSSL_PREFIX %+ _ASN1_T61STRING_free
%xdefine ASN1_T61STRING_it BORINGSSL_PREFIX %+ _ASN1_T61STRING_it
%xdefine ASN1_T61STRING_new BORINGSSL_PREFIX %+ _ASN1_T61STRING_new
%xdefine ASN1_TBOOLEAN_it BORINGSSL_PREFIX %+ _ASN1_TBOOLEAN_it
%xdefine ASN1_TIME_adj BORINGSSL_PREFIX %+ _ASN1_TIME_adj
%xdefine ASN1_TIME_check BORINGSSL_PREFIX %+ _ASN1_TIME_check
%xdefine ASN1_TIME_diff BORINGSSL_PREFIX %+ _ASN1_TIME_diff
%xdefine ASN1_TIME_free BORINGSSL_PREFIX %+ _ASN1_TIME_free
%xdefine ASN1_TIME_it BORINGSSL_PREFIX %+ _ASN1_TIME_it
%xdefine ASN1_TIME_new BORINGSSL_PREFIX %+ _ASN1_TIME_new
%xdefine ASN1_TIME_print BORINGSSL_PREFIX %+ _ASN1_TIME_print
%xdefine ASN1_TIME_set BORINGSSL_PREFIX %+ _ASN1_TIME_set
%xdefine ASN1_TIME_set_posix BORINGSSL_PREFIX %+ _ASN1_TIME_set_posix
%xdefine ASN1_TIME_set_string BORINGSSL_PREFIX %+ _ASN1_TIME_set_string
%xdefine ASN1_TIME_set_string_X509 BORINGSSL_PREFIX %+ _ASN1_TIME_set_string_X509
%xdefine ASN1_TIME_to_generalizedtime BORINGSSL_PREFIX %+ _ASN1_TIME_to_generalizedtime
%xdefine ASN1_TIME_to_posix BORINGSSL_PREFIX %+ _ASN1_TIME_to_posix
%xdefine ASN1_TIME_to_time_t BORINGSSL_PREFIX %+ _ASN1_TIME_to_time_t
%xdefine ASN1_TIME_to_tm BORINGSSL_PREFIX %+ _ASN1_TIME_to_tm
%xdefine ASN1_TYPE_cmp BORINGSSL_PREFIX %+ _ASN1_TYPE_cmp
%xdefine ASN1_TYPE_free BORINGSSL_PREFIX %+ _ASN1_TYPE_free
%xdefine ASN1_TYPE_get BORINGSSL_PREFIX %+ _ASN1_TYPE_get
%xdefine ASN1_TYPE_new BORINGSSL_PREFIX %+ _ASN1_TYPE_new
%xdefine ASN1_TYPE_set BORINGSSL_PREFIX %+ _ASN1_TYPE_set
%xdefine ASN1_TYPE_set1 BORINGSSL_PREFIX %+ _ASN1_TYPE_set1
%xdefine ASN1_UNIVERSALSTRING_free BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_free
%xdefine ASN1_UNIVERSALSTRING_it BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_it
%xdefine ASN1_UNIVERSALSTRING_new BORINGSSL_PREFIX %+ _ASN1_UNIVERSALSTRING_new
%xdefine ASN1_UTCTIME_adj BORINGSSL_PREFIX %+ _ASN1_UTCTIME_adj
%xdefine ASN1_UTCTIME_check BORINGSSL_PREFIX %+ _ASN1_UTCTIME_check
%xdefine ASN1_UTCTIME_cmp_time_t BORINGSSL_PREFIX %+ _ASN1_UTCTIME_cmp_time_t
%xdefine ASN1_UTCTIME_free BORINGSSL_PREFIX %+ _ASN1_UTCTIME_free
%xdefine ASN1_UTCTIME_it BORINGSSL_PREFIX %+ _ASN1_UTCTIME_it
%xdefine ASN1_UTCTIME_new BORINGSSL_PREFIX %+ _ASN1_UTCTIME_new
%xdefine ASN1_UTCTIME_print BORINGSSL_PREFIX %+ _ASN1_UTCTIME_print
%xdefine ASN1_UTCTIME_set BORINGSSL_PREFIX %+ _ASN1_UTCTIME_set
%xdefine ASN1_UTCTIME_set_string BORINGSSL_PREFIX %+ _ASN1_UTCTIME_set_string
%xdefine ASN1_UTF8STRING_free BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_free
%xdefine ASN1_UTF8STRING_it BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_it
%xdefine ASN1_UTF8STRING_new BORINGSSL_PREFIX %+ _ASN1_UTF8STRING_new
%xdefine ASN1_VISIBLESTRING_free BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_free
%xdefine ASN1_VISIBLESTRING_it BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_it
%xdefine ASN1_VISIBLESTRING_new BORINGSSL_PREFIX %+ _ASN1_VISIBLESTRING_new
%xdefine ASN1_digest BORINGSSL_PREFIX %+ _ASN1_digest
%xdefine ASN1_dup BORINGSSL_PREFIX %+ _ASN1_dup
%xdefine ASN1_generate_v3 BORINGSSL_PREFIX %+ _ASN1_generate_v3
%xdefine ASN1_get_object BORINGSSL_PREFIX %+ _ASN1_get_object
%xdefine ASN1_i2d_bio BORINGSSL_PREFIX %+ _ASN1_i2d_bio
%xdefine ASN1_item_d2i BORINGSSL_PREFIX %+ _ASN1_item_d2i
%xdefine ASN1_item_d2i_bio BORINGSSL_PREFIX %+ _ASN1_item_d2i_bio
%xdefine ASN1_item_d2i_fp BORINGSSL_PREFIX %+ _ASN1_item_d2i_fp
%xdefine ASN1_item_digest BORINGSSL_PREFIX %+ _ASN1_item_digest
%xdefine ASN1_item_dup BORINGSSL_PREFIX %+ _ASN1_item_dup
%xdefine ASN1_item_ex_d2i BORINGSSL_PREFIX %+ _ASN1_item_ex_d2i
%xdefine ASN1_item_ex_free BORINGSSL_PREFIX %+ _ASN1_item_ex_free
%xdefine ASN1_item_ex_i2d BORINGSSL_PREFIX %+ _ASN1_item_ex_i2d
%xdefine ASN1_item_ex_new BORINGSSL_PREFIX %+ _ASN1_item_ex_new
%xdefine ASN1_item_free BORINGSSL_PREFIX %+ _ASN1_item_free
%xdefine ASN1_item_i2d BORINGSSL_PREFIX %+ _ASN1_item_i2d
%xdefine ASN1_item_i2d_bio BORINGSSL_PREFIX %+ _ASN1_item_i2d_bio
%xdefine ASN1_item_i2d_fp BORINGSSL_PREFIX %+ _ASN1_item_i2d_fp
%xdefine ASN1_item_new BORINGSSL_PREFIX %+ _ASN1_item_new
%xdefine ASN1_item_pack BORINGSSL_PREFIX %+ _ASN1_item_pack
%xdefine ASN1_item_sign BORINGSSL_PREFIX %+ _ASN1_item_sign
%xdefine ASN1_item_sign_ctx BORINGSSL_PREFIX %+ _ASN1_item_sign_ctx
%xdefine ASN1_item_unpack BORINGSSL_PREFIX %+ _ASN1_item_unpack
%xdefine ASN1_item_verify BORINGSSL_PREFIX %+ _ASN1_item_verify
%xdefine ASN1_mbstring_copy BORINGSSL_PREFIX %+ _ASN1_mbstring_copy
%xdefine ASN1_mbstring_ncopy BORINGSSL_PREFIX %+ _ASN1_mbstring_ncopy
%xdefine ASN1_object_size BORINGSSL_PREFIX %+ _ASN1_object_size
%xdefine ASN1_parse BORINGSSL_PREFIX %+ _ASN1_parse
%xdefine ASN1_primitive_free BORINGSSL_PREFIX %+ _ASN1_primitive_free
%xdefine ASN1_put_eoc BORINGSSL_PREFIX %+ _ASN1_put_eoc
%xdefine ASN1_put_object BORINGSSL_PREFIX %+ _ASN1_put_object
%xdefine ASN1_tag2bit BORINGSSL_PREFIX %+ _ASN1_tag2bit
%xdefine ASN1_tag2str BORINGSSL_PREFIX %+ _ASN1_tag2str
%xdefine ASN1_template_free BORINGSSL_PREFIX %+ _ASN1_template_free
%xdefine AUTHORITY_INFO_ACCESS_free BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_free
%xdefine AUTHORITY_INFO_ACCESS_it BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_it
%xdefine AUTHORITY_INFO_ACCESS_new BORINGSSL_PREFIX %+ _AUTHORITY_INFO_ACCESS_new
%xdefine AUTHORITY_KEYID_free BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_free
%xdefine AUTHORITY_KEYID_it BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_it
%xdefine AUTHORITY_KEYID_new BORINGSSL_PREFIX %+ _AUTHORITY_KEYID_new
%xdefine AWSLC_non_fips_pkey_evp_asn1_methods BORINGSSL_PREFIX %+ _AWSLC_non_fips_pkey_evp_asn1_methods
%xdefine AWSLC_non_fips_pkey_evp_methods BORINGSSL_PREFIX %+ _AWSLC_non_fips_pkey_evp_methods
%xdefine AWSLC_thread_local_clear BORINGSSL_PREFIX %+ _AWSLC_thread_local_clear
%xdefine AWSLC_thread_local_shutdown BORINGSSL_PREFIX %+ _AWSLC_thread_local_shutdown
%xdefine AWS_LC_FIPS_failure BORINGSSL_PREFIX %+ _AWS_LC_FIPS_failure
%xdefine BASIC_CONSTRAINTS_free BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_free
%xdefine BASIC_CONSTRAINTS_it BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_it
%xdefine BASIC_CONSTRAINTS_new BORINGSSL_PREFIX %+ _BASIC_CONSTRAINTS_new
%xdefine BF_cbc_encrypt BORINGSSL_PREFIX %+ _BF_cbc_encrypt
%xdefine BF_cfb64_encrypt BORINGSSL_PREFIX %+ _BF_cfb64_encrypt
%xdefine BF_decrypt BORINGSSL_PREFIX %+ _BF_decrypt
%xdefine BF_ecb_encrypt BORINGSSL_PREFIX %+ _BF_ecb_encrypt
%xdefine BF_encrypt BORINGSSL_PREFIX %+ _BF_encrypt
%xdefine BF_ofb64_encrypt BORINGSSL_PREFIX %+ _BF_ofb64_encrypt
%xdefine BF_set_key BORINGSSL_PREFIX %+ _BF_set_key
%xdefine BIO_ADDR_clear BORINGSSL_PREFIX %+ _BIO_ADDR_clear
%xdefine BIO_ADDR_copy BORINGSSL_PREFIX %+ _BIO_ADDR_copy
%xdefine BIO_ADDR_dup BORINGSSL_PREFIX %+ _BIO_ADDR_dup
%xdefine BIO_ADDR_family BORINGSSL_PREFIX %+ _BIO_ADDR_family
%xdefine BIO_ADDR_free BORINGSSL_PREFIX %+ _BIO_ADDR_free
%xdefine BIO_ADDR_new BORINGSSL_PREFIX %+ _BIO_ADDR_new
%xdefine BIO_ADDR_rawaddress BORINGSSL_PREFIX %+ _BIO_ADDR_rawaddress
%xdefine BIO_ADDR_rawmake BORINGSSL_PREFIX %+ _BIO_ADDR_rawmake
%xdefine BIO_ADDR_rawport BORINGSSL_PREFIX %+ _BIO_ADDR_rawport
%xdefine BIO_append_filename BORINGSSL_PREFIX %+ _BIO_append_filename
%xdefine BIO_callback_ctrl BORINGSSL_PREFIX %+ _BIO_callback_ctrl
%xdefine BIO_clear_flags BORINGSSL_PREFIX %+ _BIO_clear_flags
%xdefine BIO_clear_retry_flags BORINGSSL_PREFIX %+ _BIO_clear_retry_flags
%xdefine BIO_copy_next_retry BORINGSSL_PREFIX %+ _BIO_copy_next_retry
%xdefine BIO_ctrl BORINGSSL_PREFIX %+ _BIO_ctrl
%xdefine BIO_ctrl_dgram_connect BORINGSSL_PREFIX %+ _BIO_ctrl_dgram_connect
%xdefine BIO_ctrl_get_read_request BORINGSSL_PREFIX %+ _BIO_ctrl_get_read_request
%xdefine BIO_ctrl_get_write_guarantee BORINGSSL_PREFIX %+ _BIO_ctrl_get_write_guarantee
%xdefine BIO_ctrl_pending BORINGSSL_PREFIX %+ _BIO_ctrl_pending
%xdefine BIO_ctrl_set_connected BORINGSSL_PREFIX %+ _BIO_ctrl_set_connected
%xdefine BIO_destroy_bio_pair BORINGSSL_PREFIX %+ _BIO_destroy_bio_pair
%xdefine BIO_dgram_get_peer BORINGSSL_PREFIX %+ _BIO_dgram_get_peer
%xdefine BIO_dgram_recv_timedout BORINGSSL_PREFIX %+ _BIO_dgram_recv_timedout
%xdefine BIO_dgram_send_timedout BORINGSSL_PREFIX %+ _BIO_dgram_send_timedout
%xdefine BIO_dgram_set_peer BORINGSSL_PREFIX %+ _BIO_dgram_set_peer
%xdefine BIO_do_connect BORINGSSL_PREFIX %+ _BIO_do_connect
%xdefine BIO_dump BORINGSSL_PREFIX %+ _BIO_dump
%xdefine BIO_eof BORINGSSL_PREFIX %+ _BIO_eof
%xdefine BIO_f_base64 BORINGSSL_PREFIX %+ _BIO_f_base64
%xdefine BIO_f_cipher BORINGSSL_PREFIX %+ _BIO_f_cipher
%xdefine BIO_f_md BORINGSSL_PREFIX %+ _BIO_f_md
%xdefine BIO_find_type BORINGSSL_PREFIX %+ _BIO_find_type
%xdefine BIO_flush BORINGSSL_PREFIX %+ _BIO_flush
%xdefine BIO_free BORINGSSL_PREFIX %+ _BIO_free
%xdefine BIO_free_all BORINGSSL_PREFIX %+ _BIO_free_all
%xdefine BIO_get_callback_arg BORINGSSL_PREFIX %+ _BIO_get_callback_arg
%xdefine BIO_get_cipher_ctx BORINGSSL_PREFIX %+ _BIO_get_cipher_ctx
%xdefine BIO_get_cipher_status BORINGSSL_PREFIX %+ _BIO_get_cipher_status
%xdefine BIO_get_close BORINGSSL_PREFIX %+ _BIO_get_close
%xdefine BIO_get_data BORINGSSL_PREFIX %+ _BIO_get_data
%xdefine BIO_get_ex_data BORINGSSL_PREFIX %+ _BIO_get_ex_data
%xdefine BIO_get_ex_new_index BORINGSSL_PREFIX %+ _BIO_get_ex_new_index
%xdefine BIO_get_fd BORINGSSL_PREFIX %+ _BIO_get_fd
%xdefine BIO_get_fp BORINGSSL_PREFIX %+ _BIO_get_fp
%xdefine BIO_get_init BORINGSSL_PREFIX %+ _BIO_get_init
%xdefine BIO_get_md BORINGSSL_PREFIX %+ _BIO_get_md
%xdefine BIO_get_md_ctx BORINGSSL_PREFIX %+ _BIO_get_md_ctx
%xdefine BIO_get_mem_ptr BORINGSSL_PREFIX %+ _BIO_get_mem_ptr
%xdefine BIO_get_new_index BORINGSSL_PREFIX %+ _BIO_get_new_index
%xdefine BIO_get_retry_flags BORINGSSL_PREFIX %+ _BIO_get_retry_flags
%xdefine BIO_get_retry_reason BORINGSSL_PREFIX %+ _BIO_get_retry_reason
%xdefine BIO_get_shutdown BORINGSSL_PREFIX %+ _BIO_get_shutdown
%xdefine BIO_gets BORINGSSL_PREFIX %+ _BIO_gets
%xdefine BIO_hexdump BORINGSSL_PREFIX %+ _BIO_hexdump
%xdefine BIO_indent BORINGSSL_PREFIX %+ _BIO_indent
%xdefine BIO_int_ctrl BORINGSSL_PREFIX %+ _BIO_int_ctrl
%xdefine BIO_mem_contents BORINGSSL_PREFIX %+ _BIO_mem_contents
%xdefine BIO_meth_free BORINGSSL_PREFIX %+ _BIO_meth_free
%xdefine BIO_meth_get_callback_ctrl BORINGSSL_PREFIX %+ _BIO_meth_get_callback_ctrl
%xdefine BIO_meth_get_create BORINGSSL_PREFIX %+ _BIO_meth_get_create
%xdefine BIO_meth_get_ctrl BORINGSSL_PREFIX %+ _BIO_meth_get_ctrl
%xdefine BIO_meth_get_destroy BORINGSSL_PREFIX %+ _BIO_meth_get_destroy
%xdefine BIO_meth_get_gets BORINGSSL_PREFIX %+ _BIO_meth_get_gets
%xdefine BIO_meth_get_puts BORINGSSL_PREFIX %+ _BIO_meth_get_puts
%xdefine BIO_meth_new BORINGSSL_PREFIX %+ _BIO_meth_new
%xdefine BIO_meth_set_callback_ctrl BORINGSSL_PREFIX %+ _BIO_meth_set_callback_ctrl
%xdefine BIO_meth_set_create BORINGSSL_PREFIX %+ _BIO_meth_set_create
%xdefine BIO_meth_set_ctrl BORINGSSL_PREFIX %+ _BIO_meth_set_ctrl
%xdefine BIO_meth_set_destroy BORINGSSL_PREFIX %+ _BIO_meth_set_destroy
%xdefine BIO_meth_set_gets BORINGSSL_PREFIX %+ _BIO_meth_set_gets
%xdefine BIO_meth_set_puts BORINGSSL_PREFIX %+ _BIO_meth_set_puts
%xdefine BIO_meth_set_read BORINGSSL_PREFIX %+ _BIO_meth_set_read
%xdefine BIO_meth_set_write BORINGSSL_PREFIX %+ _BIO_meth_set_write
%xdefine BIO_method_name BORINGSSL_PREFIX %+ _BIO_method_name
%xdefine BIO_method_type BORINGSSL_PREFIX %+ _BIO_method_type
%xdefine BIO_new BORINGSSL_PREFIX %+ _BIO_new
%xdefine BIO_new_bio_pair BORINGSSL_PREFIX %+ _BIO_new_bio_pair
%xdefine BIO_new_connect BORINGSSL_PREFIX %+ _BIO_new_connect
%xdefine BIO_new_dgram BORINGSSL_PREFIX %+ _BIO_new_dgram
%xdefine BIO_new_fd BORINGSSL_PREFIX %+ _BIO_new_fd
%xdefine BIO_new_file BORINGSSL_PREFIX %+ _BIO_new_file
%xdefine BIO_new_fp BORINGSSL_PREFIX %+ _BIO_new_fp
%xdefine BIO_new_mem_buf BORINGSSL_PREFIX %+ _BIO_new_mem_buf
%xdefine BIO_new_socket BORINGSSL_PREFIX %+ _BIO_new_socket
%xdefine BIO_next BORINGSSL_PREFIX %+ _BIO_next
%xdefine BIO_number_read BORINGSSL_PREFIX %+ _BIO_number_read
%xdefine BIO_number_written BORINGSSL_PREFIX %+ _BIO_number_written
%xdefine BIO_pending BORINGSSL_PREFIX %+ _BIO_pending
%xdefine BIO_pop BORINGSSL_PREFIX %+ _BIO_pop
%xdefine BIO_printf BORINGSSL_PREFIX %+ _BIO_printf
%xdefine BIO_ptr_ctrl BORINGSSL_PREFIX %+ _BIO_ptr_ctrl
%xdefine BIO_push BORINGSSL_PREFIX %+ _BIO_push
%xdefine BIO_puts BORINGSSL_PREFIX %+ _BIO_puts
%xdefine BIO_read BORINGSSL_PREFIX %+ _BIO_read
%xdefine BIO_read_asn1 BORINGSSL_PREFIX %+ _BIO_read_asn1
%xdefine BIO_read_ex BORINGSSL_PREFIX %+ _BIO_read_ex
%xdefine BIO_read_filename BORINGSSL_PREFIX %+ _BIO_read_filename
%xdefine BIO_reset BORINGSSL_PREFIX %+ _BIO_reset
%xdefine BIO_rw_filename BORINGSSL_PREFIX %+ _BIO_rw_filename
%xdefine BIO_s_connect BORINGSSL_PREFIX %+ _BIO_s_connect
%xdefine BIO_s_datagram BORINGSSL_PREFIX %+ _BIO_s_datagram
%xdefine BIO_s_fd BORINGSSL_PREFIX %+ _BIO_s_fd
%xdefine BIO_s_file BORINGSSL_PREFIX %+ _BIO_s_file
%xdefine BIO_s_mem BORINGSSL_PREFIX %+ _BIO_s_mem
%xdefine BIO_s_secmem BORINGSSL_PREFIX %+ _BIO_s_secmem
%xdefine BIO_s_socket BORINGSSL_PREFIX %+ _BIO_s_socket
%xdefine BIO_seek BORINGSSL_PREFIX %+ _BIO_seek
%xdefine BIO_set_callback BORINGSSL_PREFIX %+ _BIO_set_callback
%xdefine BIO_set_callback_arg BORINGSSL_PREFIX %+ _BIO_set_callback_arg
%xdefine BIO_set_callback_ex BORINGSSL_PREFIX %+ _BIO_set_callback_ex
%xdefine BIO_set_cipher BORINGSSL_PREFIX %+ _BIO_set_cipher
%xdefine BIO_set_close BORINGSSL_PREFIX %+ _BIO_set_close
%xdefine BIO_set_conn_hostname BORINGSSL_PREFIX %+ _BIO_set_conn_hostname
%xdefine BIO_set_conn_int_port BORINGSSL_PREFIX %+ _BIO_set_conn_int_port
%xdefine BIO_set_conn_port BORINGSSL_PREFIX %+ _BIO_set_conn_port
%xdefine BIO_set_data BORINGSSL_PREFIX %+ _BIO_set_data
%xdefine BIO_set_ex_data BORINGSSL_PREFIX %+ _BIO_set_ex_data
%xdefine BIO_set_fd BORINGSSL_PREFIX %+ _BIO_set_fd
%xdefine BIO_set_flags BORINGSSL_PREFIX %+ _BIO_set_flags
%xdefine BIO_set_fp BORINGSSL_PREFIX %+ _BIO_set_fp
%xdefine BIO_set_init BORINGSSL_PREFIX %+ _BIO_set_init
%xdefine BIO_set_md BORINGSSL_PREFIX %+ _BIO_set_md
%xdefine BIO_set_mem_buf BORINGSSL_PREFIX %+ _BIO_set_mem_buf
%xdefine BIO_set_mem_eof_return BORINGSSL_PREFIX %+ _BIO_set_mem_eof_return
%xdefine BIO_set_nbio BORINGSSL_PREFIX %+ _BIO_set_nbio
%xdefine BIO_set_retry_read BORINGSSL_PREFIX %+ _BIO_set_retry_read
%xdefine BIO_set_retry_reason BORINGSSL_PREFIX %+ _BIO_set_retry_reason
%xdefine BIO_set_retry_special BORINGSSL_PREFIX %+ _BIO_set_retry_special
%xdefine BIO_set_retry_write BORINGSSL_PREFIX %+ _BIO_set_retry_write
%xdefine BIO_set_shutdown BORINGSSL_PREFIX %+ _BIO_set_shutdown
%xdefine BIO_set_write_buffer_size BORINGSSL_PREFIX %+ _BIO_set_write_buffer_size
%xdefine BIO_should_io_special BORINGSSL_PREFIX %+ _BIO_should_io_special
%xdefine BIO_should_read BORINGSSL_PREFIX %+ _BIO_should_read
%xdefine BIO_should_retry BORINGSSL_PREFIX %+ _BIO_should_retry
%xdefine BIO_should_write BORINGSSL_PREFIX %+ _BIO_should_write
%xdefine BIO_shutdown_wr BORINGSSL_PREFIX %+ _BIO_shutdown_wr
%xdefine BIO_snprintf BORINGSSL_PREFIX %+ _BIO_snprintf
%xdefine BIO_tell BORINGSSL_PREFIX %+ _BIO_tell
%xdefine BIO_test_flags BORINGSSL_PREFIX %+ _BIO_test_flags
%xdefine BIO_up_ref BORINGSSL_PREFIX %+ _BIO_up_ref
%xdefine BIO_vfree BORINGSSL_PREFIX %+ _BIO_vfree
%xdefine BIO_vsnprintf BORINGSSL_PREFIX %+ _BIO_vsnprintf
%xdefine BIO_wpending BORINGSSL_PREFIX %+ _BIO_wpending
%xdefine BIO_write BORINGSSL_PREFIX %+ _BIO_write
%xdefine BIO_write_all BORINGSSL_PREFIX %+ _BIO_write_all
%xdefine BIO_write_ex BORINGSSL_PREFIX %+ _BIO_write_ex
%xdefine BIO_write_filename BORINGSSL_PREFIX %+ _BIO_write_filename
%xdefine BLAKE2B256 BORINGSSL_PREFIX %+ _BLAKE2B256
%xdefine BLAKE2B256_Final BORINGSSL_PREFIX %+ _BLAKE2B256_Final
%xdefine BLAKE2B256_Init BORINGSSL_PREFIX %+ _BLAKE2B256_Init
%xdefine BLAKE2B256_Update BORINGSSL_PREFIX %+ _BLAKE2B256_Update
%xdefine BN_BLINDING_convert BORINGSSL_PREFIX %+ _BN_BLINDING_convert
%xdefine BN_BLINDING_free BORINGSSL_PREFIX %+ _BN_BLINDING_free
%xdefine BN_BLINDING_invalidate BORINGSSL_PREFIX %+ _BN_BLINDING_invalidate
%xdefine BN_BLINDING_invert BORINGSSL_PREFIX %+ _BN_BLINDING_invert
%xdefine BN_BLINDING_new BORINGSSL_PREFIX %+ _BN_BLINDING_new
%xdefine BN_CTX_end BORINGSSL_PREFIX %+ _BN_CTX_end
%xdefine BN_CTX_free BORINGSSL_PREFIX %+ _BN_CTX_free
%xdefine BN_CTX_get BORINGSSL_PREFIX %+ _BN_CTX_get
%xdefine BN_CTX_new BORINGSSL_PREFIX %+ _BN_CTX_new
%xdefine BN_CTX_secure_new BORINGSSL_PREFIX %+ _BN_CTX_secure_new
%xdefine BN_CTX_start BORINGSSL_PREFIX %+ _BN_CTX_start
%xdefine BN_GENCB_call BORINGSSL_PREFIX %+ _BN_GENCB_call
%xdefine BN_GENCB_free BORINGSSL_PREFIX %+ _BN_GENCB_free
%xdefine BN_GENCB_get_arg BORINGSSL_PREFIX %+ _BN_GENCB_get_arg
%xdefine BN_GENCB_new BORINGSSL_PREFIX %+ _BN_GENCB_new
%xdefine BN_GENCB_set BORINGSSL_PREFIX %+ _BN_GENCB_set
%xdefine BN_GENCB_set_old BORINGSSL_PREFIX %+ _BN_GENCB_set_old
%xdefine BN_MONT_CTX_copy BORINGSSL_PREFIX %+ _BN_MONT_CTX_copy
%xdefine BN_MONT_CTX_free BORINGSSL_PREFIX %+ _BN_MONT_CTX_free
%xdefine BN_MONT_CTX_new BORINGSSL_PREFIX %+ _BN_MONT_CTX_new
%xdefine BN_MONT_CTX_new_consttime BORINGSSL_PREFIX %+ _BN_MONT_CTX_new_consttime
%xdefine BN_MONT_CTX_new_for_modulus BORINGSSL_PREFIX %+ _BN_MONT_CTX_new_for_modulus
%xdefine BN_MONT_CTX_set BORINGSSL_PREFIX %+ _BN_MONT_CTX_set
%xdefine BN_MONT_CTX_set_locked BORINGSSL_PREFIX %+ _BN_MONT_CTX_set_locked
%xdefine BN_abs_is_word BORINGSSL_PREFIX %+ _BN_abs_is_word
%xdefine BN_add BORINGSSL_PREFIX %+ _BN_add
%xdefine BN_add_word BORINGSSL_PREFIX %+ _BN_add_word
%xdefine BN_asc2bn BORINGSSL_PREFIX %+ _BN_asc2bn
%xdefine BN_bin2bn BORINGSSL_PREFIX %+ _BN_bin2bn
%xdefine BN_bn2bin BORINGSSL_PREFIX %+ _BN_bn2bin
%xdefine BN_bn2bin_padded BORINGSSL_PREFIX %+ _BN_bn2bin_padded
%xdefine BN_bn2binpad BORINGSSL_PREFIX %+ _BN_bn2binpad
%xdefine BN_bn2cbb_padded BORINGSSL_PREFIX %+ _BN_bn2cbb_padded
%xdefine BN_bn2dec BORINGSSL_PREFIX %+ _BN_bn2dec
%xdefine BN_bn2hex BORINGSSL_PREFIX %+ _BN_bn2hex
%xdefine BN_bn2le_padded BORINGSSL_PREFIX %+ _BN_bn2le_padded
%xdefine BN_bn2mpi BORINGSSL_PREFIX %+ _BN_bn2mpi
%xdefine BN_clear BORINGSSL_PREFIX %+ _BN_clear
%xdefine BN_clear_bit BORINGSSL_PREFIX %+ _BN_clear_bit
%xdefine BN_clear_free BORINGSSL_PREFIX %+ _BN_clear_free
%xdefine BN_cmp BORINGSSL_PREFIX %+ _BN_cmp
%xdefine BN_cmp_word BORINGSSL_PREFIX %+ _BN_cmp_word
%xdefine BN_copy BORINGSSL_PREFIX %+ _BN_copy
%xdefine BN_count_low_zero_bits BORINGSSL_PREFIX %+ _BN_count_low_zero_bits
%xdefine BN_dec2bn BORINGSSL_PREFIX %+ _BN_dec2bn
%xdefine BN_div BORINGSSL_PREFIX %+ _BN_div
%xdefine BN_div_word BORINGSSL_PREFIX %+ _BN_div_word
%xdefine BN_dup BORINGSSL_PREFIX %+ _BN_dup
%xdefine BN_enhanced_miller_rabin_primality_test BORINGSSL_PREFIX %+ _BN_enhanced_miller_rabin_primality_test
%xdefine BN_equal_consttime BORINGSSL_PREFIX %+ _BN_equal_consttime
%xdefine BN_exp BORINGSSL_PREFIX %+ _BN_exp
%xdefine BN_free BORINGSSL_PREFIX %+ _BN_free
%xdefine BN_from_montgomery BORINGSSL_PREFIX %+ _BN_from_montgomery
%xdefine BN_gcd BORINGSSL_PREFIX %+ _BN_gcd
%xdefine BN_generate_prime_ex BORINGSSL_PREFIX %+ _BN_generate_prime_ex
%xdefine BN_get_flags BORINGSSL_PREFIX %+ _BN_get_flags
%xdefine BN_get_minimal_width BORINGSSL_PREFIX %+ _BN_get_minimal_width
%xdefine BN_get_rfc3526_prime_1536 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_1536
%xdefine BN_get_rfc3526_prime_2048 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_2048
%xdefine BN_get_rfc3526_prime_3072 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_3072
%xdefine BN_get_rfc3526_prime_4096 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_4096
%xdefine BN_get_rfc3526_prime_6144 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_6144
%xdefine BN_get_rfc3526_prime_8192 BORINGSSL_PREFIX %+ _BN_get_rfc3526_prime_8192
%xdefine BN_get_u64 BORINGSSL_PREFIX %+ _BN_get_u64
%xdefine BN_get_word BORINGSSL_PREFIX %+ _BN_get_word
%xdefine BN_hex2bn BORINGSSL_PREFIX %+ _BN_hex2bn
%xdefine BN_init BORINGSSL_PREFIX %+ _BN_init
%xdefine BN_is_bit_set BORINGSSL_PREFIX %+ _BN_is_bit_set
%xdefine BN_is_negative BORINGSSL_PREFIX %+ _BN_is_negative
%xdefine BN_is_odd BORINGSSL_PREFIX %+ _BN_is_odd
%xdefine BN_is_one BORINGSSL_PREFIX %+ _BN_is_one
%xdefine BN_is_pow2 BORINGSSL_PREFIX %+ _BN_is_pow2
%xdefine BN_is_prime_ex BORINGSSL_PREFIX %+ _BN_is_prime_ex
%xdefine BN_is_prime_fasttest_ex BORINGSSL_PREFIX %+ _BN_is_prime_fasttest_ex
%xdefine BN_is_word BORINGSSL_PREFIX %+ _BN_is_word
%xdefine BN_is_zero BORINGSSL_PREFIX %+ _BN_is_zero
%xdefine BN_le2bn BORINGSSL_PREFIX %+ _BN_le2bn
%xdefine BN_lshift BORINGSSL_PREFIX %+ _BN_lshift
%xdefine BN_lshift1 BORINGSSL_PREFIX %+ _BN_lshift1
%xdefine BN_marshal_asn1 BORINGSSL_PREFIX %+ _BN_marshal_asn1
%xdefine BN_mask_bits BORINGSSL_PREFIX %+ _BN_mask_bits
%xdefine BN_mod_add BORINGSSL_PREFIX %+ _BN_mod_add
%xdefine BN_mod_add_quick BORINGSSL_PREFIX %+ _BN_mod_add_quick
%xdefine BN_mod_exp BORINGSSL_PREFIX %+ _BN_mod_exp
%xdefine BN_mod_exp2_mont BORINGSSL_PREFIX %+ _BN_mod_exp2_mont
%xdefine BN_mod_exp_mont BORINGSSL_PREFIX %+ _BN_mod_exp_mont
%xdefine BN_mod_exp_mont_consttime BORINGSSL_PREFIX %+ _BN_mod_exp_mont_consttime
%xdefine BN_mod_exp_mont_consttime_x2 BORINGSSL_PREFIX %+ _BN_mod_exp_mont_consttime_x2
%xdefine BN_mod_exp_mont_word BORINGSSL_PREFIX %+ _BN_mod_exp_mont_word
%xdefine BN_mod_inverse BORINGSSL_PREFIX %+ _BN_mod_inverse
%xdefine BN_mod_inverse_blinded BORINGSSL_PREFIX %+ _BN_mod_inverse_blinded
%xdefine BN_mod_inverse_odd BORINGSSL_PREFIX %+ _BN_mod_inverse_odd
%xdefine BN_mod_lshift BORINGSSL_PREFIX %+ _BN_mod_lshift
%xdefine BN_mod_lshift1 BORINGSSL_PREFIX %+ _BN_mod_lshift1
%xdefine BN_mod_lshift1_quick BORINGSSL_PREFIX %+ _BN_mod_lshift1_quick
%xdefine BN_mod_lshift_quick BORINGSSL_PREFIX %+ _BN_mod_lshift_quick
%xdefine BN_mod_mul BORINGSSL_PREFIX %+ _BN_mod_mul
%xdefine BN_mod_mul_montgomery BORINGSSL_PREFIX %+ _BN_mod_mul_montgomery
%xdefine BN_mod_pow2 BORINGSSL_PREFIX %+ _BN_mod_pow2
%xdefine BN_mod_sqr BORINGSSL_PREFIX %+ _BN_mod_sqr
%xdefine BN_mod_sqrt BORINGSSL_PREFIX %+ _BN_mod_sqrt
%xdefine BN_mod_sub BORINGSSL_PREFIX %+ _BN_mod_sub
%xdefine BN_mod_sub_quick BORINGSSL_PREFIX %+ _BN_mod_sub_quick
%xdefine BN_mod_word BORINGSSL_PREFIX %+ _BN_mod_word
%xdefine BN_mpi2bn BORINGSSL_PREFIX %+ _BN_mpi2bn
%xdefine BN_mul BORINGSSL_PREFIX %+ _BN_mul
%xdefine BN_mul_word BORINGSSL_PREFIX %+ _BN_mul_word
%xdefine BN_new BORINGSSL_PREFIX %+ _BN_new
%xdefine BN_nnmod BORINGSSL_PREFIX %+ _BN_nnmod
%xdefine BN_nnmod_pow2 BORINGSSL_PREFIX %+ _BN_nnmod_pow2
%xdefine BN_num_bits BORINGSSL_PREFIX %+ _BN_num_bits
%xdefine BN_num_bits_word BORINGSSL_PREFIX %+ _BN_num_bits_word
%xdefine BN_num_bytes BORINGSSL_PREFIX %+ _BN_num_bytes
%xdefine BN_one BORINGSSL_PREFIX %+ _BN_one
%xdefine BN_parse_asn1_unsigned BORINGSSL_PREFIX %+ _BN_parse_asn1_unsigned
%xdefine BN_primality_test BORINGSSL_PREFIX %+ _BN_primality_test
%xdefine BN_print BORINGSSL_PREFIX %+ _BN_print
%xdefine BN_print_fp BORINGSSL_PREFIX %+ _BN_print_fp
%xdefine BN_pseudo_rand BORINGSSL_PREFIX %+ _BN_pseudo_rand
%xdefine BN_pseudo_rand_range BORINGSSL_PREFIX %+ _BN_pseudo_rand_range
%xdefine BN_rand BORINGSSL_PREFIX %+ _BN_rand
%xdefine BN_rand_range BORINGSSL_PREFIX %+ _BN_rand_range
%xdefine BN_rand_range_ex BORINGSSL_PREFIX %+ _BN_rand_range_ex
%xdefine BN_rshift BORINGSSL_PREFIX %+ _BN_rshift
%xdefine BN_rshift1 BORINGSSL_PREFIX %+ _BN_rshift1
%xdefine BN_secure_new BORINGSSL_PREFIX %+ _BN_secure_new
%xdefine BN_set_bit BORINGSSL_PREFIX %+ _BN_set_bit
%xdefine BN_set_flags BORINGSSL_PREFIX %+ _BN_set_flags
%xdefine BN_set_negative BORINGSSL_PREFIX %+ _BN_set_negative
%xdefine BN_set_u64 BORINGSSL_PREFIX %+ _BN_set_u64
%xdefine BN_set_word BORINGSSL_PREFIX %+ _BN_set_word
%xdefine BN_sqr BORINGSSL_PREFIX %+ _BN_sqr
%xdefine BN_sqrt BORINGSSL_PREFIX %+ _BN_sqrt
%xdefine BN_sub BORINGSSL_PREFIX %+ _BN_sub
%xdefine BN_sub_word BORINGSSL_PREFIX %+ _BN_sub_word
%xdefine BN_to_ASN1_ENUMERATED BORINGSSL_PREFIX %+ _BN_to_ASN1_ENUMERATED
%xdefine BN_to_ASN1_INTEGER BORINGSSL_PREFIX %+ _BN_to_ASN1_INTEGER
%xdefine BN_to_montgomery BORINGSSL_PREFIX %+ _BN_to_montgomery
%xdefine BN_uadd BORINGSSL_PREFIX %+ _BN_uadd
%xdefine BN_ucmp BORINGSSL_PREFIX %+ _BN_ucmp
%xdefine BN_usub BORINGSSL_PREFIX %+ _BN_usub
%xdefine BN_value_one BORINGSSL_PREFIX %+ _BN_value_one
%xdefine BN_zero BORINGSSL_PREFIX %+ _BN_zero
%xdefine BORINGSSL_function_hit BORINGSSL_PREFIX %+ _BORINGSSL_function_hit
%xdefine BORINGSSL_self_test BORINGSSL_PREFIX %+ _BORINGSSL_self_test
%xdefine BUF_MEM_append BORINGSSL_PREFIX %+ _BUF_MEM_append
%xdefine BUF_MEM_free BORINGSSL_PREFIX %+ _BUF_MEM_free
%xdefine BUF_MEM_grow BORINGSSL_PREFIX %+ _BUF_MEM_grow
%xdefine BUF_MEM_grow_clean BORINGSSL_PREFIX %+ _BUF_MEM_grow_clean
%xdefine BUF_MEM_new BORINGSSL_PREFIX %+ _BUF_MEM_new
%xdefine BUF_MEM_reserve BORINGSSL_PREFIX %+ _BUF_MEM_reserve
%xdefine BUF_memdup BORINGSSL_PREFIX %+ _BUF_memdup
%xdefine BUF_strdup BORINGSSL_PREFIX %+ _BUF_strdup
%xdefine BUF_strlcat BORINGSSL_PREFIX %+ _BUF_strlcat
%xdefine BUF_strlcpy BORINGSSL_PREFIX %+ _BUF_strlcpy
%xdefine BUF_strndup BORINGSSL_PREFIX %+ _BUF_strndup
%xdefine BUF_strnlen BORINGSSL_PREFIX %+ _BUF_strnlen
%xdefine CAST_S_table0 BORINGSSL_PREFIX %+ _CAST_S_table0
%xdefine CAST_S_table1 BORINGSSL_PREFIX %+ _CAST_S_table1
%xdefine CAST_S_table2 BORINGSSL_PREFIX %+ _CAST_S_table2
%xdefine CAST_S_table3 BORINGSSL_PREFIX %+ _CAST_S_table3
%xdefine CAST_S_table4 BORINGSSL_PREFIX %+ _CAST_S_table4
%xdefine CAST_S_table5 BORINGSSL_PREFIX %+ _CAST_S_table5
%xdefine CAST_S_table6 BORINGSSL_PREFIX %+ _CAST_S_table6
%xdefine CAST_S_table7 BORINGSSL_PREFIX %+ _CAST_S_table7
%xdefine CAST_cbc_encrypt BORINGSSL_PREFIX %+ _CAST_cbc_encrypt
%xdefine CAST_decrypt BORINGSSL_PREFIX %+ _CAST_decrypt
%xdefine CAST_ecb_encrypt BORINGSSL_PREFIX %+ _CAST_ecb_encrypt
%xdefine CAST_encrypt BORINGSSL_PREFIX %+ _CAST_encrypt
%xdefine CAST_set_key BORINGSSL_PREFIX %+ _CAST_set_key
%xdefine CBB_add_asn1 BORINGSSL_PREFIX %+ _CBB_add_asn1
%xdefine CBB_add_asn1_bool BORINGSSL_PREFIX %+ _CBB_add_asn1_bool
%xdefine CBB_add_asn1_int64 BORINGSSL_PREFIX %+ _CBB_add_asn1_int64
%xdefine CBB_add_asn1_int64_with_tag BORINGSSL_PREFIX %+ _CBB_add_asn1_int64_with_tag
%xdefine CBB_add_asn1_octet_string BORINGSSL_PREFIX %+ _CBB_add_asn1_octet_string
%xdefine CBB_add_asn1_oid_from_text BORINGSSL_PREFIX %+ _CBB_add_asn1_oid_from_text
%xdefine CBB_add_asn1_uint64 BORINGSSL_PREFIX %+ _CBB_add_asn1_uint64
%xdefine CBB_add_asn1_uint64_with_tag BORINGSSL_PREFIX %+ _CBB_add_asn1_uint64_with_tag
%xdefine CBB_add_bytes BORINGSSL_PREFIX %+ _CBB_add_bytes
%xdefine CBB_add_space BORINGSSL_PREFIX %+ _CBB_add_space
%xdefine CBB_add_u16 BORINGSSL_PREFIX %+ _CBB_add_u16
%xdefine CBB_add_u16_length_prefixed BORINGSSL_PREFIX %+ _CBB_add_u16_length_prefixed
%xdefine CBB_add_u16le BORINGSSL_PREFIX %+ _CBB_add_u16le
%xdefine CBB_add_u24 BORINGSSL_PREFIX %+ _CBB_add_u24
%xdefine CBB_add_u24_length_prefixed BORINGSSL_PREFIX %+ _CBB_add_u24_length_prefixed
%xdefine CBB_add_u32 BORINGSSL_PREFIX %+ _CBB_add_u32
%xdefine CBB_add_u32le BORINGSSL_PREFIX %+ _CBB_add_u32le
%xdefine CBB_add_u64 BORINGSSL_PREFIX %+ _CBB_add_u64
%xdefine CBB_add_u64le BORINGSSL_PREFIX %+ _CBB_add_u64le
%xdefine CBB_add_u8 BORINGSSL_PREFIX %+ _CBB_add_u8
%xdefine CBB_add_u8_length_prefixed BORINGSSL_PREFIX %+ _CBB_add_u8_length_prefixed
%xdefine CBB_add_zeros BORINGSSL_PREFIX %+ _CBB_add_zeros
%xdefine CBB_cleanup BORINGSSL_PREFIX %+ _CBB_cleanup
%xdefine CBB_data BORINGSSL_PREFIX %+ _CBB_data
%xdefine CBB_did_write BORINGSSL_PREFIX %+ _CBB_did_write
%xdefine CBB_discard_child BORINGSSL_PREFIX %+ _CBB_discard_child
%xdefine CBB_finish BORINGSSL_PREFIX %+ _CBB_finish
%xdefine CBB_finish_i2d BORINGSSL_PREFIX %+ _CBB_finish_i2d
%xdefine CBB_flush BORINGSSL_PREFIX %+ _CBB_flush
%xdefine CBB_flush_asn1_set_of BORINGSSL_PREFIX %+ _CBB_flush_asn1_set_of
%xdefine CBB_init BORINGSSL_PREFIX %+ _CBB_init
%xdefine CBB_init_fixed BORINGSSL_PREFIX %+ _CBB_init_fixed
%xdefine CBB_len BORINGSSL_PREFIX %+ _CBB_len
%xdefine CBB_reserve BORINGSSL_PREFIX %+ _CBB_reserve
%xdefine CBB_zero BORINGSSL_PREFIX %+ _CBB_zero
%xdefine CBS_asn1_ber_to_der BORINGSSL_PREFIX %+ _CBS_asn1_ber_to_der
%xdefine CBS_asn1_bitstring_has_bit BORINGSSL_PREFIX %+ _CBS_asn1_bitstring_has_bit
%xdefine CBS_asn1_oid_to_text BORINGSSL_PREFIX %+ _CBS_asn1_oid_to_text
%xdefine CBS_contains_zero_byte BORINGSSL_PREFIX %+ _CBS_contains_zero_byte
%xdefine CBS_copy_bytes BORINGSSL_PREFIX %+ _CBS_copy_bytes
%xdefine CBS_data BORINGSSL_PREFIX %+ _CBS_data
%xdefine CBS_get_any_asn1 BORINGSSL_PREFIX %+ _CBS_get_any_asn1
%xdefine CBS_get_any_asn1_element BORINGSSL_PREFIX %+ _CBS_get_any_asn1_element
%xdefine CBS_get_any_ber_asn1_element BORINGSSL_PREFIX %+ _CBS_get_any_ber_asn1_element
%xdefine CBS_get_asn1 BORINGSSL_PREFIX %+ _CBS_get_asn1
%xdefine CBS_get_asn1_bool BORINGSSL_PREFIX %+ _CBS_get_asn1_bool
%xdefine CBS_get_asn1_element BORINGSSL_PREFIX %+ _CBS_get_asn1_element
%xdefine CBS_get_asn1_implicit_string BORINGSSL_PREFIX %+ _CBS_get_asn1_implicit_string
%xdefine CBS_get_asn1_int64 BORINGSSL_PREFIX %+ _CBS_get_asn1_int64
%xdefine CBS_get_asn1_uint64 BORINGSSL_PREFIX %+ _CBS_get_asn1_uint64
%xdefine CBS_get_bytes BORINGSSL_PREFIX %+ _CBS_get_bytes
%xdefine CBS_get_last_u8 BORINGSSL_PREFIX %+ _CBS_get_last_u8
%xdefine CBS_get_optional_asn1 BORINGSSL_PREFIX %+ _CBS_get_optional_asn1
%xdefine CBS_get_optional_asn1_bool BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_bool
%xdefine CBS_get_optional_asn1_int64 BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_int64
%xdefine CBS_get_optional_asn1_octet_string BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_octet_string
%xdefine CBS_get_optional_asn1_uint64 BORINGSSL_PREFIX %+ _CBS_get_optional_asn1_uint64
%xdefine CBS_get_u16 BORINGSSL_PREFIX %+ _CBS_get_u16
%xdefine CBS_get_u16_length_prefixed BORINGSSL_PREFIX %+ _CBS_get_u16_length_prefixed
%xdefine CBS_get_u16le BORINGSSL_PREFIX %+ _CBS_get_u16le
%xdefine CBS_get_u24 BORINGSSL_PREFIX %+ _CBS_get_u24
%xdefine CBS_get_u24_length_prefixed BORINGSSL_PREFIX %+ _CBS_get_u24_length_prefixed
%xdefine CBS_get_u32 BORINGSSL_PREFIX %+ _CBS_get_u32
%xdefine CBS_get_u32le BORINGSSL_PREFIX %+ _CBS_get_u32le
%xdefine CBS_get_u64 BORINGSSL_PREFIX %+ _CBS_get_u64
%xdefine CBS_get_u64_decimal BORINGSSL_PREFIX %+ _CBS_get_u64_decimal
%xdefine CBS_get_u64le BORINGSSL_PREFIX %+ _CBS_get_u64le
%xdefine CBS_get_u8 BORINGSSL_PREFIX %+ _CBS_get_u8
%xdefine CBS_get_u8_length_prefixed BORINGSSL_PREFIX %+ _CBS_get_u8_length_prefixed
%xdefine CBS_get_until_first BORINGSSL_PREFIX %+ _CBS_get_until_first
%xdefine CBS_init BORINGSSL_PREFIX %+ _CBS_init
%xdefine CBS_is_unsigned_asn1_integer BORINGSSL_PREFIX %+ _CBS_is_unsigned_asn1_integer
%xdefine CBS_is_valid_asn1_bitstring BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_bitstring
%xdefine CBS_is_valid_asn1_integer BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_integer
%xdefine CBS_is_valid_asn1_oid BORINGSSL_PREFIX %+ _CBS_is_valid_asn1_oid
%xdefine CBS_len BORINGSSL_PREFIX %+ _CBS_len
%xdefine CBS_mem_equal BORINGSSL_PREFIX %+ _CBS_mem_equal
%xdefine CBS_parse_generalized_time BORINGSSL_PREFIX %+ _CBS_parse_generalized_time
%xdefine CBS_parse_utc_time BORINGSSL_PREFIX %+ _CBS_parse_utc_time
%xdefine CBS_peek_asn1_tag BORINGSSL_PREFIX %+ _CBS_peek_asn1_tag
%xdefine CBS_skip BORINGSSL_PREFIX %+ _CBS_skip
%xdefine CBS_stow BORINGSSL_PREFIX %+ _CBS_stow
%xdefine CBS_strdup BORINGSSL_PREFIX %+ _CBS_strdup
%xdefine CERTIFICATEPOLICIES_free BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_free
%xdefine CERTIFICATEPOLICIES_it BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_it
%xdefine CERTIFICATEPOLICIES_new BORINGSSL_PREFIX %+ _CERTIFICATEPOLICIES_new
%xdefine CMAC_CTX_copy BORINGSSL_PREFIX %+ _CMAC_CTX_copy
%xdefine CMAC_CTX_free BORINGSSL_PREFIX %+ _CMAC_CTX_free
%xdefine CMAC_CTX_get0_cipher_ctx BORINGSSL_PREFIX %+ _CMAC_CTX_get0_cipher_ctx
%xdefine CMAC_CTX_new BORINGSSL_PREFIX %+ _CMAC_CTX_new
%xdefine CMAC_Final BORINGSSL_PREFIX %+ _CMAC_Final
%xdefine CMAC_Init BORINGSSL_PREFIX %+ _CMAC_Init
%xdefine CMAC_Reset BORINGSSL_PREFIX %+ _CMAC_Reset
%xdefine CMAC_Update BORINGSSL_PREFIX %+ _CMAC_Update
%xdefine CONF_VALUE_new BORINGSSL_PREFIX %+ _CONF_VALUE_new
%xdefine CONF_get1_default_config_file BORINGSSL_PREFIX %+ _CONF_get1_default_config_file
%xdefine CONF_modules_finish BORINGSSL_PREFIX %+ _CONF_modules_finish
%xdefine CONF_modules_free BORINGSSL_PREFIX %+ _CONF_modules_free
%xdefine CONF_modules_load_file BORINGSSL_PREFIX %+ _CONF_modules_load_file
%xdefine CONF_modules_unload BORINGSSL_PREFIX %+ _CONF_modules_unload
%xdefine CONF_parse_list BORINGSSL_PREFIX %+ _CONF_parse_list
%xdefine CRL_DIST_POINTS_free BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_free
%xdefine CRL_DIST_POINTS_it BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_it
%xdefine CRL_DIST_POINTS_new BORINGSSL_PREFIX %+ _CRL_DIST_POINTS_new
%xdefine CRYPTO_BUFFER_POOL_free BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_POOL_free
%xdefine CRYPTO_BUFFER_POOL_new BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_POOL_new
%xdefine CRYPTO_BUFFER_alloc BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_alloc
%xdefine CRYPTO_BUFFER_data BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_data
%xdefine CRYPTO_BUFFER_free BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_free
%xdefine CRYPTO_BUFFER_init_CBS BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_init_CBS
%xdefine CRYPTO_BUFFER_len BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_len
%xdefine CRYPTO_BUFFER_new BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new
%xdefine CRYPTO_BUFFER_new_from_CBS BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new_from_CBS
%xdefine CRYPTO_BUFFER_new_from_static_data_unsafe BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_new_from_static_data_unsafe
%xdefine CRYPTO_BUFFER_up_ref BORINGSSL_PREFIX %+ _CRYPTO_BUFFER_up_ref
%xdefine CRYPTO_MUTEX_cleanup BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_cleanup
%xdefine CRYPTO_MUTEX_init BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_init
%xdefine CRYPTO_MUTEX_lock_read BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_lock_read
%xdefine CRYPTO_MUTEX_lock_write BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_lock_write
%xdefine CRYPTO_MUTEX_unlock_read BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_unlock_read
%xdefine CRYPTO_MUTEX_unlock_write BORINGSSL_PREFIX %+ _CRYPTO_MUTEX_unlock_write
%xdefine CRYPTO_POLYVAL_finish BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_finish
%xdefine CRYPTO_POLYVAL_init BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_init
%xdefine CRYPTO_POLYVAL_update_blocks BORINGSSL_PREFIX %+ _CRYPTO_POLYVAL_update_blocks
%xdefine CRYPTO_STATIC_MUTEX_is_write_locked BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_is_write_locked
%xdefine CRYPTO_STATIC_MUTEX_lock_read BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_lock_read
%xdefine CRYPTO_STATIC_MUTEX_lock_write BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_lock_write
%xdefine CRYPTO_STATIC_MUTEX_unlock_read BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_unlock_read
%xdefine CRYPTO_STATIC_MUTEX_unlock_write BORINGSSL_PREFIX %+ _CRYPTO_STATIC_MUTEX_unlock_write
%xdefine CRYPTO_THREADID_current BORINGSSL_PREFIX %+ _CRYPTO_THREADID_current
%xdefine CRYPTO_THREADID_set_callback BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_callback
%xdefine CRYPTO_THREADID_set_numeric BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_numeric
%xdefine CRYPTO_THREADID_set_pointer BORINGSSL_PREFIX %+ _CRYPTO_THREADID_set_pointer
%xdefine CRYPTO_cbc128_decrypt BORINGSSL_PREFIX %+ _CRYPTO_cbc128_decrypt
%xdefine CRYPTO_cbc128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_cbc128_encrypt
%xdefine CRYPTO_cfb128_1_encrypt BORINGSSL_PREFIX %+ _CRYPTO_cfb128_1_encrypt
%xdefine CRYPTO_cfb128_8_encrypt BORINGSSL_PREFIX %+ _CRYPTO_cfb128_8_encrypt
%xdefine CRYPTO_cfb128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_cfb128_encrypt
%xdefine CRYPTO_chacha_20 BORINGSSL_PREFIX %+ _CRYPTO_chacha_20
%xdefine CRYPTO_cleanup_all_ex_data BORINGSSL_PREFIX %+ _CRYPTO_cleanup_all_ex_data
%xdefine CRYPTO_ctr128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_ctr128_encrypt
%xdefine CRYPTO_ctr128_encrypt_ctr32 BORINGSSL_PREFIX %+ _CRYPTO_ctr128_encrypt_ctr32
%xdefine CRYPTO_fork_detect_ignore_inheritzero_FOR_TESTING BORINGSSL_PREFIX %+ _CRYPTO_fork_detect_ignore_inheritzero_FOR_TESTING
%xdefine CRYPTO_fork_detect_ignore_wipeonfork_FOR_TESTING BORINGSSL_PREFIX %+ _CRYPTO_fork_detect_ignore_wipeonfork_FOR_TESTING
%xdefine CRYPTO_free BORINGSSL_PREFIX %+ _CRYPTO_free
%xdefine CRYPTO_free_ex_data BORINGSSL_PREFIX %+ _CRYPTO_free_ex_data
%xdefine CRYPTO_gcm128_aad BORINGSSL_PREFIX %+ _CRYPTO_gcm128_aad
%xdefine CRYPTO_gcm128_decrypt BORINGSSL_PREFIX %+ _CRYPTO_gcm128_decrypt
%xdefine CRYPTO_gcm128_decrypt_ctr32 BORINGSSL_PREFIX %+ _CRYPTO_gcm128_decrypt_ctr32
%xdefine CRYPTO_gcm128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_gcm128_encrypt
%xdefine CRYPTO_gcm128_encrypt_ctr32 BORINGSSL_PREFIX %+ _CRYPTO_gcm128_encrypt_ctr32
%xdefine CRYPTO_gcm128_finish BORINGSSL_PREFIX %+ _CRYPTO_gcm128_finish
%xdefine CRYPTO_gcm128_init_key BORINGSSL_PREFIX %+ _CRYPTO_gcm128_init_key
%xdefine CRYPTO_gcm128_setiv BORINGSSL_PREFIX %+ _CRYPTO_gcm128_setiv
%xdefine CRYPTO_gcm128_tag BORINGSSL_PREFIX %+ _CRYPTO_gcm128_tag
%xdefine CRYPTO_get_dynlock_create_callback BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_create_callback
%xdefine CRYPTO_get_dynlock_destroy_callback BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_destroy_callback
%xdefine CRYPTO_get_dynlock_lock_callback BORINGSSL_PREFIX %+ _CRYPTO_get_dynlock_lock_callback
%xdefine CRYPTO_get_ex_data BORINGSSL_PREFIX %+ _CRYPTO_get_ex_data
%xdefine CRYPTO_get_ex_new_index BORINGSSL_PREFIX %+ _CRYPTO_get_ex_new_index
%xdefine CRYPTO_get_fork_ube_generation BORINGSSL_PREFIX %+ _CRYPTO_get_fork_ube_generation
%xdefine CRYPTO_get_lock_name BORINGSSL_PREFIX %+ _CRYPTO_get_lock_name
%xdefine CRYPTO_get_locking_callback BORINGSSL_PREFIX %+ _CRYPTO_get_locking_callback
%xdefine CRYPTO_get_sysgenid_path BORINGSSL_PREFIX %+ _CRYPTO_get_sysgenid_path
%xdefine CRYPTO_get_thread_local BORINGSSL_PREFIX %+ _CRYPTO_get_thread_local
%xdefine CRYPTO_get_ube_generation_number BORINGSSL_PREFIX %+ _CRYPTO_get_ube_generation_number
%xdefine CRYPTO_get_vm_ube_active BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_active
%xdefine CRYPTO_get_vm_ube_generation BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_generation
%xdefine CRYPTO_get_vm_ube_supported BORINGSSL_PREFIX %+ _CRYPTO_get_vm_ube_supported
%xdefine CRYPTO_ghash_init BORINGSSL_PREFIX %+ _CRYPTO_ghash_init
%xdefine CRYPTO_has_asm BORINGSSL_PREFIX %+ _CRYPTO_has_asm
%xdefine CRYPTO_has_broken_NEON BORINGSSL_PREFIX %+ _CRYPTO_has_broken_NEON
%xdefine CRYPTO_hchacha20 BORINGSSL_PREFIX %+ _CRYPTO_hchacha20
%xdefine CRYPTO_is_ARMv8_DIT_capable_for_testing BORINGSSL_PREFIX %+ _CRYPTO_is_ARMv8_DIT_capable_for_testing
%xdefine CRYPTO_is_PPC64LE_vcrypto_capable BORINGSSL_PREFIX %+ _CRYPTO_is_PPC64LE_vcrypto_capable
%xdefine CRYPTO_is_confidential_build BORINGSSL_PREFIX %+ _CRYPTO_is_confidential_build
%xdefine CRYPTO_library_init BORINGSSL_PREFIX %+ _CRYPTO_library_init
%xdefine CRYPTO_malloc BORINGSSL_PREFIX %+ _CRYPTO_malloc
%xdefine CRYPTO_malloc_init BORINGSSL_PREFIX %+ _CRYPTO_malloc_init
%xdefine CRYPTO_mem_ctrl BORINGSSL_PREFIX %+ _CRYPTO_mem_ctrl
%xdefine CRYPTO_memcmp BORINGSSL_PREFIX %+ _CRYPTO_memcmp
%xdefine CRYPTO_needs_hwcap2_workaround BORINGSSL_PREFIX %+ _CRYPTO_needs_hwcap2_workaround
%xdefine CRYPTO_new_ex_data BORINGSSL_PREFIX %+ _CRYPTO_new_ex_data
%xdefine CRYPTO_num_locks BORINGSSL_PREFIX %+ _CRYPTO_num_locks
%xdefine CRYPTO_ofb128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_ofb128_encrypt
%xdefine CRYPTO_once BORINGSSL_PREFIX %+ _CRYPTO_once
%xdefine CRYPTO_poly1305_finish BORINGSSL_PREFIX %+ _CRYPTO_poly1305_finish
%xdefine CRYPTO_poly1305_finish_neon BORINGSSL_PREFIX %+ _CRYPTO_poly1305_finish_neon
%xdefine CRYPTO_poly1305_init BORINGSSL_PREFIX %+ _CRYPTO_poly1305_init
%xdefine CRYPTO_poly1305_init_neon BORINGSSL_PREFIX %+ _CRYPTO_poly1305_init_neon
%xdefine CRYPTO_poly1305_update BORINGSSL_PREFIX %+ _CRYPTO_poly1305_update
%xdefine CRYPTO_poly1305_update_neon BORINGSSL_PREFIX %+ _CRYPTO_poly1305_update_neon
%xdefine CRYPTO_pre_sandbox_init BORINGSSL_PREFIX %+ _CRYPTO_pre_sandbox_init
%xdefine CRYPTO_rdrand_multiple8 BORINGSSL_PREFIX %+ _CRYPTO_rdrand_multiple8
%xdefine CRYPTO_realloc BORINGSSL_PREFIX %+ _CRYPTO_realloc
%xdefine CRYPTO_refcount_dec_and_test_zero BORINGSSL_PREFIX %+ _CRYPTO_refcount_dec_and_test_zero
%xdefine CRYPTO_refcount_inc BORINGSSL_PREFIX %+ _CRYPTO_refcount_inc
%xdefine CRYPTO_rndr_multiple8 BORINGSSL_PREFIX %+ _CRYPTO_rndr_multiple8
%xdefine CRYPTO_secure_malloc_init BORINGSSL_PREFIX %+ _CRYPTO_secure_malloc_init
%xdefine CRYPTO_secure_malloc_initialized BORINGSSL_PREFIX %+ _CRYPTO_secure_malloc_initialized
%xdefine CRYPTO_secure_used BORINGSSL_PREFIX %+ _CRYPTO_secure_used
%xdefine CRYPTO_set_add_lock_callback BORINGSSL_PREFIX %+ _CRYPTO_set_add_lock_callback
%xdefine CRYPTO_set_dynlock_create_callback BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_create_callback
%xdefine CRYPTO_set_dynlock_destroy_callback BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_destroy_callback
%xdefine CRYPTO_set_dynlock_lock_callback BORINGSSL_PREFIX %+ _CRYPTO_set_dynlock_lock_callback
%xdefine CRYPTO_set_ex_data BORINGSSL_PREFIX %+ _CRYPTO_set_ex_data
%xdefine CRYPTO_set_id_callback BORINGSSL_PREFIX %+ _CRYPTO_set_id_callback
%xdefine CRYPTO_set_locking_callback BORINGSSL_PREFIX %+ _CRYPTO_set_locking_callback
%xdefine CRYPTO_set_mem_functions BORINGSSL_PREFIX %+ _CRYPTO_set_mem_functions
%xdefine CRYPTO_set_thread_local BORINGSSL_PREFIX %+ _CRYPTO_set_thread_local
%xdefine CRYPTO_sysrand BORINGSSL_PREFIX %+ _CRYPTO_sysrand
%xdefine CRYPTO_sysrand_if_available BORINGSSL_PREFIX %+ _CRYPTO_sysrand_if_available
%xdefine CRYPTO_tls1_prf BORINGSSL_PREFIX %+ _CRYPTO_tls1_prf
%xdefine CRYPTO_xts128_encrypt BORINGSSL_PREFIX %+ _CRYPTO_xts128_encrypt
%xdefine CTR_DRBG_clear BORINGSSL_PREFIX %+ _CTR_DRBG_clear
%xdefine CTR_DRBG_free BORINGSSL_PREFIX %+ _CTR_DRBG_free
%xdefine CTR_DRBG_generate BORINGSSL_PREFIX %+ _CTR_DRBG_generate
%xdefine CTR_DRBG_init BORINGSSL_PREFIX %+ _CTR_DRBG_init
%xdefine CTR_DRBG_new BORINGSSL_PREFIX %+ _CTR_DRBG_new
%xdefine CTR_DRBG_reseed BORINGSSL_PREFIX %+ _CTR_DRBG_reseed
%xdefine ChaCha20_ctr32_avx2 BORINGSSL_PREFIX %+ _ChaCha20_ctr32_avx2
%xdefine ChaCha20_ctr32_neon BORINGSSL_PREFIX %+ _ChaCha20_ctr32_neon
%xdefine ChaCha20_ctr32_nohw BORINGSSL_PREFIX %+ _ChaCha20_ctr32_nohw
%xdefine ChaCha20_ctr32_ssse3 BORINGSSL_PREFIX %+ _ChaCha20_ctr32_ssse3
%xdefine ChaCha20_ctr32_ssse3_4x BORINGSSL_PREFIX %+ _ChaCha20_ctr32_ssse3_4x
%xdefine DES_decrypt3 BORINGSSL_PREFIX %+ _DES_decrypt3
%xdefine DES_ecb3_encrypt BORINGSSL_PREFIX %+ _DES_ecb3_encrypt
%xdefine DES_ecb3_encrypt_ex BORINGSSL_PREFIX %+ _DES_ecb3_encrypt_ex
%xdefine DES_ecb_encrypt BORINGSSL_PREFIX %+ _DES_ecb_encrypt
%xdefine DES_ecb_encrypt_ex BORINGSSL_PREFIX %+ _DES_ecb_encrypt_ex
%xdefine DES_ede2_cbc_encrypt BORINGSSL_PREFIX %+ _DES_ede2_cbc_encrypt
%xdefine DES_ede3_cbc_encrypt BORINGSSL_PREFIX %+ _DES_ede3_cbc_encrypt
%xdefine DES_ede3_cbc_encrypt_ex BORINGSSL_PREFIX %+ _DES_ede3_cbc_encrypt_ex
%xdefine DES_encrypt3 BORINGSSL_PREFIX %+ _DES_encrypt3
%xdefine DES_is_weak_key BORINGSSL_PREFIX %+ _DES_is_weak_key
%xdefine DES_key_sched BORINGSSL_PREFIX %+ _DES_key_sched
%xdefine DES_ncbc_encrypt BORINGSSL_PREFIX %+ _DES_ncbc_encrypt
%xdefine DES_ncbc_encrypt_ex BORINGSSL_PREFIX %+ _DES_ncbc_encrypt_ex
%xdefine DES_set_key BORINGSSL_PREFIX %+ _DES_set_key
%xdefine DES_set_key_ex BORINGSSL_PREFIX %+ _DES_set_key_ex
%xdefine DES_set_key_unchecked BORINGSSL_PREFIX %+ _DES_set_key_unchecked
%xdefine DES_set_odd_parity BORINGSSL_PREFIX %+ _DES_set_odd_parity
%xdefine DH_bits BORINGSSL_PREFIX %+ _DH_bits
%xdefine DH_check BORINGSSL_PREFIX %+ _DH_check
%xdefine DH_check_pub_key BORINGSSL_PREFIX %+ _DH_check_pub_key
%xdefine DH_clear_flags BORINGSSL_PREFIX %+ _DH_clear_flags
%xdefine DH_compute_key BORINGSSL_PREFIX %+ _DH_compute_key
%xdefine DH_compute_key_hashed BORINGSSL_PREFIX %+ _DH_compute_key_hashed
%xdefine DH_compute_key_padded BORINGSSL_PREFIX %+ _DH_compute_key_padded
%xdefine DH_free BORINGSSL_PREFIX %+ _DH_free
%xdefine DH_generate_key BORINGSSL_PREFIX %+ _DH_generate_key
%xdefine DH_generate_parameters BORINGSSL_PREFIX %+ _DH_generate_parameters
%xdefine DH_generate_parameters_ex BORINGSSL_PREFIX %+ _DH_generate_parameters_ex
%xdefine DH_get0_g BORINGSSL_PREFIX %+ _DH_get0_g
%xdefine DH_get0_key BORINGSSL_PREFIX %+ _DH_get0_key
%xdefine DH_get0_p BORINGSSL_PREFIX %+ _DH_get0_p
%xdefine DH_get0_pqg BORINGSSL_PREFIX %+ _DH_get0_pqg
%xdefine DH_get0_priv_key BORINGSSL_PREFIX %+ _DH_get0_priv_key
%xdefine DH_get0_pub_key BORINGSSL_PREFIX %+ _DH_get0_pub_key
%xdefine DH_get0_q BORINGSSL_PREFIX %+ _DH_get0_q
%xdefine DH_get_2048_256 BORINGSSL_PREFIX %+ _DH_get_2048_256
%xdefine DH_get_rfc7919_2048 BORINGSSL_PREFIX %+ _DH_get_rfc7919_2048
%xdefine DH_get_rfc7919_3072 BORINGSSL_PREFIX %+ _DH_get_rfc7919_3072
%xdefine DH_get_rfc7919_4096 BORINGSSL_PREFIX %+ _DH_get_rfc7919_4096
%xdefine DH_get_rfc7919_8192 BORINGSSL_PREFIX %+ _DH_get_rfc7919_8192
%xdefine DH_marshal_parameters BORINGSSL_PREFIX %+ _DH_marshal_parameters
%xdefine DH_new BORINGSSL_PREFIX %+ _DH_new
%xdefine DH_new_by_nid BORINGSSL_PREFIX %+ _DH_new_by_nid
%xdefine DH_num_bits BORINGSSL_PREFIX %+ _DH_num_bits
%xdefine DH_parse_parameters BORINGSSL_PREFIX %+ _DH_parse_parameters
%xdefine DH_set0_key BORINGSSL_PREFIX %+ _DH_set0_key
%xdefine DH_set0_pqg BORINGSSL_PREFIX %+ _DH_set0_pqg
%xdefine DH_set_length BORINGSSL_PREFIX %+ _DH_set_length
%xdefine DH_size BORINGSSL_PREFIX %+ _DH_size
%xdefine DH_up_ref BORINGSSL_PREFIX %+ _DH_up_ref
%xdefine DHparams_dup BORINGSSL_PREFIX %+ _DHparams_dup
%xdefine DIRECTORYSTRING_free BORINGSSL_PREFIX %+ _DIRECTORYSTRING_free
%xdefine DIRECTORYSTRING_it BORINGSSL_PREFIX %+ _DIRECTORYSTRING_it
%xdefine DIRECTORYSTRING_new BORINGSSL_PREFIX %+ _DIRECTORYSTRING_new
%xdefine DISPLAYTEXT_free BORINGSSL_PREFIX %+ _DISPLAYTEXT_free
%xdefine DISPLAYTEXT_it BORINGSSL_PREFIX %+ _DISPLAYTEXT_it
%xdefine DISPLAYTEXT_new BORINGSSL_PREFIX %+ _DISPLAYTEXT_new
%xdefine DIST_POINT_NAME_free BORINGSSL_PREFIX %+ _DIST_POINT_NAME_free
%xdefine DIST_POINT_NAME_it BORINGSSL_PREFIX %+ _DIST_POINT_NAME_it
%xdefine DIST_POINT_NAME_new BORINGSSL_PREFIX %+ _DIST_POINT_NAME_new
%xdefine DIST_POINT_free BORINGSSL_PREFIX %+ _DIST_POINT_free
%xdefine DIST_POINT_it BORINGSSL_PREFIX %+ _DIST_POINT_it
%xdefine DIST_POINT_new BORINGSSL_PREFIX %+ _DIST_POINT_new
%xdefine DIST_POINT_set_dpname BORINGSSL_PREFIX %+ _DIST_POINT_set_dpname
%xdefine DSA_SIG_free BORINGSSL_PREFIX %+ _DSA_SIG_free
%xdefine DSA_SIG_get0 BORINGSSL_PREFIX %+ _DSA_SIG_get0
%xdefine DSA_SIG_marshal BORINGSSL_PREFIX %+ _DSA_SIG_marshal
%xdefine DSA_SIG_new BORINGSSL_PREFIX %+ _DSA_SIG_new
%xdefine DSA_SIG_parse BORINGSSL_PREFIX %+ _DSA_SIG_parse
%xdefine DSA_SIG_set0 BORINGSSL_PREFIX %+ _DSA_SIG_set0
%xdefine DSA_bits BORINGSSL_PREFIX %+ _DSA_bits
%xdefine DSA_check_signature BORINGSSL_PREFIX %+ _DSA_check_signature
%xdefine DSA_do_check_signature BORINGSSL_PREFIX %+ _DSA_do_check_signature
%xdefine DSA_do_sign BORINGSSL_PREFIX %+ _DSA_do_sign
%xdefine DSA_do_verify BORINGSSL_PREFIX %+ _DSA_do_verify
%xdefine DSA_dup_DH BORINGSSL_PREFIX %+ _DSA_dup_DH
%xdefine DSA_free BORINGSSL_PREFIX %+ _DSA_free
%xdefine DSA_generate_key BORINGSSL_PREFIX %+ _DSA_generate_key
%xdefine DSA_generate_parameters_ex BORINGSSL_PREFIX %+ _DSA_generate_parameters_ex
%xdefine DSA_get0_g BORINGSSL_PREFIX %+ _DSA_get0_g
%xdefine DSA_get0_key BORINGSSL_PREFIX %+ _DSA_get0_key
%xdefine DSA_get0_p BORINGSSL_PREFIX %+ _DSA_get0_p
%xdefine DSA_get0_pqg BORINGSSL_PREFIX %+ _DSA_get0_pqg
%xdefine DSA_get0_priv_key BORINGSSL_PREFIX %+ _DSA_get0_priv_key
%xdefine DSA_get0_pub_key BORINGSSL_PREFIX %+ _DSA_get0_pub_key
%xdefine DSA_get0_q BORINGSSL_PREFIX %+ _DSA_get0_q
%xdefine DSA_get_ex_data BORINGSSL_PREFIX %+ _DSA_get_ex_data
%xdefine DSA_get_ex_new_index BORINGSSL_PREFIX %+ _DSA_get_ex_new_index
%xdefine DSA_marshal_parameters BORINGSSL_PREFIX %+ _DSA_marshal_parameters
%xdefine DSA_marshal_private_key BORINGSSL_PREFIX %+ _DSA_marshal_private_key
%xdefine DSA_marshal_public_key BORINGSSL_PREFIX %+ _DSA_marshal_public_key
%xdefine DSA_new BORINGSSL_PREFIX %+ _DSA_new
%xdefine DSA_parse_parameters BORINGSSL_PREFIX %+ _DSA_parse_parameters
%xdefine DSA_parse_private_key BORINGSSL_PREFIX %+ _DSA_parse_private_key
%xdefine DSA_parse_public_key BORINGSSL_PREFIX %+ _DSA_parse_public_key
%xdefine DSA_print BORINGSSL_PREFIX %+ _DSA_print
%xdefine DSA_print_fp BORINGSSL_PREFIX %+ _DSA_print_fp
%xdefine DSA_set0_key BORINGSSL_PREFIX %+ _DSA_set0_key
%xdefine DSA_set0_pqg BORINGSSL_PREFIX %+ _DSA_set0_pqg
%xdefine DSA_set_ex_data BORINGSSL_PREFIX %+ _DSA_set_ex_data
%xdefine DSA_sign BORINGSSL_PREFIX %+ _DSA_sign
%xdefine DSA_size BORINGSSL_PREFIX %+ _DSA_size
%xdefine DSA_up_ref BORINGSSL_PREFIX %+ _DSA_up_ref
%xdefine DSA_verify BORINGSSL_PREFIX %+ _DSA_verify
%xdefine DSAparams_dup BORINGSSL_PREFIX %+ _DSAparams_dup
%xdefine ECDH_compute_key BORINGSSL_PREFIX %+ _ECDH_compute_key
%xdefine ECDH_compute_key_fips BORINGSSL_PREFIX %+ _ECDH_compute_key_fips
%xdefine ECDH_compute_shared_secret BORINGSSL_PREFIX %+ _ECDH_compute_shared_secret
%xdefine ECDSA_SIG_free BORINGSSL_PREFIX %+ _ECDSA_SIG_free
%xdefine ECDSA_SIG_from_bytes BORINGSSL_PREFIX %+ _ECDSA_SIG_from_bytes
%xdefine ECDSA_SIG_get0 BORINGSSL_PREFIX %+ _ECDSA_SIG_get0
%xdefine ECDSA_SIG_get0_r BORINGSSL_PREFIX %+ _ECDSA_SIG_get0_r
%xdefine ECDSA_SIG_get0_s BORINGSSL_PREFIX %+ _ECDSA_SIG_get0_s
%xdefine ECDSA_SIG_marshal BORINGSSL_PREFIX %+ _ECDSA_SIG_marshal
%xdefine ECDSA_SIG_max_len BORINGSSL_PREFIX %+ _ECDSA_SIG_max_len
%xdefine ECDSA_SIG_new BORINGSSL_PREFIX %+ _ECDSA_SIG_new
%xdefine ECDSA_SIG_parse BORINGSSL_PREFIX %+ _ECDSA_SIG_parse
%xdefine ECDSA_SIG_set0 BORINGSSL_PREFIX %+ _ECDSA_SIG_set0
%xdefine ECDSA_SIG_to_bytes BORINGSSL_PREFIX %+ _ECDSA_SIG_to_bytes
%xdefine ECDSA_do_sign BORINGSSL_PREFIX %+ _ECDSA_do_sign
%xdefine ECDSA_do_verify BORINGSSL_PREFIX %+ _ECDSA_do_verify
%xdefine ECDSA_sign BORINGSSL_PREFIX %+ _ECDSA_sign
%xdefine ECDSA_sign_with_nonce_and_leak_private_key_for_testing BORINGSSL_PREFIX %+ _ECDSA_sign_with_nonce_and_leak_private_key_for_testing
%xdefine ECDSA_size BORINGSSL_PREFIX %+ _ECDSA_size
%xdefine ECDSA_verify BORINGSSL_PREFIX %+ _ECDSA_verify
%xdefine ECPKParameters_print BORINGSSL_PREFIX %+ _ECPKParameters_print
%xdefine EC_GFp_mont_method BORINGSSL_PREFIX %+ _EC_GFp_mont_method
%xdefine EC_GFp_nistp224_method BORINGSSL_PREFIX %+ _EC_GFp_nistp224_method
%xdefine EC_GFp_nistp256_method BORINGSSL_PREFIX %+ _EC_GFp_nistp256_method
%xdefine EC_GFp_nistp384_method BORINGSSL_PREFIX %+ _EC_GFp_nistp384_method
%xdefine EC_GFp_nistp521_method BORINGSSL_PREFIX %+ _EC_GFp_nistp521_method
%xdefine EC_GFp_nistz256_method BORINGSSL_PREFIX %+ _EC_GFp_nistz256_method
%xdefine EC_GROUP_cmp BORINGSSL_PREFIX %+ _EC_GROUP_cmp
%xdefine EC_GROUP_dup BORINGSSL_PREFIX %+ _EC_GROUP_dup
%xdefine EC_GROUP_free BORINGSSL_PREFIX %+ _EC_GROUP_free
%xdefine EC_GROUP_get0_generator BORINGSSL_PREFIX %+ _EC_GROUP_get0_generator
%xdefine EC_GROUP_get0_order BORINGSSL_PREFIX %+ _EC_GROUP_get0_order
%xdefine EC_GROUP_get0_seed BORINGSSL_PREFIX %+ _EC_GROUP_get0_seed
%xdefine EC_GROUP_get_asn1_flag BORINGSSL_PREFIX %+ _EC_GROUP_get_asn1_flag
%xdefine EC_GROUP_get_cofactor BORINGSSL_PREFIX %+ _EC_GROUP_get_cofactor
%xdefine EC_GROUP_get_curve_GFp BORINGSSL_PREFIX %+ _EC_GROUP_get_curve_GFp
%xdefine EC_GROUP_get_curve_name BORINGSSL_PREFIX %+ _EC_GROUP_get_curve_name
%xdefine EC_GROUP_get_degree BORINGSSL_PREFIX %+ _EC_GROUP_get_degree
%xdefine EC_GROUP_get_order BORINGSSL_PREFIX %+ _EC_GROUP_get_order
%xdefine EC_GROUP_get_point_conversion_form BORINGSSL_PREFIX %+ _EC_GROUP_get_point_conversion_form
%xdefine EC_GROUP_get_seed_len BORINGSSL_PREFIX %+ _EC_GROUP_get_seed_len
%xdefine EC_GROUP_method_of BORINGSSL_PREFIX %+ _EC_GROUP_method_of
%xdefine EC_GROUP_new_by_curve_name BORINGSSL_PREFIX %+ _EC_GROUP_new_by_curve_name
%xdefine EC_GROUP_new_by_curve_name_mutable BORINGSSL_PREFIX %+ _EC_GROUP_new_by_curve_name_mutable
%xdefine EC_GROUP_new_curve_GFp BORINGSSL_PREFIX %+ _EC_GROUP_new_curve_GFp
%xdefine EC_GROUP_order_bits BORINGSSL_PREFIX %+ _EC_GROUP_order_bits
%xdefine EC_GROUP_set_asn1_flag BORINGSSL_PREFIX %+ _EC_GROUP_set_asn1_flag
%xdefine EC_GROUP_set_generator BORINGSSL_PREFIX %+ _EC_GROUP_set_generator
%xdefine EC_GROUP_set_point_conversion_form BORINGSSL_PREFIX %+ _EC_GROUP_set_point_conversion_form
%xdefine EC_GROUP_set_seed BORINGSSL_PREFIX %+ _EC_GROUP_set_seed
%xdefine EC_KEY_METHOD_free BORINGSSL_PREFIX %+ _EC_KEY_METHOD_free
%xdefine EC_KEY_METHOD_new BORINGSSL_PREFIX %+ _EC_KEY_METHOD_new
%xdefine EC_KEY_METHOD_set_flags BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_flags
%xdefine EC_KEY_METHOD_set_init_awslc BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_init_awslc
%xdefine EC_KEY_METHOD_set_sign_awslc BORINGSSL_PREFIX %+ _EC_KEY_METHOD_set_sign_awslc
%xdefine EC_KEY_OpenSSL BORINGSSL_PREFIX %+ _EC_KEY_OpenSSL
%xdefine EC_KEY_check_fips BORINGSSL_PREFIX %+ _EC_KEY_check_fips
%xdefine EC_KEY_check_key BORINGSSL_PREFIX %+ _EC_KEY_check_key
%xdefine EC_KEY_decoded_from_explicit_params BORINGSSL_PREFIX %+ _EC_KEY_decoded_from_explicit_params
%xdefine EC_KEY_derive_from_secret BORINGSSL_PREFIX %+ _EC_KEY_derive_from_secret
%xdefine EC_KEY_dup BORINGSSL_PREFIX %+ _EC_KEY_dup
%xdefine EC_KEY_free BORINGSSL_PREFIX %+ _EC_KEY_free
%xdefine EC_KEY_generate_key BORINGSSL_PREFIX %+ _EC_KEY_generate_key
%xdefine EC_KEY_generate_key_fips BORINGSSL_PREFIX %+ _EC_KEY_generate_key_fips
%xdefine EC_KEY_get0_group BORINGSSL_PREFIX %+ _EC_KEY_get0_group
%xdefine EC_KEY_get0_private_key BORINGSSL_PREFIX %+ _EC_KEY_get0_private_key
%xdefine EC_KEY_get0_public_key BORINGSSL_PREFIX %+ _EC_KEY_get0_public_key
%xdefine EC_KEY_get_conv_form BORINGSSL_PREFIX %+ _EC_KEY_get_conv_form
%xdefine EC_KEY_get_default_method BORINGSSL_PREFIX %+ _EC_KEY_get_default_method
%xdefine EC_KEY_get_enc_flags BORINGSSL_PREFIX %+ _EC_KEY_get_enc_flags
%xdefine EC_KEY_get_ex_data BORINGSSL_PREFIX %+ _EC_KEY_get_ex_data
%xdefine EC_KEY_get_ex_new_index BORINGSSL_PREFIX %+ _EC_KEY_get_ex_new_index
%xdefine EC_KEY_get_method BORINGSSL_PREFIX %+ _EC_KEY_get_method
%xdefine EC_KEY_is_opaque BORINGSSL_PREFIX %+ _EC_KEY_is_opaque
%xdefine EC_KEY_key2buf BORINGSSL_PREFIX %+ _EC_KEY_key2buf
%xdefine EC_KEY_marshal_curve_name BORINGSSL_PREFIX %+ _EC_KEY_marshal_curve_name
%xdefine EC_KEY_marshal_private_key BORINGSSL_PREFIX %+ _EC_KEY_marshal_private_key
%xdefine EC_KEY_new BORINGSSL_PREFIX %+ _EC_KEY_new
%xdefine EC_KEY_new_by_curve_name BORINGSSL_PREFIX %+ _EC_KEY_new_by_curve_name
%xdefine EC_KEY_new_method BORINGSSL_PREFIX %+ _EC_KEY_new_method
%xdefine EC_KEY_parse_curve_name BORINGSSL_PREFIX %+ _EC_KEY_parse_curve_name
%xdefine EC_KEY_parse_parameters BORINGSSL_PREFIX %+ _EC_KEY_parse_parameters
%xdefine EC_KEY_parse_parameters_and_type BORINGSSL_PREFIX %+ _EC_KEY_parse_parameters_and_type
%xdefine EC_KEY_parse_private_key BORINGSSL_PREFIX %+ _EC_KEY_parse_private_key
%xdefine EC_KEY_set_asn1_flag BORINGSSL_PREFIX %+ _EC_KEY_set_asn1_flag
%xdefine EC_KEY_set_conv_form BORINGSSL_PREFIX %+ _EC_KEY_set_conv_form
%xdefine EC_KEY_set_enc_flags BORINGSSL_PREFIX %+ _EC_KEY_set_enc_flags
%xdefine EC_KEY_set_ex_data BORINGSSL_PREFIX %+ _EC_KEY_set_ex_data
%xdefine EC_KEY_set_group BORINGSSL_PREFIX %+ _EC_KEY_set_group
%xdefine EC_KEY_set_method BORINGSSL_PREFIX %+ _EC_KEY_set_method
%xdefine EC_KEY_set_private_key BORINGSSL_PREFIX %+ _EC_KEY_set_private_key
%xdefine EC_KEY_set_public_key BORINGSSL_PREFIX %+ _EC_KEY_set_public_key
%xdefine EC_KEY_set_public_key_affine_coordinates BORINGSSL_PREFIX %+ _EC_KEY_set_public_key_affine_coordinates
%xdefine EC_KEY_up_ref BORINGSSL_PREFIX %+ _EC_KEY_up_ref
%xdefine EC_METHOD_get_field_type BORINGSSL_PREFIX %+ _EC_METHOD_get_field_type
%xdefine EC_POINT_add BORINGSSL_PREFIX %+ _EC_POINT_add
%xdefine EC_POINT_bn2point BORINGSSL_PREFIX %+ _EC_POINT_bn2point
%xdefine EC_POINT_clear_free BORINGSSL_PREFIX %+ _EC_POINT_clear_free
%xdefine EC_POINT_cmp BORINGSSL_PREFIX %+ _EC_POINT_cmp
%xdefine EC_POINT_copy BORINGSSL_PREFIX %+ _EC_POINT_copy
%xdefine EC_POINT_dbl BORINGSSL_PREFIX %+ _EC_POINT_dbl
%xdefine EC_POINT_dup BORINGSSL_PREFIX %+ _EC_POINT_dup
%xdefine EC_POINT_free BORINGSSL_PREFIX %+ _EC_POINT_free
%xdefine EC_POINT_get_affine_coordinates BORINGSSL_PREFIX %+ _EC_POINT_get_affine_coordinates
%xdefine EC_POINT_get_affine_coordinates_GFp BORINGSSL_PREFIX %+ _EC_POINT_get_affine_coordinates_GFp
%xdefine EC_POINT_invert BORINGSSL_PREFIX %+ _EC_POINT_invert
%xdefine EC_POINT_is_at_infinity BORINGSSL_PREFIX %+ _EC_POINT_is_at_infinity
%xdefine EC_POINT_is_on_curve BORINGSSL_PREFIX %+ _EC_POINT_is_on_curve
%xdefine EC_POINT_mul BORINGSSL_PREFIX %+ _EC_POINT_mul
%xdefine EC_POINT_new BORINGSSL_PREFIX %+ _EC_POINT_new
%xdefine EC_POINT_oct2point BORINGSSL_PREFIX %+ _EC_POINT_oct2point
%xdefine EC_POINT_point2bn BORINGSSL_PREFIX %+ _EC_POINT_point2bn
%xdefine EC_POINT_point2cbb BORINGSSL_PREFIX %+ _EC_POINT_point2cbb
%xdefine EC_POINT_point2oct BORINGSSL_PREFIX %+ _EC_POINT_point2oct
%xdefine EC_POINT_set_affine_coordinates BORINGSSL_PREFIX %+ _EC_POINT_set_affine_coordinates
%xdefine EC_POINT_set_affine_coordinates_GFp BORINGSSL_PREFIX %+ _EC_POINT_set_affine_coordinates_GFp
%xdefine EC_POINT_set_compressed_coordinates_GFp BORINGSSL_PREFIX %+ _EC_POINT_set_compressed_coordinates_GFp
%xdefine EC_POINT_set_to_infinity BORINGSSL_PREFIX %+ _EC_POINT_set_to_infinity
%xdefine EC_curve_nid2nist BORINGSSL_PREFIX %+ _EC_curve_nid2nist
%xdefine EC_curve_nist2nid BORINGSSL_PREFIX %+ _EC_curve_nist2nid
%xdefine EC_get_builtin_curves BORINGSSL_PREFIX %+ _EC_get_builtin_curves
%xdefine EC_group_p224 BORINGSSL_PREFIX %+ _EC_group_p224
%xdefine EC_group_p256 BORINGSSL_PREFIX %+ _EC_group_p256
%xdefine EC_group_p384 BORINGSSL_PREFIX %+ _EC_group_p384
%xdefine EC_group_p521 BORINGSSL_PREFIX %+ _EC_group_p521
%xdefine EC_group_secp256k1 BORINGSSL_PREFIX %+ _EC_group_secp256k1
%xdefine EC_hash_to_curve_p256_xmd_sha256_sswu BORINGSSL_PREFIX %+ _EC_hash_to_curve_p256_xmd_sha256_sswu
%xdefine EC_hash_to_curve_p384_xmd_sha384_sswu BORINGSSL_PREFIX %+ _EC_hash_to_curve_p384_xmd_sha384_sswu
%xdefine ED25519_check_public_key BORINGSSL_PREFIX %+ _ED25519_check_public_key
%xdefine ED25519_keypair BORINGSSL_PREFIX %+ _ED25519_keypair
%xdefine ED25519_keypair_from_seed BORINGSSL_PREFIX %+ _ED25519_keypair_from_seed
%xdefine ED25519_keypair_internal BORINGSSL_PREFIX %+ _ED25519_keypair_internal
%xdefine ED25519_sign BORINGSSL_PREFIX %+ _ED25519_sign
%xdefine ED25519_sign_no_self_test BORINGSSL_PREFIX %+ _ED25519_sign_no_self_test
%xdefine ED25519_verify BORINGSSL_PREFIX %+ _ED25519_verify
%xdefine ED25519_verify_no_self_test BORINGSSL_PREFIX %+ _ED25519_verify_no_self_test
%xdefine ED25519ctx_sign BORINGSSL_PREFIX %+ _ED25519ctx_sign
%xdefine ED25519ctx_sign_no_self_test BORINGSSL_PREFIX %+ _ED25519ctx_sign_no_self_test
%xdefine ED25519ctx_verify BORINGSSL_PREFIX %+ _ED25519ctx_verify
%xdefine ED25519ctx_verify_no_self_test BORINGSSL_PREFIX %+ _ED25519ctx_verify_no_self_test
%xdefine ED25519ph_sign BORINGSSL_PREFIX %+ _ED25519ph_sign
%xdefine ED25519ph_sign_digest BORINGSSL_PREFIX %+ _ED25519ph_sign_digest
%xdefine ED25519ph_sign_digest_no_self_test BORINGSSL_PREFIX %+ _ED25519ph_sign_digest_no_self_test
%xdefine ED25519ph_sign_no_self_test BORINGSSL_PREFIX %+ _ED25519ph_sign_no_self_test
%xdefine ED25519ph_verify BORINGSSL_PREFIX %+ _ED25519ph_verify
%xdefine ED25519ph_verify_digest BORINGSSL_PREFIX %+ _ED25519ph_verify_digest
%xdefine ED25519ph_verify_digest_no_self_test BORINGSSL_PREFIX %+ _ED25519ph_verify_digest_no_self_test
%xdefine ED25519ph_verify_no_self_test BORINGSSL_PREFIX %+ _ED25519ph_verify_no_self_test
%xdefine EDIPARTYNAME_free BORINGSSL_PREFIX %+ _EDIPARTYNAME_free
%xdefine EDIPARTYNAME_it BORINGSSL_PREFIX %+ _EDIPARTYNAME_it
%xdefine EDIPARTYNAME_new BORINGSSL_PREFIX %+ _EDIPARTYNAME_new
%xdefine ENGINE_cleanup BORINGSSL_PREFIX %+ _ENGINE_cleanup
%xdefine ENGINE_free BORINGSSL_PREFIX %+ _ENGINE_free
%xdefine ENGINE_get_EC BORINGSSL_PREFIX %+ _ENGINE_get_EC
%xdefine ENGINE_get_RSA BORINGSSL_PREFIX %+ _ENGINE_get_RSA
%xdefine ENGINE_load_builtin_engines BORINGSSL_PREFIX %+ _ENGINE_load_builtin_engines
%xdefine ENGINE_new BORINGSSL_PREFIX %+ _ENGINE_new
%xdefine ENGINE_register_all_ciphers BORINGSSL_PREFIX %+ _ENGINE_register_all_ciphers
%xdefine ENGINE_register_all_complete BORINGSSL_PREFIX %+ _ENGINE_register_all_complete
%xdefine ENGINE_register_all_digests BORINGSSL_PREFIX %+ _ENGINE_register_all_digests
%xdefine ENGINE_set_EC BORINGSSL_PREFIX %+ _ENGINE_set_EC
%xdefine ENGINE_set_RSA BORINGSSL_PREFIX %+ _ENGINE_set_RSA
%xdefine ERR_SAVE_STATE_free BORINGSSL_PREFIX %+ _ERR_SAVE_STATE_free
%xdefine ERR_add_error_data BORINGSSL_PREFIX %+ _ERR_add_error_data
%xdefine ERR_add_error_dataf BORINGSSL_PREFIX %+ _ERR_add_error_dataf
%xdefine ERR_clear_error BORINGSSL_PREFIX %+ _ERR_clear_error
%xdefine ERR_clear_system_error BORINGSSL_PREFIX %+ _ERR_clear_system_error
%xdefine ERR_error_string BORINGSSL_PREFIX %+ _ERR_error_string
%xdefine ERR_error_string_n BORINGSSL_PREFIX %+ _ERR_error_string_n
%xdefine ERR_free_strings BORINGSSL_PREFIX %+ _ERR_free_strings
%xdefine ERR_func_error_string BORINGSSL_PREFIX %+ _ERR_func_error_string
%xdefine ERR_get_error BORINGSSL_PREFIX %+ _ERR_get_error
%xdefine ERR_get_error_line BORINGSSL_PREFIX %+ _ERR_get_error_line
%xdefine ERR_get_error_line_data BORINGSSL_PREFIX %+ _ERR_get_error_line_data
%xdefine ERR_get_next_error_library BORINGSSL_PREFIX %+ _ERR_get_next_error_library
%xdefine ERR_lib_error_string BORINGSSL_PREFIX %+ _ERR_lib_error_string
%xdefine ERR_load_BIO_strings BORINGSSL_PREFIX %+ _ERR_load_BIO_strings
%xdefine ERR_load_CRYPTO_strings BORINGSSL_PREFIX %+ _ERR_load_CRYPTO_strings
%xdefine ERR_load_ERR_strings BORINGSSL_PREFIX %+ _ERR_load_ERR_strings
%xdefine ERR_load_RAND_strings BORINGSSL_PREFIX %+ _ERR_load_RAND_strings
%xdefine ERR_load_crypto_strings BORINGSSL_PREFIX %+ _ERR_load_crypto_strings
%xdefine ERR_peek_error BORINGSSL_PREFIX %+ _ERR_peek_error
%xdefine ERR_peek_error_line BORINGSSL_PREFIX %+ _ERR_peek_error_line
%xdefine ERR_peek_error_line_data BORINGSSL_PREFIX %+ _ERR_peek_error_line_data
%xdefine ERR_peek_last_error BORINGSSL_PREFIX %+ _ERR_peek_last_error
%xdefine ERR_peek_last_error_line BORINGSSL_PREFIX %+ _ERR_peek_last_error_line
%xdefine ERR_peek_last_error_line_data BORINGSSL_PREFIX %+ _ERR_peek_last_error_line_data
%xdefine ERR_pop_to_mark BORINGSSL_PREFIX %+ _ERR_pop_to_mark
%xdefine ERR_print_errors BORINGSSL_PREFIX %+ _ERR_print_errors
%xdefine ERR_print_errors_cb BORINGSSL_PREFIX %+ _ERR_print_errors_cb
%xdefine ERR_print_errors_fp BORINGSSL_PREFIX %+ _ERR_print_errors_fp
%xdefine ERR_put_error BORINGSSL_PREFIX %+ _ERR_put_error
%xdefine ERR_reason_error_string BORINGSSL_PREFIX %+ _ERR_reason_error_string
%xdefine ERR_remove_state BORINGSSL_PREFIX %+ _ERR_remove_state
%xdefine ERR_remove_thread_state BORINGSSL_PREFIX %+ _ERR_remove_thread_state
%xdefine ERR_restore_state BORINGSSL_PREFIX %+ _ERR_restore_state
%xdefine ERR_save_state BORINGSSL_PREFIX %+ _ERR_save_state
%xdefine ERR_set_error_data BORINGSSL_PREFIX %+ _ERR_set_error_data
%xdefine ERR_set_mark BORINGSSL_PREFIX %+ _ERR_set_mark
%xdefine EVP_AEAD_CTX_aead BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_aead
%xdefine EVP_AEAD_CTX_cleanup BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_cleanup
%xdefine EVP_AEAD_CTX_deserialize_state BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_deserialize_state
%xdefine EVP_AEAD_CTX_free BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_free
%xdefine EVP_AEAD_CTX_get_aead_id BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_get_aead_id
%xdefine EVP_AEAD_CTX_get_iv BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_get_iv
%xdefine EVP_AEAD_CTX_init BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_init
%xdefine EVP_AEAD_CTX_init_with_direction BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_init_with_direction
%xdefine EVP_AEAD_CTX_new BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_new
%xdefine EVP_AEAD_CTX_open BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_open
%xdefine EVP_AEAD_CTX_open_gather BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_open_gather
%xdefine EVP_AEAD_CTX_seal BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_seal
%xdefine EVP_AEAD_CTX_seal_scatter BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_seal_scatter
%xdefine EVP_AEAD_CTX_serialize_state BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_serialize_state
%xdefine EVP_AEAD_CTX_tag_len BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_tag_len
%xdefine EVP_AEAD_CTX_zero BORINGSSL_PREFIX %+ _EVP_AEAD_CTX_zero
%xdefine EVP_AEAD_get_iv_from_ipv4_nanosecs BORINGSSL_PREFIX %+ _EVP_AEAD_get_iv_from_ipv4_nanosecs
%xdefine EVP_AEAD_key_length BORINGSSL_PREFIX %+ _EVP_AEAD_key_length
%xdefine EVP_AEAD_max_overhead BORINGSSL_PREFIX %+ _EVP_AEAD_max_overhead
%xdefine EVP_AEAD_max_tag_len BORINGSSL_PREFIX %+ _EVP_AEAD_max_tag_len
%xdefine EVP_AEAD_nonce_length BORINGSSL_PREFIX %+ _EVP_AEAD_nonce_length
%xdefine EVP_BytesToKey BORINGSSL_PREFIX %+ _EVP_BytesToKey
%xdefine EVP_CIPHER_CTX_block_size BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_block_size
%xdefine EVP_CIPHER_CTX_cipher BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_cipher
%xdefine EVP_CIPHER_CTX_cleanup BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_cleanup
%xdefine EVP_CIPHER_CTX_copy BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_copy
%xdefine EVP_CIPHER_CTX_ctrl BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_ctrl
%xdefine EVP_CIPHER_CTX_encrypting BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_encrypting
%xdefine EVP_CIPHER_CTX_flags BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_flags
%xdefine EVP_CIPHER_CTX_free BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_free
%xdefine EVP_CIPHER_CTX_get_app_data BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_get_app_data
%xdefine EVP_CIPHER_CTX_init BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_init
%xdefine EVP_CIPHER_CTX_iv_length BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_iv_length
%xdefine EVP_CIPHER_CTX_key_length BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_key_length
%xdefine EVP_CIPHER_CTX_mode BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_mode
%xdefine EVP_CIPHER_CTX_new BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_new
%xdefine EVP_CIPHER_CTX_nid BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_nid
%xdefine EVP_CIPHER_CTX_reset BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_reset
%xdefine EVP_CIPHER_CTX_set_app_data BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_app_data
%xdefine EVP_CIPHER_CTX_set_flags BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_flags
%xdefine EVP_CIPHER_CTX_set_key_length BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_key_length
%xdefine EVP_CIPHER_CTX_set_padding BORINGSSL_PREFIX %+ _EVP_CIPHER_CTX_set_padding
%xdefine EVP_CIPHER_block_size BORINGSSL_PREFIX %+ _EVP_CIPHER_block_size
%xdefine EVP_CIPHER_do_all_sorted BORINGSSL_PREFIX %+ _EVP_CIPHER_do_all_sorted
%xdefine EVP_CIPHER_flags BORINGSSL_PREFIX %+ _EVP_CIPHER_flags
%xdefine EVP_CIPHER_iv_length BORINGSSL_PREFIX %+ _EVP_CIPHER_iv_length
%xdefine EVP_CIPHER_key_length BORINGSSL_PREFIX %+ _EVP_CIPHER_key_length
%xdefine EVP_CIPHER_mode BORINGSSL_PREFIX %+ _EVP_CIPHER_mode
%xdefine EVP_CIPHER_name BORINGSSL_PREFIX %+ _EVP_CIPHER_name
%xdefine EVP_CIPHER_nid BORINGSSL_PREFIX %+ _EVP_CIPHER_nid
%xdefine EVP_Cipher BORINGSSL_PREFIX %+ _EVP_Cipher
%xdefine EVP_CipherFinal BORINGSSL_PREFIX %+ _EVP_CipherFinal
%xdefine EVP_CipherFinal_ex BORINGSSL_PREFIX %+ _EVP_CipherFinal_ex
%xdefine EVP_CipherInit BORINGSSL_PREFIX %+ _EVP_CipherInit
%xdefine EVP_CipherInit_ex BORINGSSL_PREFIX %+ _EVP_CipherInit_ex
%xdefine EVP_CipherUpdate BORINGSSL_PREFIX %+ _EVP_CipherUpdate
%xdefine EVP_DecodeBase64 BORINGSSL_PREFIX %+ _EVP_DecodeBase64
%xdefine EVP_DecodeBlock BORINGSSL_PREFIX %+ _EVP_DecodeBlock
%xdefine EVP_DecodeFinal BORINGSSL_PREFIX %+ _EVP_DecodeFinal
%xdefine EVP_DecodeInit BORINGSSL_PREFIX %+ _EVP_DecodeInit
%xdefine EVP_DecodeUpdate BORINGSSL_PREFIX %+ _EVP_DecodeUpdate
%xdefine EVP_DecodedLength BORINGSSL_PREFIX %+ _EVP_DecodedLength
%xdefine EVP_DecryptFinal BORINGSSL_PREFIX %+ _EVP_DecryptFinal
%xdefine EVP_DecryptFinal_ex BORINGSSL_PREFIX %+ _EVP_DecryptFinal_ex
%xdefine EVP_DecryptInit BORINGSSL_PREFIX %+ _EVP_DecryptInit
%xdefine EVP_DecryptInit_ex BORINGSSL_PREFIX %+ _EVP_DecryptInit_ex
%xdefine EVP_DecryptUpdate BORINGSSL_PREFIX %+ _EVP_DecryptUpdate
%xdefine EVP_Digest BORINGSSL_PREFIX %+ _EVP_Digest
%xdefine EVP_DigestFinal BORINGSSL_PREFIX %+ _EVP_DigestFinal
%xdefine EVP_DigestFinalXOF BORINGSSL_PREFIX %+ _EVP_DigestFinalXOF
%xdefine EVP_DigestFinal_ex BORINGSSL_PREFIX %+ _EVP_DigestFinal_ex
%xdefine EVP_DigestInit BORINGSSL_PREFIX %+ _EVP_DigestInit
%xdefine EVP_DigestInit_ex BORINGSSL_PREFIX %+ _EVP_DigestInit_ex
%xdefine EVP_DigestSign BORINGSSL_PREFIX %+ _EVP_DigestSign
%xdefine EVP_DigestSignFinal BORINGSSL_PREFIX %+ _EVP_DigestSignFinal
%xdefine EVP_DigestSignInit BORINGSSL_PREFIX %+ _EVP_DigestSignInit
%xdefine EVP_DigestSignUpdate BORINGSSL_PREFIX %+ _EVP_DigestSignUpdate
%xdefine EVP_DigestSqueeze BORINGSSL_PREFIX %+ _EVP_DigestSqueeze
%xdefine EVP_DigestUpdate BORINGSSL_PREFIX %+ _EVP_DigestUpdate
%xdefine EVP_DigestVerify BORINGSSL_PREFIX %+ _EVP_DigestVerify
%xdefine EVP_DigestVerifyFinal BORINGSSL_PREFIX %+ _EVP_DigestVerifyFinal
%xdefine EVP_DigestVerifyInit BORINGSSL_PREFIX %+ _EVP_DigestVerifyInit
%xdefine EVP_DigestVerifyUpdate BORINGSSL_PREFIX %+ _EVP_DigestVerifyUpdate
%xdefine EVP_ENCODE_CTX_free BORINGSSL_PREFIX %+ _EVP_ENCODE_CTX_free
%xdefine EVP_ENCODE_CTX_new BORINGSSL_PREFIX %+ _EVP_ENCODE_CTX_new
%xdefine EVP_EncodeBlock BORINGSSL_PREFIX %+ _EVP_EncodeBlock
%xdefine EVP_EncodeFinal BORINGSSL_PREFIX %+ _EVP_EncodeFinal
%xdefine EVP_EncodeInit BORINGSSL_PREFIX %+ _EVP_EncodeInit
%xdefine EVP_EncodeUpdate BORINGSSL_PREFIX %+ _EVP_EncodeUpdate
%xdefine EVP_EncodedLength BORINGSSL_PREFIX %+ _EVP_EncodedLength
%xdefine EVP_EncryptFinal BORINGSSL_PREFIX %+ _EVP_EncryptFinal
%xdefine EVP_EncryptFinal_ex BORINGSSL_PREFIX %+ _EVP_EncryptFinal_ex
%xdefine EVP_EncryptInit BORINGSSL_PREFIX %+ _EVP_EncryptInit
%xdefine EVP_EncryptInit_ex BORINGSSL_PREFIX %+ _EVP_EncryptInit_ex
%xdefine EVP_EncryptUpdate BORINGSSL_PREFIX %+ _EVP_EncryptUpdate
%xdefine EVP_HPKE_AEAD_aead BORINGSSL_PREFIX %+ _EVP_HPKE_AEAD_aead
%xdefine EVP_HPKE_AEAD_id BORINGSSL_PREFIX %+ _EVP_HPKE_AEAD_id
%xdefine EVP_HPKE_CTX_aead BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_aead
%xdefine EVP_HPKE_CTX_cleanup BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_cleanup
%xdefine EVP_HPKE_CTX_export BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_export
%xdefine EVP_HPKE_CTX_free BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_free
%xdefine EVP_HPKE_CTX_kdf BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_kdf
%xdefine EVP_HPKE_CTX_kem BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_kem
%xdefine EVP_HPKE_CTX_max_overhead BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_max_overhead
%xdefine EVP_HPKE_CTX_new BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_new
%xdefine EVP_HPKE_CTX_open BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_open
%xdefine EVP_HPKE_CTX_seal BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_seal
%xdefine EVP_HPKE_CTX_setup_auth_recipient BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_recipient
%xdefine EVP_HPKE_CTX_setup_auth_sender BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_sender
%xdefine EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_auth_sender_with_seed_for_testing
%xdefine EVP_HPKE_CTX_setup_recipient BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_recipient
%xdefine EVP_HPKE_CTX_setup_sender BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_sender
%xdefine EVP_HPKE_CTX_setup_sender_with_seed_for_testing BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_setup_sender_with_seed_for_testing
%xdefine EVP_HPKE_CTX_zero BORINGSSL_PREFIX %+ _EVP_HPKE_CTX_zero
%xdefine EVP_HPKE_KDF_hkdf_md BORINGSSL_PREFIX %+ _EVP_HPKE_KDF_hkdf_md
%xdefine EVP_HPKE_KDF_id BORINGSSL_PREFIX %+ _EVP_HPKE_KDF_id
%xdefine EVP_HPKE_KEM_enc_len BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_enc_len
%xdefine EVP_HPKE_KEM_id BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_id
%xdefine EVP_HPKE_KEM_private_key_len BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_private_key_len
%xdefine EVP_HPKE_KEM_public_key_len BORINGSSL_PREFIX %+ _EVP_HPKE_KEM_public_key_len
%xdefine EVP_HPKE_KEY_cleanup BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_cleanup
%xdefine EVP_HPKE_KEY_copy BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_copy
%xdefine EVP_HPKE_KEY_free BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_free
%xdefine EVP_HPKE_KEY_generate BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_generate
%xdefine EVP_HPKE_KEY_init BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_init
%xdefine EVP_HPKE_KEY_kem BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_kem
%xdefine EVP_HPKE_KEY_move BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_move
%xdefine EVP_HPKE_KEY_new BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_new
%xdefine EVP_HPKE_KEY_private_key BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_private_key
%xdefine EVP_HPKE_KEY_public_key BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_public_key
%xdefine EVP_HPKE_KEY_zero BORINGSSL_PREFIX %+ _EVP_HPKE_KEY_zero
%xdefine EVP_MD_CTX_block_size BORINGSSL_PREFIX %+ _EVP_MD_CTX_block_size
%xdefine EVP_MD_CTX_cleanse BORINGSSL_PREFIX %+ _EVP_MD_CTX_cleanse
%xdefine EVP_MD_CTX_cleanup BORINGSSL_PREFIX %+ _EVP_MD_CTX_cleanup
%xdefine EVP_MD_CTX_copy BORINGSSL_PREFIX %+ _EVP_MD_CTX_copy
%xdefine EVP_MD_CTX_copy_ex BORINGSSL_PREFIX %+ _EVP_MD_CTX_copy_ex
%xdefine EVP_MD_CTX_create BORINGSSL_PREFIX %+ _EVP_MD_CTX_create
%xdefine EVP_MD_CTX_destroy BORINGSSL_PREFIX %+ _EVP_MD_CTX_destroy
%xdefine EVP_MD_CTX_free BORINGSSL_PREFIX %+ _EVP_MD_CTX_free
%xdefine EVP_MD_CTX_get_pkey_ctx BORINGSSL_PREFIX %+ _EVP_MD_CTX_get_pkey_ctx
%xdefine EVP_MD_CTX_init BORINGSSL_PREFIX %+ _EVP_MD_CTX_init
%xdefine EVP_MD_CTX_md BORINGSSL_PREFIX %+ _EVP_MD_CTX_md
%xdefine EVP_MD_CTX_move BORINGSSL_PREFIX %+ _EVP_MD_CTX_move
%xdefine EVP_MD_CTX_new BORINGSSL_PREFIX %+ _EVP_MD_CTX_new
%xdefine EVP_MD_CTX_pkey_ctx BORINGSSL_PREFIX %+ _EVP_MD_CTX_pkey_ctx
%xdefine EVP_MD_CTX_reset BORINGSSL_PREFIX %+ _EVP_MD_CTX_reset
%xdefine EVP_MD_CTX_set_flags BORINGSSL_PREFIX %+ _EVP_MD_CTX_set_flags
%xdefine EVP_MD_CTX_set_pkey_ctx BORINGSSL_PREFIX %+ _EVP_MD_CTX_set_pkey_ctx
%xdefine EVP_MD_CTX_size BORINGSSL_PREFIX %+ _EVP_MD_CTX_size
%xdefine EVP_MD_CTX_type BORINGSSL_PREFIX %+ _EVP_MD_CTX_type
%xdefine EVP_MD_block_size BORINGSSL_PREFIX %+ _EVP_MD_block_size
%xdefine EVP_MD_do_all BORINGSSL_PREFIX %+ _EVP_MD_do_all
%xdefine EVP_MD_do_all_sorted BORINGSSL_PREFIX %+ _EVP_MD_do_all_sorted
%xdefine EVP_MD_flags BORINGSSL_PREFIX %+ _EVP_MD_flags
%xdefine EVP_MD_get0_name BORINGSSL_PREFIX %+ _EVP_MD_get0_name
%xdefine EVP_MD_get_pkey_type BORINGSSL_PREFIX %+ _EVP_MD_get_pkey_type
%xdefine EVP_MD_meth_get_flags BORINGSSL_PREFIX %+ _EVP_MD_meth_get_flags
%xdefine EVP_MD_name BORINGSSL_PREFIX %+ _EVP_MD_name
%xdefine EVP_MD_nid BORINGSSL_PREFIX %+ _EVP_MD_nid
%xdefine EVP_MD_pkey_type BORINGSSL_PREFIX %+ _EVP_MD_pkey_type
%xdefine EVP_MD_size BORINGSSL_PREFIX %+ _EVP_MD_size
%xdefine EVP_MD_type BORINGSSL_PREFIX %+ _EVP_MD_type
%xdefine EVP_MD_unstable_sha3_enable BORINGSSL_PREFIX %+ _EVP_MD_unstable_sha3_enable
%xdefine EVP_MD_unstable_sha3_is_enabled BORINGSSL_PREFIX %+ _EVP_MD_unstable_sha3_is_enabled
%xdefine EVP_PBE_scrypt BORINGSSL_PREFIX %+ _EVP_PBE_scrypt
%xdefine EVP_PKCS82PKEY BORINGSSL_PREFIX %+ _EVP_PKCS82PKEY
%xdefine EVP_PKEY2PKCS8 BORINGSSL_PREFIX %+ _EVP_PKEY2PKCS8
%xdefine EVP_PKEY_CTX_add1_hkdf_info BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_add1_hkdf_info
%xdefine EVP_PKEY_CTX_ctrl BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_ctrl
%xdefine EVP_PKEY_CTX_ctrl_str BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_ctrl_str
%xdefine EVP_PKEY_CTX_dup BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_dup
%xdefine EVP_PKEY_CTX_free BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_free
%xdefine EVP_PKEY_CTX_get0_pkey BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_pkey
%xdefine EVP_PKEY_CTX_get0_rsa_oaep_label BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_rsa_oaep_label
%xdefine EVP_PKEY_CTX_get0_signature_context BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get0_signature_context
%xdefine EVP_PKEY_CTX_get_app_data BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_app_data
%xdefine EVP_PKEY_CTX_get_keygen_info BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_keygen_info
%xdefine EVP_PKEY_CTX_get_rsa_mgf1_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_mgf1_md
%xdefine EVP_PKEY_CTX_get_rsa_oaep_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_oaep_md
%xdefine EVP_PKEY_CTX_get_rsa_padding BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_padding
%xdefine EVP_PKEY_CTX_get_rsa_pss_saltlen BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_rsa_pss_saltlen
%xdefine EVP_PKEY_CTX_get_signature_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_get_signature_md
%xdefine EVP_PKEY_CTX_hkdf_mode BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_hkdf_mode
%xdefine EVP_PKEY_CTX_kem_set_params BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_kem_set_params
%xdefine EVP_PKEY_CTX_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_md
%xdefine EVP_PKEY_CTX_new BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_new
%xdefine EVP_PKEY_CTX_new_id BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_new_id
%xdefine EVP_PKEY_CTX_pqdsa_set_params BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_pqdsa_set_params
%xdefine EVP_PKEY_CTX_set0_rsa_oaep_label BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set0_rsa_oaep_label
%xdefine EVP_PKEY_CTX_set1_hkdf_key BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set1_hkdf_key
%xdefine EVP_PKEY_CTX_set1_hkdf_salt BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set1_hkdf_salt
%xdefine EVP_PKEY_CTX_set_app_data BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_app_data
%xdefine EVP_PKEY_CTX_set_cb BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_cb
%xdefine EVP_PKEY_CTX_set_dh_pad BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_pad
%xdefine EVP_PKEY_CTX_set_dh_paramgen_generator BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_paramgen_generator
%xdefine EVP_PKEY_CTX_set_dh_paramgen_prime_len BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dh_paramgen_prime_len
%xdefine EVP_PKEY_CTX_set_dsa_paramgen_bits BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_bits
%xdefine EVP_PKEY_CTX_set_dsa_paramgen_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_md
%xdefine EVP_PKEY_CTX_set_dsa_paramgen_q_bits BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_dsa_paramgen_q_bits
%xdefine EVP_PKEY_CTX_set_ec_param_enc BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_ec_param_enc
%xdefine EVP_PKEY_CTX_set_ec_paramgen_curve_nid BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_ec_paramgen_curve_nid
%xdefine EVP_PKEY_CTX_set_hkdf_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_hkdf_md
%xdefine EVP_PKEY_CTX_set_rsa_keygen_bits BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_keygen_bits
%xdefine EVP_PKEY_CTX_set_rsa_keygen_pubexp BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_keygen_pubexp
%xdefine EVP_PKEY_CTX_set_rsa_mgf1_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_mgf1_md
%xdefine EVP_PKEY_CTX_set_rsa_oaep_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_oaep_md
%xdefine EVP_PKEY_CTX_set_rsa_padding BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_padding
%xdefine EVP_PKEY_CTX_set_rsa_pss_keygen_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_md
%xdefine EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_mgf1_md
%xdefine EVP_PKEY_CTX_set_rsa_pss_keygen_saltlen BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_keygen_saltlen
%xdefine EVP_PKEY_CTX_set_rsa_pss_saltlen BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_rsa_pss_saltlen
%xdefine EVP_PKEY_CTX_set_signature_context BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_signature_context
%xdefine EVP_PKEY_CTX_set_signature_md BORINGSSL_PREFIX %+ _EVP_PKEY_CTX_set_signature_md
%xdefine EVP_PKEY_asn1_find BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_find
%xdefine EVP_PKEY_asn1_find_str BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_find_str
%xdefine EVP_PKEY_asn1_get0 BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get0
%xdefine EVP_PKEY_asn1_get0_info BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get0_info
%xdefine EVP_PKEY_asn1_get_count BORINGSSL_PREFIX %+ _EVP_PKEY_asn1_get_count
%xdefine EVP_PKEY_assign BORINGSSL_PREFIX %+ _EVP_PKEY_assign
%xdefine EVP_PKEY_assign_DH BORINGSSL_PREFIX %+ _EVP_PKEY_assign_DH
%xdefine EVP_PKEY_assign_DSA BORINGSSL_PREFIX %+ _EVP_PKEY_assign_DSA
%xdefine EVP_PKEY_assign_EC_KEY BORINGSSL_PREFIX %+ _EVP_PKEY_assign_EC_KEY
%xdefine EVP_PKEY_assign_RSA BORINGSSL_PREFIX %+ _EVP_PKEY_assign_RSA
%xdefine EVP_PKEY_base_id BORINGSSL_PREFIX %+ _EVP_PKEY_base_id
%xdefine EVP_PKEY_bits BORINGSSL_PREFIX %+ _EVP_PKEY_bits
%xdefine EVP_PKEY_check BORINGSSL_PREFIX %+ _EVP_PKEY_check
%xdefine EVP_PKEY_cmp BORINGSSL_PREFIX %+ _EVP_PKEY_cmp
%xdefine EVP_PKEY_cmp_parameters BORINGSSL_PREFIX %+ _EVP_PKEY_cmp_parameters
%xdefine EVP_PKEY_copy_parameters BORINGSSL_PREFIX %+ _EVP_PKEY_copy_parameters
%xdefine EVP_PKEY_decapsulate BORINGSSL_PREFIX %+ _EVP_PKEY_decapsulate
%xdefine EVP_PKEY_decrypt BORINGSSL_PREFIX %+ _EVP_PKEY_decrypt
%xdefine EVP_PKEY_decrypt_init BORINGSSL_PREFIX %+ _EVP_PKEY_decrypt_init
%xdefine EVP_PKEY_derive BORINGSSL_PREFIX %+ _EVP_PKEY_derive
%xdefine EVP_PKEY_derive_init BORINGSSL_PREFIX %+ _EVP_PKEY_derive_init
%xdefine EVP_PKEY_derive_set_peer BORINGSSL_PREFIX %+ _EVP_PKEY_derive_set_peer
%xdefine EVP_PKEY_ec_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_ec_pkey_meth
%xdefine EVP_PKEY_ed25519_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_ed25519_pkey_meth
%xdefine EVP_PKEY_ed25519ph_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_ed25519ph_pkey_meth
%xdefine EVP_PKEY_encapsulate BORINGSSL_PREFIX %+ _EVP_PKEY_encapsulate
%xdefine EVP_PKEY_encapsulate_deterministic BORINGSSL_PREFIX %+ _EVP_PKEY_encapsulate_deterministic
%xdefine EVP_PKEY_encrypt BORINGSSL_PREFIX %+ _EVP_PKEY_encrypt
%xdefine EVP_PKEY_encrypt_init BORINGSSL_PREFIX %+ _EVP_PKEY_encrypt_init
%xdefine EVP_PKEY_free BORINGSSL_PREFIX %+ _EVP_PKEY_free
%xdefine EVP_PKEY_get0 BORINGSSL_PREFIX %+ _EVP_PKEY_get0
%xdefine EVP_PKEY_get0_DH BORINGSSL_PREFIX %+ _EVP_PKEY_get0_DH
%xdefine EVP_PKEY_get0_DSA BORINGSSL_PREFIX %+ _EVP_PKEY_get0_DSA
%xdefine EVP_PKEY_get0_EC_KEY BORINGSSL_PREFIX %+ _EVP_PKEY_get0_EC_KEY
%xdefine EVP_PKEY_get0_RSA BORINGSSL_PREFIX %+ _EVP_PKEY_get0_RSA
%xdefine EVP_PKEY_get1_DH BORINGSSL_PREFIX %+ _EVP_PKEY_get1_DH
%xdefine EVP_PKEY_get1_DSA BORINGSSL_PREFIX %+ _EVP_PKEY_get1_DSA
%xdefine EVP_PKEY_get1_EC_KEY BORINGSSL_PREFIX %+ _EVP_PKEY_get1_EC_KEY
%xdefine EVP_PKEY_get1_RSA BORINGSSL_PREFIX %+ _EVP_PKEY_get1_RSA
%xdefine EVP_PKEY_get1_tls_encodedpoint BORINGSSL_PREFIX %+ _EVP_PKEY_get1_tls_encodedpoint
%xdefine EVP_PKEY_get_raw_private_key BORINGSSL_PREFIX %+ _EVP_PKEY_get_raw_private_key
%xdefine EVP_PKEY_get_raw_public_key BORINGSSL_PREFIX %+ _EVP_PKEY_get_raw_public_key
%xdefine EVP_PKEY_hkdf_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_hkdf_pkey_meth
%xdefine EVP_PKEY_hmac_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_hmac_pkey_meth
%xdefine EVP_PKEY_id BORINGSSL_PREFIX %+ _EVP_PKEY_id
%xdefine EVP_PKEY_is_opaque BORINGSSL_PREFIX %+ _EVP_PKEY_is_opaque
%xdefine EVP_PKEY_kem_check_key BORINGSSL_PREFIX %+ _EVP_PKEY_kem_check_key
%xdefine EVP_PKEY_kem_new_raw_key BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_key
%xdefine EVP_PKEY_kem_new_raw_public_key BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_public_key
%xdefine EVP_PKEY_kem_new_raw_secret_key BORINGSSL_PREFIX %+ _EVP_PKEY_kem_new_raw_secret_key
%xdefine EVP_PKEY_kem_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_kem_pkey_meth
%xdefine EVP_PKEY_kem_set_params BORINGSSL_PREFIX %+ _EVP_PKEY_kem_set_params
%xdefine EVP_PKEY_keygen BORINGSSL_PREFIX %+ _EVP_PKEY_keygen
%xdefine EVP_PKEY_keygen_deterministic BORINGSSL_PREFIX %+ _EVP_PKEY_keygen_deterministic
%xdefine EVP_PKEY_keygen_init BORINGSSL_PREFIX %+ _EVP_PKEY_keygen_init
%xdefine EVP_PKEY_missing_parameters BORINGSSL_PREFIX %+ _EVP_PKEY_missing_parameters
%xdefine EVP_PKEY_new BORINGSSL_PREFIX %+ _EVP_PKEY_new
%xdefine EVP_PKEY_new_mac_key BORINGSSL_PREFIX %+ _EVP_PKEY_new_mac_key
%xdefine EVP_PKEY_new_raw_private_key BORINGSSL_PREFIX %+ _EVP_PKEY_new_raw_private_key
%xdefine EVP_PKEY_new_raw_public_key BORINGSSL_PREFIX %+ _EVP_PKEY_new_raw_public_key
%xdefine EVP_PKEY_param_check BORINGSSL_PREFIX %+ _EVP_PKEY_param_check
%xdefine EVP_PKEY_paramgen BORINGSSL_PREFIX %+ _EVP_PKEY_paramgen
%xdefine EVP_PKEY_paramgen_init BORINGSSL_PREFIX %+ _EVP_PKEY_paramgen_init
%xdefine EVP_PKEY_pqdsa_new_raw_private_key BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_new_raw_private_key
%xdefine EVP_PKEY_pqdsa_new_raw_public_key BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_new_raw_public_key
%xdefine EVP_PKEY_pqdsa_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_pkey_meth
%xdefine EVP_PKEY_pqdsa_set_params BORINGSSL_PREFIX %+ _EVP_PKEY_pqdsa_set_params
%xdefine EVP_PKEY_print_params BORINGSSL_PREFIX %+ _EVP_PKEY_print_params
%xdefine EVP_PKEY_print_private BORINGSSL_PREFIX %+ _EVP_PKEY_print_private
%xdefine EVP_PKEY_print_public BORINGSSL_PREFIX %+ _EVP_PKEY_print_public
%xdefine EVP_PKEY_public_check BORINGSSL_PREFIX %+ _EVP_PKEY_public_check
%xdefine EVP_PKEY_rsa_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_rsa_pkey_meth
%xdefine EVP_PKEY_rsa_pss_pkey_meth BORINGSSL_PREFIX %+ _EVP_PKEY_rsa_pss_pkey_meth
%xdefine EVP_PKEY_set1_DH BORINGSSL_PREFIX %+ _EVP_PKEY_set1_DH
%xdefine EVP_PKEY_set1_DSA BORINGSSL_PREFIX %+ _EVP_PKEY_set1_DSA
%xdefine EVP_PKEY_set1_EC_KEY BORINGSSL_PREFIX %+ _EVP_PKEY_set1_EC_KEY
%xdefine EVP_PKEY_set1_RSA BORINGSSL_PREFIX %+ _EVP_PKEY_set1_RSA
%xdefine EVP_PKEY_set1_tls_encodedpoint BORINGSSL_PREFIX %+ _EVP_PKEY_set1_tls_encodedpoint
%xdefine EVP_PKEY_set_type BORINGSSL_PREFIX %+ _EVP_PKEY_set_type
%xdefine EVP_PKEY_set_type_str BORINGSSL_PREFIX %+ _EVP_PKEY_set_type_str
%xdefine EVP_PKEY_sign BORINGSSL_PREFIX %+ _EVP_PKEY_sign
%xdefine EVP_PKEY_sign_init BORINGSSL_PREFIX %+ _EVP_PKEY_sign_init
%xdefine EVP_PKEY_size BORINGSSL_PREFIX %+ _EVP_PKEY_size
%xdefine EVP_PKEY_type BORINGSSL_PREFIX %+ _EVP_PKEY_type
%xdefine EVP_PKEY_up_ref BORINGSSL_PREFIX %+ _EVP_PKEY_up_ref
%xdefine EVP_PKEY_verify BORINGSSL_PREFIX %+ _EVP_PKEY_verify
%xdefine EVP_PKEY_verify_init BORINGSSL_PREFIX %+ _EVP_PKEY_verify_init
%xdefine EVP_PKEY_verify_recover BORINGSSL_PREFIX %+ _EVP_PKEY_verify_recover
%xdefine EVP_PKEY_verify_recover_init BORINGSSL_PREFIX %+ _EVP_PKEY_verify_recover_init
%xdefine EVP_RSA_PKEY_CTX_ctrl BORINGSSL_PREFIX %+ _EVP_RSA_PKEY_CTX_ctrl
%xdefine EVP_SignFinal BORINGSSL_PREFIX %+ _EVP_SignFinal
%xdefine EVP_SignInit BORINGSSL_PREFIX %+ _EVP_SignInit
%xdefine EVP_SignInit_ex BORINGSSL_PREFIX %+ _EVP_SignInit_ex
%xdefine EVP_SignUpdate BORINGSSL_PREFIX %+ _EVP_SignUpdate
%xdefine EVP_VerifyFinal BORINGSSL_PREFIX %+ _EVP_VerifyFinal
%xdefine EVP_VerifyInit BORINGSSL_PREFIX %+ _EVP_VerifyInit
%xdefine EVP_VerifyInit_ex BORINGSSL_PREFIX %+ _EVP_VerifyInit_ex
%xdefine EVP_VerifyUpdate BORINGSSL_PREFIX %+ _EVP_VerifyUpdate
%xdefine EVP_add_cipher_alias BORINGSSL_PREFIX %+ _EVP_add_cipher_alias
%xdefine EVP_add_digest BORINGSSL_PREFIX %+ _EVP_add_digest
%xdefine EVP_aead_aes_128_cbc_sha1_tls BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha1_tls
%xdefine EVP_aead_aes_128_cbc_sha1_tls_implicit_iv BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha1_tls_implicit_iv
%xdefine EVP_aead_aes_128_cbc_sha256_tls BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha256_tls
%xdefine EVP_aead_aes_128_cbc_sha256_tls_implicit_iv BORINGSSL_PREFIX %+ _EVP_aead_aes_128_cbc_sha256_tls_implicit_iv
%xdefine EVP_aead_aes_128_ccm_bluetooth BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_bluetooth
%xdefine EVP_aead_aes_128_ccm_bluetooth_8 BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_bluetooth_8
%xdefine EVP_aead_aes_128_ccm_matter BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ccm_matter
%xdefine EVP_aead_aes_128_ctr_hmac_sha256 BORINGSSL_PREFIX %+ _EVP_aead_aes_128_ctr_hmac_sha256
%xdefine EVP_aead_aes_128_gcm BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm
%xdefine EVP_aead_aes_128_gcm_randnonce BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_randnonce
%xdefine EVP_aead_aes_128_gcm_siv BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_siv
%xdefine EVP_aead_aes_128_gcm_tls12 BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_tls12
%xdefine EVP_aead_aes_128_gcm_tls13 BORINGSSL_PREFIX %+ _EVP_aead_aes_128_gcm_tls13
%xdefine EVP_aead_aes_192_gcm BORINGSSL_PREFIX %+ _EVP_aead_aes_192_gcm
%xdefine EVP_aead_aes_256_cbc_sha1_tls BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha1_tls
%xdefine EVP_aead_aes_256_cbc_sha1_tls_implicit_iv BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha1_tls_implicit_iv
%xdefine EVP_aead_aes_256_cbc_sha384_tls BORINGSSL_PREFIX %+ _EVP_aead_aes_256_cbc_sha384_tls
%xdefine EVP_aead_aes_256_ctr_hmac_sha256 BORINGSSL_PREFIX %+ _EVP_aead_aes_256_ctr_hmac_sha256
%xdefine EVP_aead_aes_256_gcm BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm
%xdefine EVP_aead_aes_256_gcm_randnonce BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_randnonce
%xdefine EVP_aead_aes_256_gcm_siv BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_siv
%xdefine EVP_aead_aes_256_gcm_tls12 BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_tls12
%xdefine EVP_aead_aes_256_gcm_tls13 BORINGSSL_PREFIX %+ _EVP_aead_aes_256_gcm_tls13
%xdefine EVP_aead_chacha20_poly1305 BORINGSSL_PREFIX %+ _EVP_aead_chacha20_poly1305
%xdefine EVP_aead_des_ede3_cbc_sha1_tls BORINGSSL_PREFIX %+ _EVP_aead_des_ede3_cbc_sha1_tls
%xdefine EVP_aead_des_ede3_cbc_sha1_tls_implicit_iv BORINGSSL_PREFIX %+ _EVP_aead_des_ede3_cbc_sha1_tls_implicit_iv
%xdefine EVP_aead_null_sha1_tls BORINGSSL_PREFIX %+ _EVP_aead_null_sha1_tls
%xdefine EVP_aead_xchacha20_poly1305 BORINGSSL_PREFIX %+ _EVP_aead_xchacha20_poly1305
%xdefine EVP_aes_128_cbc BORINGSSL_PREFIX %+ _EVP_aes_128_cbc
%xdefine EVP_aes_128_cbc_hmac_sha1 BORINGSSL_PREFIX %+ _EVP_aes_128_cbc_hmac_sha1
%xdefine EVP_aes_128_cbc_hmac_sha256 BORINGSSL_PREFIX %+ _EVP_aes_128_cbc_hmac_sha256
%xdefine EVP_aes_128_ccm BORINGSSL_PREFIX %+ _EVP_aes_128_ccm
%xdefine EVP_aes_128_cfb BORINGSSL_PREFIX %+ _EVP_aes_128_cfb
%xdefine EVP_aes_128_cfb1 BORINGSSL_PREFIX %+ _EVP_aes_128_cfb1
%xdefine EVP_aes_128_cfb128 BORINGSSL_PREFIX %+ _EVP_aes_128_cfb128
%xdefine EVP_aes_128_cfb8 BORINGSSL_PREFIX %+ _EVP_aes_128_cfb8
%xdefine EVP_aes_128_ctr BORINGSSL_PREFIX %+ _EVP_aes_128_ctr
%xdefine EVP_aes_128_ecb BORINGSSL_PREFIX %+ _EVP_aes_128_ecb
%xdefine EVP_aes_128_gcm BORINGSSL_PREFIX %+ _EVP_aes_128_gcm
%xdefine EVP_aes_128_ofb BORINGSSL_PREFIX %+ _EVP_aes_128_ofb
%xdefine EVP_aes_192_cbc BORINGSSL_PREFIX %+ _EVP_aes_192_cbc
%xdefine EVP_aes_192_ccm BORINGSSL_PREFIX %+ _EVP_aes_192_ccm
%xdefine EVP_aes_192_cfb BORINGSSL_PREFIX %+ _EVP_aes_192_cfb
%xdefine EVP_aes_192_cfb1 BORINGSSL_PREFIX %+ _EVP_aes_192_cfb1
%xdefine EVP_aes_192_cfb128 BORINGSSL_PREFIX %+ _EVP_aes_192_cfb128
%xdefine EVP_aes_192_cfb8 BORINGSSL_PREFIX %+ _EVP_aes_192_cfb8
%xdefine EVP_aes_192_ctr BORINGSSL_PREFIX %+ _EVP_aes_192_ctr
%xdefine EVP_aes_192_ecb BORINGSSL_PREFIX %+ _EVP_aes_192_ecb
%xdefine EVP_aes_192_gcm BORINGSSL_PREFIX %+ _EVP_aes_192_gcm
%xdefine EVP_aes_192_ofb BORINGSSL_PREFIX %+ _EVP_aes_192_ofb
%xdefine EVP_aes_256_cbc BORINGSSL_PREFIX %+ _EVP_aes_256_cbc
%xdefine EVP_aes_256_cbc_hmac_sha1 BORINGSSL_PREFIX %+ _EVP_aes_256_cbc_hmac_sha1
%xdefine EVP_aes_256_cbc_hmac_sha256 BORINGSSL_PREFIX %+ _EVP_aes_256_cbc_hmac_sha256
%xdefine EVP_aes_256_ccm BORINGSSL_PREFIX %+ _EVP_aes_256_ccm
%xdefine EVP_aes_256_cfb BORINGSSL_PREFIX %+ _EVP_aes_256_cfb
%xdefine EVP_aes_256_cfb1 BORINGSSL_PREFIX %+ _EVP_aes_256_cfb1
%xdefine EVP_aes_256_cfb128 BORINGSSL_PREFIX %+ _EVP_aes_256_cfb128
%xdefine EVP_aes_256_cfb8 BORINGSSL_PREFIX %+ _EVP_aes_256_cfb8
%xdefine EVP_aes_256_ctr BORINGSSL_PREFIX %+ _EVP_aes_256_ctr
%xdefine EVP_aes_256_ecb BORINGSSL_PREFIX %+ _EVP_aes_256_ecb
%xdefine EVP_aes_256_gcm BORINGSSL_PREFIX %+ _EVP_aes_256_gcm
%xdefine EVP_aes_256_ofb BORINGSSL_PREFIX %+ _EVP_aes_256_ofb
%xdefine EVP_aes_256_wrap BORINGSSL_PREFIX %+ _EVP_aes_256_wrap
%xdefine EVP_aes_256_xts BORINGSSL_PREFIX %+ _EVP_aes_256_xts
%xdefine EVP_bf_cbc BORINGSSL_PREFIX %+ _EVP_bf_cbc
%xdefine EVP_bf_cfb BORINGSSL_PREFIX %+ _EVP_bf_cfb
%xdefine EVP_bf_cfb64 BORINGSSL_PREFIX %+ _EVP_bf_cfb64
%xdefine EVP_bf_ecb BORINGSSL_PREFIX %+ _EVP_bf_ecb
%xdefine EVP_bf_ofb BORINGSSL_PREFIX %+ _EVP_bf_ofb
%xdefine EVP_blake2b256 BORINGSSL_PREFIX %+ _EVP_blake2b256
%xdefine EVP_cast5_cbc BORINGSSL_PREFIX %+ _EVP_cast5_cbc
%xdefine EVP_cast5_ecb BORINGSSL_PREFIX %+ _EVP_cast5_ecb
%xdefine EVP_chacha20_poly1305 BORINGSSL_PREFIX %+ _EVP_chacha20_poly1305
%xdefine EVP_cleanup BORINGSSL_PREFIX %+ _EVP_cleanup
%xdefine EVP_des_cbc BORINGSSL_PREFIX %+ _EVP_des_cbc
%xdefine EVP_des_ecb BORINGSSL_PREFIX %+ _EVP_des_ecb
%xdefine EVP_des_ede BORINGSSL_PREFIX %+ _EVP_des_ede
%xdefine EVP_des_ede3 BORINGSSL_PREFIX %+ _EVP_des_ede3
%xdefine EVP_des_ede3_cbc BORINGSSL_PREFIX %+ _EVP_des_ede3_cbc
%xdefine EVP_des_ede3_ecb BORINGSSL_PREFIX %+ _EVP_des_ede3_ecb
%xdefine EVP_des_ede_cbc BORINGSSL_PREFIX %+ _EVP_des_ede_cbc
%xdefine EVP_enc_null BORINGSSL_PREFIX %+ _EVP_enc_null
%xdefine EVP_final_with_secret_suffix_sha1 BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha1
%xdefine EVP_final_with_secret_suffix_sha256 BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha256
%xdefine EVP_final_with_secret_suffix_sha384 BORINGSSL_PREFIX %+ _EVP_final_with_secret_suffix_sha384
%xdefine EVP_get_cipherbyname BORINGSSL_PREFIX %+ _EVP_get_cipherbyname
%xdefine EVP_get_cipherbynid BORINGSSL_PREFIX %+ _EVP_get_cipherbynid
%xdefine EVP_get_digestbyname BORINGSSL_PREFIX %+ _EVP_get_digestbyname
%xdefine EVP_get_digestbynid BORINGSSL_PREFIX %+ _EVP_get_digestbynid
%xdefine EVP_get_digestbyobj BORINGSSL_PREFIX %+ _EVP_get_digestbyobj
%xdefine EVP_get_pw_prompt BORINGSSL_PREFIX %+ _EVP_get_pw_prompt
%xdefine EVP_has_aes_hardware BORINGSSL_PREFIX %+ _EVP_has_aes_hardware
%xdefine EVP_hpke_aes_128_gcm BORINGSSL_PREFIX %+ _EVP_hpke_aes_128_gcm
%xdefine EVP_hpke_aes_256_gcm BORINGSSL_PREFIX %+ _EVP_hpke_aes_256_gcm
%xdefine EVP_hpke_chacha20_poly1305 BORINGSSL_PREFIX %+ _EVP_hpke_chacha20_poly1305
%xdefine EVP_hpke_hkdf_sha256 BORINGSSL_PREFIX %+ _EVP_hpke_hkdf_sha256
%xdefine EVP_hpke_x25519_hkdf_sha256 BORINGSSL_PREFIX %+ _EVP_hpke_x25519_hkdf_sha256
%xdefine EVP_marshal_digest_algorithm BORINGSSL_PREFIX %+ _EVP_marshal_digest_algorithm
%xdefine EVP_marshal_private_key BORINGSSL_PREFIX %+ _EVP_marshal_private_key
%xdefine EVP_marshal_private_key_v2 BORINGSSL_PREFIX %+ _EVP_marshal_private_key_v2
%xdefine EVP_marshal_public_key BORINGSSL_PREFIX %+ _EVP_marshal_public_key
%xdefine EVP_md4 BORINGSSL_PREFIX %+ _EVP_md4
%xdefine EVP_md5 BORINGSSL_PREFIX %+ _EVP_md5
%xdefine EVP_md5_sha1 BORINGSSL_PREFIX %+ _EVP_md5_sha1
%xdefine EVP_md_null BORINGSSL_PREFIX %+ _EVP_md_null
%xdefine EVP_parse_digest_algorithm BORINGSSL_PREFIX %+ _EVP_parse_digest_algorithm
%xdefine EVP_parse_private_key BORINGSSL_PREFIX %+ _EVP_parse_private_key
%xdefine EVP_parse_public_key BORINGSSL_PREFIX %+ _EVP_parse_public_key
%xdefine EVP_rc2_40_cbc BORINGSSL_PREFIX %+ _EVP_rc2_40_cbc
%xdefine EVP_rc2_cbc BORINGSSL_PREFIX %+ _EVP_rc2_cbc
%xdefine EVP_rc4 BORINGSSL_PREFIX %+ _EVP_rc4
%xdefine EVP_read_pw_string BORINGSSL_PREFIX %+ _EVP_read_pw_string
%xdefine EVP_read_pw_string_min BORINGSSL_PREFIX %+ _EVP_read_pw_string_min
%xdefine EVP_ripemd160 BORINGSSL_PREFIX %+ _EVP_ripemd160
%xdefine EVP_sha1 BORINGSSL_PREFIX %+ _EVP_sha1
%xdefine EVP_sha224 BORINGSSL_PREFIX %+ _EVP_sha224
%xdefine EVP_sha256 BORINGSSL_PREFIX %+ _EVP_sha256
%xdefine EVP_sha384 BORINGSSL_PREFIX %+ _EVP_sha384
%xdefine EVP_sha3_224 BORINGSSL_PREFIX %+ _EVP_sha3_224
%xdefine EVP_sha3_256 BORINGSSL_PREFIX %+ _EVP_sha3_256
%xdefine EVP_sha3_384 BORINGSSL_PREFIX %+ _EVP_sha3_384
%xdefine EVP_sha3_512 BORINGSSL_PREFIX %+ _EVP_sha3_512
%xdefine EVP_sha512 BORINGSSL_PREFIX %+ _EVP_sha512
%xdefine EVP_sha512_224 BORINGSSL_PREFIX %+ _EVP_sha512_224
%xdefine EVP_sha512_256 BORINGSSL_PREFIX %+ _EVP_sha512_256
%xdefine EVP_shake128 BORINGSSL_PREFIX %+ _EVP_shake128
%xdefine EVP_shake256 BORINGSSL_PREFIX %+ _EVP_shake256
%xdefine EVP_tls_cbc_copy_mac BORINGSSL_PREFIX %+ _EVP_tls_cbc_copy_mac
%xdefine EVP_tls_cbc_digest_record BORINGSSL_PREFIX %+ _EVP_tls_cbc_digest_record
%xdefine EVP_tls_cbc_record_digest_supported BORINGSSL_PREFIX %+ _EVP_tls_cbc_record_digest_supported
%xdefine EVP_tls_cbc_remove_padding BORINGSSL_PREFIX %+ _EVP_tls_cbc_remove_padding
%xdefine EXTENDED_KEY_USAGE_free BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_free
%xdefine EXTENDED_KEY_USAGE_it BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_it
%xdefine EXTENDED_KEY_USAGE_new BORINGSSL_PREFIX %+ _EXTENDED_KEY_USAGE_new
%xdefine FIPS_is_entropy_cpu_jitter BORINGSSL_PREFIX %+ _FIPS_is_entropy_cpu_jitter
%xdefine FIPS_mode BORINGSSL_PREFIX %+ _FIPS_mode
%xdefine FIPS_mode_set BORINGSSL_PREFIX %+ _FIPS_mode_set
%xdefine FIPS_read_counter BORINGSSL_PREFIX %+ _FIPS_read_counter
%xdefine FIPS_service_indicator_after_call BORINGSSL_PREFIX %+ _FIPS_service_indicator_after_call
%xdefine FIPS_service_indicator_before_call BORINGSSL_PREFIX %+ _FIPS_service_indicator_before_call
%xdefine GENERAL_NAMES_free BORINGSSL_PREFIX %+ _GENERAL_NAMES_free
%xdefine GENERAL_NAMES_it BORINGSSL_PREFIX %+ _GENERAL_NAMES_it
%xdefine GENERAL_NAMES_new BORINGSSL_PREFIX %+ _GENERAL_NAMES_new
%xdefine GENERAL_NAME_cmp BORINGSSL_PREFIX %+ _GENERAL_NAME_cmp
%xdefine GENERAL_NAME_dup BORINGSSL_PREFIX %+ _GENERAL_NAME_dup
%xdefine GENERAL_NAME_free BORINGSSL_PREFIX %+ _GENERAL_NAME_free
%xdefine GENERAL_NAME_get0_otherName BORINGSSL_PREFIX %+ _GENERAL_NAME_get0_otherName
%xdefine GENERAL_NAME_get0_value BORINGSSL_PREFIX %+ _GENERAL_NAME_get0_value
%xdefine GENERAL_NAME_it BORINGSSL_PREFIX %+ _GENERAL_NAME_it
%xdefine GENERAL_NAME_new BORINGSSL_PREFIX %+ _GENERAL_NAME_new
%xdefine GENERAL_NAME_print BORINGSSL_PREFIX %+ _GENERAL_NAME_print
%xdefine GENERAL_NAME_set0_othername BORINGSSL_PREFIX %+ _GENERAL_NAME_set0_othername
%xdefine GENERAL_NAME_set0_value BORINGSSL_PREFIX %+ _GENERAL_NAME_set0_value
%xdefine GENERAL_SUBTREE_free BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_free
%xdefine GENERAL_SUBTREE_it BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_it
%xdefine GENERAL_SUBTREE_new BORINGSSL_PREFIX %+ _GENERAL_SUBTREE_new
%xdefine HKDF BORINGSSL_PREFIX %+ _HKDF
%xdefine HKDF_expand BORINGSSL_PREFIX %+ _HKDF_expand
%xdefine HKDF_extract BORINGSSL_PREFIX %+ _HKDF_extract
%xdefine HMAC BORINGSSL_PREFIX %+ _HMAC
%xdefine HMAC_CTX_cleanse BORINGSSL_PREFIX %+ _HMAC_CTX_cleanse
%xdefine HMAC_CTX_cleanup BORINGSSL_PREFIX %+ _HMAC_CTX_cleanup
%xdefine HMAC_CTX_copy BORINGSSL_PREFIX %+ _HMAC_CTX_copy
%xdefine HMAC_CTX_copy_ex BORINGSSL_PREFIX %+ _HMAC_CTX_copy_ex
%xdefine HMAC_CTX_free BORINGSSL_PREFIX %+ _HMAC_CTX_free
%xdefine HMAC_CTX_get_md BORINGSSL_PREFIX %+ _HMAC_CTX_get_md
%xdefine HMAC_CTX_init BORINGSSL_PREFIX %+ _HMAC_CTX_init
%xdefine HMAC_CTX_new BORINGSSL_PREFIX %+ _HMAC_CTX_new
%xdefine HMAC_CTX_reset BORINGSSL_PREFIX %+ _HMAC_CTX_reset
%xdefine HMAC_Final BORINGSSL_PREFIX %+ _HMAC_Final
%xdefine HMAC_Init BORINGSSL_PREFIX %+ _HMAC_Init
%xdefine HMAC_Init_ex BORINGSSL_PREFIX %+ _HMAC_Init_ex
%xdefine HMAC_Init_from_precomputed_key BORINGSSL_PREFIX %+ _HMAC_Init_from_precomputed_key
%xdefine HMAC_KEY_copy BORINGSSL_PREFIX %+ _HMAC_KEY_copy
%xdefine HMAC_KEY_new BORINGSSL_PREFIX %+ _HMAC_KEY_new
%xdefine HMAC_KEY_set BORINGSSL_PREFIX %+ _HMAC_KEY_set
%xdefine HMAC_Update BORINGSSL_PREFIX %+ _HMAC_Update
%xdefine HMAC_get_precomputed_key BORINGSSL_PREFIX %+ _HMAC_get_precomputed_key
%xdefine HMAC_set_precomputed_key_export BORINGSSL_PREFIX %+ _HMAC_set_precomputed_key_export
%xdefine HMAC_size BORINGSSL_PREFIX %+ _HMAC_size
%xdefine HMAC_with_precompute BORINGSSL_PREFIX %+ _HMAC_with_precompute
%xdefine HRSS_decap BORINGSSL_PREFIX %+ _HRSS_decap
%xdefine HRSS_encap BORINGSSL_PREFIX %+ _HRSS_encap
%xdefine HRSS_generate_key BORINGSSL_PREFIX %+ _HRSS_generate_key
%xdefine HRSS_marshal_public_key BORINGSSL_PREFIX %+ _HRSS_marshal_public_key
%xdefine HRSS_parse_public_key BORINGSSL_PREFIX %+ _HRSS_parse_public_key
%xdefine HRSS_poly3_invert BORINGSSL_PREFIX %+ _HRSS_poly3_invert
%xdefine HRSS_poly3_mul BORINGSSL_PREFIX %+ _HRSS_poly3_mul
%xdefine ISSUING_DIST_POINT_free BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_free
%xdefine ISSUING_DIST_POINT_it BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_it
%xdefine ISSUING_DIST_POINT_new BORINGSSL_PREFIX %+ _ISSUING_DIST_POINT_new
%xdefine KBKDF_ctr_hmac BORINGSSL_PREFIX %+ _KBKDF_ctr_hmac
%xdefine KEM_KEY_free BORINGSSL_PREFIX %+ _KEM_KEY_free
%xdefine KEM_KEY_get0_kem BORINGSSL_PREFIX %+ _KEM_KEY_get0_kem
%xdefine KEM_KEY_init BORINGSSL_PREFIX %+ _KEM_KEY_init
%xdefine KEM_KEY_new BORINGSSL_PREFIX %+ _KEM_KEY_new
%xdefine KEM_KEY_set_raw_key BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_key
%xdefine KEM_KEY_set_raw_keypair_from_seed BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_keypair_from_seed
%xdefine KEM_KEY_set_raw_public_key BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_public_key
%xdefine KEM_KEY_set_raw_secret_key BORINGSSL_PREFIX %+ _KEM_KEY_set_raw_secret_key
%xdefine KEM_find_asn1_by_nid BORINGSSL_PREFIX %+ _KEM_find_asn1_by_nid
%xdefine KEM_find_kem_by_nid BORINGSSL_PREFIX %+ _KEM_find_kem_by_nid
%xdefine Keccak1600_Absorb BORINGSSL_PREFIX %+ _Keccak1600_Absorb
%xdefine Keccak1600_Absorb_once_x4 BORINGSSL_PREFIX %+ _Keccak1600_Absorb_once_x4
%xdefine Keccak1600_Squeeze BORINGSSL_PREFIX %+ _Keccak1600_Squeeze
%xdefine Keccak1600_Squeezeblocks_x4 BORINGSSL_PREFIX %+ _Keccak1600_Squeezeblocks_x4
%xdefine KeccakF1600 BORINGSSL_PREFIX %+ _KeccakF1600
%xdefine KeccakF1600_hw BORINGSSL_PREFIX %+ _KeccakF1600_hw
%xdefine MD4 BORINGSSL_PREFIX %+ _MD4
%xdefine MD4_Final BORINGSSL_PREFIX %+ _MD4_Final
%xdefine MD4_Init BORINGSSL_PREFIX %+ _MD4_Init
%xdefine MD4_Transform BORINGSSL_PREFIX %+ _MD4_Transform
%xdefine MD4_Update BORINGSSL_PREFIX %+ _MD4_Update
%xdefine MD5 BORINGSSL_PREFIX %+ _MD5
%xdefine MD5_Final BORINGSSL_PREFIX %+ _MD5_Final
%xdefine MD5_Init BORINGSSL_PREFIX %+ _MD5_Init
%xdefine MD5_Init_from_state BORINGSSL_PREFIX %+ _MD5_Init_from_state
%xdefine MD5_Transform BORINGSSL_PREFIX %+ _MD5_Transform
%xdefine MD5_Update BORINGSSL_PREFIX %+ _MD5_Update
%xdefine MD5_get_state BORINGSSL_PREFIX %+ _MD5_get_state
%xdefine MGF1 BORINGSSL_PREFIX %+ _MGF1
%xdefine NAME_CONSTRAINTS_check BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_check
%xdefine NAME_CONSTRAINTS_check_CN BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_check_CN
%xdefine NAME_CONSTRAINTS_free BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_free
%xdefine NAME_CONSTRAINTS_it BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_it
%xdefine NAME_CONSTRAINTS_new BORINGSSL_PREFIX %+ _NAME_CONSTRAINTS_new
%xdefine NCONF_free BORINGSSL_PREFIX %+ _NCONF_free
%xdefine NCONF_get_section BORINGSSL_PREFIX %+ _NCONF_get_section
%xdefine NCONF_get_string BORINGSSL_PREFIX %+ _NCONF_get_string
%xdefine NCONF_load BORINGSSL_PREFIX %+ _NCONF_load
%xdefine NCONF_load_bio BORINGSSL_PREFIX %+ _NCONF_load_bio
%xdefine NCONF_new BORINGSSL_PREFIX %+ _NCONF_new
%xdefine NETSCAPE_SPKAC_free BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_free
%xdefine NETSCAPE_SPKAC_it BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_it
%xdefine NETSCAPE_SPKAC_new BORINGSSL_PREFIX %+ _NETSCAPE_SPKAC_new
%xdefine NETSCAPE_SPKI_b64_decode BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_b64_decode
%xdefine NETSCAPE_SPKI_b64_encode BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_b64_encode
%xdefine NETSCAPE_SPKI_free BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_free
%xdefine NETSCAPE_SPKI_get_pubkey BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_get_pubkey
%xdefine NETSCAPE_SPKI_it BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_it
%xdefine NETSCAPE_SPKI_new BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_new
%xdefine NETSCAPE_SPKI_print BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_print
%xdefine NETSCAPE_SPKI_set_pubkey BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_set_pubkey
%xdefine NETSCAPE_SPKI_sign BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_sign
%xdefine NETSCAPE_SPKI_verify BORINGSSL_PREFIX %+ _NETSCAPE_SPKI_verify
%xdefine NOTICEREF_free BORINGSSL_PREFIX %+ _NOTICEREF_free
%xdefine NOTICEREF_it BORINGSSL_PREFIX %+ _NOTICEREF_it
%xdefine NOTICEREF_new BORINGSSL_PREFIX %+ _NOTICEREF_new
%xdefine OBJ_NAME_do_all_sorted BORINGSSL_PREFIX %+ _OBJ_NAME_do_all_sorted
%xdefine OBJ_cbs2nid BORINGSSL_PREFIX %+ _OBJ_cbs2nid
%xdefine OBJ_cleanup BORINGSSL_PREFIX %+ _OBJ_cleanup
%xdefine OBJ_cmp BORINGSSL_PREFIX %+ _OBJ_cmp
%xdefine OBJ_create BORINGSSL_PREFIX %+ _OBJ_create
%xdefine OBJ_dup BORINGSSL_PREFIX %+ _OBJ_dup
%xdefine OBJ_find_sigid_algs BORINGSSL_PREFIX %+ _OBJ_find_sigid_algs
%xdefine OBJ_find_sigid_by_algs BORINGSSL_PREFIX %+ _OBJ_find_sigid_by_algs
%xdefine OBJ_get0_data BORINGSSL_PREFIX %+ _OBJ_get0_data
%xdefine OBJ_get_undef BORINGSSL_PREFIX %+ _OBJ_get_undef
%xdefine OBJ_length BORINGSSL_PREFIX %+ _OBJ_length
%xdefine OBJ_ln2nid BORINGSSL_PREFIX %+ _OBJ_ln2nid
%xdefine OBJ_nid2cbb BORINGSSL_PREFIX %+ _OBJ_nid2cbb
%xdefine OBJ_nid2ln BORINGSSL_PREFIX %+ _OBJ_nid2ln
%xdefine OBJ_nid2obj BORINGSSL_PREFIX %+ _OBJ_nid2obj
%xdefine OBJ_nid2sn BORINGSSL_PREFIX %+ _OBJ_nid2sn
%xdefine OBJ_obj2nid BORINGSSL_PREFIX %+ _OBJ_obj2nid
%xdefine OBJ_obj2txt BORINGSSL_PREFIX %+ _OBJ_obj2txt
%xdefine OBJ_sn2nid BORINGSSL_PREFIX %+ _OBJ_sn2nid
%xdefine OBJ_txt2nid BORINGSSL_PREFIX %+ _OBJ_txt2nid
%xdefine OBJ_txt2obj BORINGSSL_PREFIX %+ _OBJ_txt2obj
%xdefine OCSP_BASICRESP_add_ext BORINGSSL_PREFIX %+ _OCSP_BASICRESP_add_ext
%xdefine OCSP_BASICRESP_delete_ext BORINGSSL_PREFIX %+ _OCSP_BASICRESP_delete_ext
%xdefine OCSP_BASICRESP_free BORINGSSL_PREFIX %+ _OCSP_BASICRESP_free
%xdefine OCSP_BASICRESP_get_ext BORINGSSL_PREFIX %+ _OCSP_BASICRESP_get_ext
%xdefine OCSP_BASICRESP_get_ext_by_NID BORINGSSL_PREFIX %+ _OCSP_BASICRESP_get_ext_by_NID
%xdefine OCSP_BASICRESP_it BORINGSSL_PREFIX %+ _OCSP_BASICRESP_it
%xdefine OCSP_BASICRESP_new BORINGSSL_PREFIX %+ _OCSP_BASICRESP_new
%xdefine OCSP_CERTID_dup BORINGSSL_PREFIX %+ _OCSP_CERTID_dup
%xdefine OCSP_CERTID_free BORINGSSL_PREFIX %+ _OCSP_CERTID_free
%xdefine OCSP_CERTID_it BORINGSSL_PREFIX %+ _OCSP_CERTID_it
%xdefine OCSP_CERTID_new BORINGSSL_PREFIX %+ _OCSP_CERTID_new
%xdefine OCSP_CERTSTATUS_it BORINGSSL_PREFIX %+ _OCSP_CERTSTATUS_it
%xdefine OCSP_ONEREQ_free BORINGSSL_PREFIX %+ _OCSP_ONEREQ_free
%xdefine OCSP_ONEREQ_it BORINGSSL_PREFIX %+ _OCSP_ONEREQ_it
%xdefine OCSP_ONEREQ_new BORINGSSL_PREFIX %+ _OCSP_ONEREQ_new
%xdefine OCSP_REQINFO_free BORINGSSL_PREFIX %+ _OCSP_REQINFO_free
%xdefine OCSP_REQINFO_it BORINGSSL_PREFIX %+ _OCSP_REQINFO_it
%xdefine OCSP_REQINFO_new BORINGSSL_PREFIX %+ _OCSP_REQINFO_new
%xdefine OCSP_REQUEST_free BORINGSSL_PREFIX %+ _OCSP_REQUEST_free
%xdefine OCSP_REQUEST_get_ext BORINGSSL_PREFIX %+ _OCSP_REQUEST_get_ext
%xdefine OCSP_REQUEST_get_ext_by_NID BORINGSSL_PREFIX %+ _OCSP_REQUEST_get_ext_by_NID
%xdefine OCSP_REQUEST_it BORINGSSL_PREFIX %+ _OCSP_REQUEST_it
%xdefine OCSP_REQUEST_new BORINGSSL_PREFIX %+ _OCSP_REQUEST_new
%xdefine OCSP_REQUEST_print BORINGSSL_PREFIX %+ _OCSP_REQUEST_print
%xdefine OCSP_REQ_CTX_add1_header BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_add1_header
%xdefine OCSP_REQ_CTX_free BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_free
%xdefine OCSP_REQ_CTX_get0_mem_bio BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_get0_mem_bio
%xdefine OCSP_REQ_CTX_http BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_http
%xdefine OCSP_REQ_CTX_i2d BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_i2d
%xdefine OCSP_REQ_CTX_nbio BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_nbio
%xdefine OCSP_REQ_CTX_nbio_d2i BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_nbio_d2i
%xdefine OCSP_REQ_CTX_new BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_new
%xdefine OCSP_REQ_CTX_set1_req BORINGSSL_PREFIX %+ _OCSP_REQ_CTX_set1_req
%xdefine OCSP_RESPBYTES_free BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_free
%xdefine OCSP_RESPBYTES_it BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_it
%xdefine OCSP_RESPBYTES_new BORINGSSL_PREFIX %+ _OCSP_RESPBYTES_new
%xdefine OCSP_RESPDATA_free BORINGSSL_PREFIX %+ _OCSP_RESPDATA_free
%xdefine OCSP_RESPDATA_it BORINGSSL_PREFIX %+ _OCSP_RESPDATA_it
%xdefine OCSP_RESPDATA_new BORINGSSL_PREFIX %+ _OCSP_RESPDATA_new
%xdefine OCSP_RESPID_it BORINGSSL_PREFIX %+ _OCSP_RESPID_it
%xdefine OCSP_RESPONSE_free BORINGSSL_PREFIX %+ _OCSP_RESPONSE_free
%xdefine OCSP_RESPONSE_it BORINGSSL_PREFIX %+ _OCSP_RESPONSE_it
%xdefine OCSP_RESPONSE_new BORINGSSL_PREFIX %+ _OCSP_RESPONSE_new
%xdefine OCSP_RESPONSE_print BORINGSSL_PREFIX %+ _OCSP_RESPONSE_print
%xdefine OCSP_REVOKEDINFO_free BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_free
%xdefine OCSP_REVOKEDINFO_it BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_it
%xdefine OCSP_REVOKEDINFO_new BORINGSSL_PREFIX %+ _OCSP_REVOKEDINFO_new
%xdefine OCSP_SIGNATURE_free BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_free
%xdefine OCSP_SIGNATURE_it BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_it
%xdefine OCSP_SIGNATURE_new BORINGSSL_PREFIX %+ _OCSP_SIGNATURE_new
%xdefine OCSP_SINGLERESP_add_ext BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_add_ext
%xdefine OCSP_SINGLERESP_free BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_free
%xdefine OCSP_SINGLERESP_get0_id BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get0_id
%xdefine OCSP_SINGLERESP_get_ext BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get_ext
%xdefine OCSP_SINGLERESP_get_ext_count BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_get_ext_count
%xdefine OCSP_SINGLERESP_it BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_it
%xdefine OCSP_SINGLERESP_new BORINGSSL_PREFIX %+ _OCSP_SINGLERESP_new
%xdefine OCSP_basic_add1_cert BORINGSSL_PREFIX %+ _OCSP_basic_add1_cert
%xdefine OCSP_basic_add1_nonce BORINGSSL_PREFIX %+ _OCSP_basic_add1_nonce
%xdefine OCSP_basic_add1_status BORINGSSL_PREFIX %+ _OCSP_basic_add1_status
%xdefine OCSP_basic_sign BORINGSSL_PREFIX %+ _OCSP_basic_sign
%xdefine OCSP_basic_verify BORINGSSL_PREFIX %+ _OCSP_basic_verify
%xdefine OCSP_cert_id_new BORINGSSL_PREFIX %+ _OCSP_cert_id_new
%xdefine OCSP_cert_status_str BORINGSSL_PREFIX %+ _OCSP_cert_status_str
%xdefine OCSP_cert_to_id BORINGSSL_PREFIX %+ _OCSP_cert_to_id
%xdefine OCSP_check_nonce BORINGSSL_PREFIX %+ _OCSP_check_nonce
%xdefine OCSP_check_validity BORINGSSL_PREFIX %+ _OCSP_check_validity
%xdefine OCSP_copy_nonce BORINGSSL_PREFIX %+ _OCSP_copy_nonce
%xdefine OCSP_crl_reason_str BORINGSSL_PREFIX %+ _OCSP_crl_reason_str
%xdefine OCSP_get_default_digest BORINGSSL_PREFIX %+ _OCSP_get_default_digest
%xdefine OCSP_id_cmp BORINGSSL_PREFIX %+ _OCSP_id_cmp
%xdefine OCSP_id_get0_info BORINGSSL_PREFIX %+ _OCSP_id_get0_info
%xdefine OCSP_id_issuer_cmp BORINGSSL_PREFIX %+ _OCSP_id_issuer_cmp
%xdefine OCSP_onereq_get0_id BORINGSSL_PREFIX %+ _OCSP_onereq_get0_id
%xdefine OCSP_parse_url BORINGSSL_PREFIX %+ _OCSP_parse_url
%xdefine OCSP_request_add0_id BORINGSSL_PREFIX %+ _OCSP_request_add0_id
%xdefine OCSP_request_add1_cert BORINGSSL_PREFIX %+ _OCSP_request_add1_cert
%xdefine OCSP_request_add1_nonce BORINGSSL_PREFIX %+ _OCSP_request_add1_nonce
%xdefine OCSP_request_is_signed BORINGSSL_PREFIX %+ _OCSP_request_is_signed
%xdefine OCSP_request_onereq_count BORINGSSL_PREFIX %+ _OCSP_request_onereq_count
%xdefine OCSP_request_onereq_get0 BORINGSSL_PREFIX %+ _OCSP_request_onereq_get0
%xdefine OCSP_request_set1_name BORINGSSL_PREFIX %+ _OCSP_request_set1_name
%xdefine OCSP_request_sign BORINGSSL_PREFIX %+ _OCSP_request_sign
%xdefine OCSP_request_verify BORINGSSL_PREFIX %+ _OCSP_request_verify
%xdefine OCSP_resp_count BORINGSSL_PREFIX %+ _OCSP_resp_count
%xdefine OCSP_resp_find BORINGSSL_PREFIX %+ _OCSP_resp_find
%xdefine OCSP_resp_find_status BORINGSSL_PREFIX %+ _OCSP_resp_find_status
%xdefine OCSP_resp_get0 BORINGSSL_PREFIX %+ _OCSP_resp_get0
%xdefine OCSP_response_create BORINGSSL_PREFIX %+ _OCSP_response_create
%xdefine OCSP_response_get1_basic BORINGSSL_PREFIX %+ _OCSP_response_get1_basic
%xdefine OCSP_response_status BORINGSSL_PREFIX %+ _OCSP_response_status
%xdefine OCSP_response_status_str BORINGSSL_PREFIX %+ _OCSP_response_status_str
%xdefine OCSP_sendreq_bio BORINGSSL_PREFIX %+ _OCSP_sendreq_bio
%xdefine OCSP_sendreq_nbio BORINGSSL_PREFIX %+ _OCSP_sendreq_nbio
%xdefine OCSP_sendreq_new BORINGSSL_PREFIX %+ _OCSP_sendreq_new
%xdefine OCSP_set_max_response_length BORINGSSL_PREFIX %+ _OCSP_set_max_response_length
%xdefine OCSP_single_get0_status BORINGSSL_PREFIX %+ _OCSP_single_get0_status
%xdefine OPENSSL_add_all_algorithms_conf BORINGSSL_PREFIX %+ _OPENSSL_add_all_algorithms_conf
%xdefine OPENSSL_armcap_P BORINGSSL_PREFIX %+ _OPENSSL_armcap_P
%xdefine OPENSSL_asprintf BORINGSSL_PREFIX %+ _OPENSSL_asprintf
%xdefine OPENSSL_calloc BORINGSSL_PREFIX %+ _OPENSSL_calloc
%xdefine OPENSSL_cleanse BORINGSSL_PREFIX %+ _OPENSSL_cleanse
%xdefine OPENSSL_cleanup BORINGSSL_PREFIX %+ _OPENSSL_cleanup
%xdefine OPENSSL_clear_free BORINGSSL_PREFIX %+ _OPENSSL_clear_free
%xdefine OPENSSL_config BORINGSSL_PREFIX %+ _OPENSSL_config
%xdefine OPENSSL_cpucap_initialized BORINGSSL_PREFIX %+ _OPENSSL_cpucap_initialized
%xdefine OPENSSL_cpuid_setup BORINGSSL_PREFIX %+ _OPENSSL_cpuid_setup
%xdefine OPENSSL_free BORINGSSL_PREFIX %+ _OPENSSL_free
%xdefine OPENSSL_fromxdigit BORINGSSL_PREFIX %+ _OPENSSL_fromxdigit
%xdefine OPENSSL_gmtime BORINGSSL_PREFIX %+ _OPENSSL_gmtime
%xdefine OPENSSL_gmtime_adj BORINGSSL_PREFIX %+ _OPENSSL_gmtime_adj
%xdefine OPENSSL_gmtime_diff BORINGSSL_PREFIX %+ _OPENSSL_gmtime_diff
%xdefine OPENSSL_hash32 BORINGSSL_PREFIX %+ _OPENSSL_hash32
%xdefine OPENSSL_hexstr2buf BORINGSSL_PREFIX %+ _OPENSSL_hexstr2buf
%xdefine OPENSSL_ia32cap_P BORINGSSL_PREFIX %+ _OPENSSL_ia32cap_P
%xdefine OPENSSL_init BORINGSSL_PREFIX %+ _OPENSSL_init
%xdefine OPENSSL_init_crypto BORINGSSL_PREFIX %+ _OPENSSL_init_crypto
%xdefine OPENSSL_isalnum BORINGSSL_PREFIX %+ _OPENSSL_isalnum
%xdefine OPENSSL_isalpha BORINGSSL_PREFIX %+ _OPENSSL_isalpha
%xdefine OPENSSL_isdigit BORINGSSL_PREFIX %+ _OPENSSL_isdigit
%xdefine OPENSSL_isspace BORINGSSL_PREFIX %+ _OPENSSL_isspace
%xdefine OPENSSL_isxdigit BORINGSSL_PREFIX %+ _OPENSSL_isxdigit
%xdefine OPENSSL_lh_delete BORINGSSL_PREFIX %+ _OPENSSL_lh_delete
%xdefine OPENSSL_lh_doall_arg BORINGSSL_PREFIX %+ _OPENSSL_lh_doall_arg
%xdefine OPENSSL_lh_free BORINGSSL_PREFIX %+ _OPENSSL_lh_free
%xdefine OPENSSL_lh_insert BORINGSSL_PREFIX %+ _OPENSSL_lh_insert
%xdefine OPENSSL_lh_new BORINGSSL_PREFIX %+ _OPENSSL_lh_new
%xdefine OPENSSL_lh_num_items BORINGSSL_PREFIX %+ _OPENSSL_lh_num_items
%xdefine OPENSSL_lh_retrieve BORINGSSL_PREFIX %+ _OPENSSL_lh_retrieve
%xdefine OPENSSL_lh_retrieve_key BORINGSSL_PREFIX %+ _OPENSSL_lh_retrieve_key
%xdefine OPENSSL_load_builtin_modules BORINGSSL_PREFIX %+ _OPENSSL_load_builtin_modules
%xdefine OPENSSL_malloc BORINGSSL_PREFIX %+ _OPENSSL_malloc
%xdefine OPENSSL_malloc_init BORINGSSL_PREFIX %+ _OPENSSL_malloc_init
%xdefine OPENSSL_memdup BORINGSSL_PREFIX %+ _OPENSSL_memdup
%xdefine OPENSSL_no_config BORINGSSL_PREFIX %+ _OPENSSL_no_config
%xdefine OPENSSL_posix_to_tm BORINGSSL_PREFIX %+ _OPENSSL_posix_to_tm
%xdefine OPENSSL_ppc64le_hwcap2 BORINGSSL_PREFIX %+ _OPENSSL_ppc64le_hwcap2
%xdefine OPENSSL_realloc BORINGSSL_PREFIX %+ _OPENSSL_realloc
%xdefine OPENSSL_secure_clear_free BORINGSSL_PREFIX %+ _OPENSSL_secure_clear_free
%xdefine OPENSSL_secure_malloc BORINGSSL_PREFIX %+ _OPENSSL_secure_malloc
%xdefine OPENSSL_secure_zalloc BORINGSSL_PREFIX %+ _OPENSSL_secure_zalloc
%xdefine OPENSSL_sk_deep_copy BORINGSSL_PREFIX %+ _OPENSSL_sk_deep_copy
%xdefine OPENSSL_sk_delete BORINGSSL_PREFIX %+ _OPENSSL_sk_delete
%xdefine OPENSSL_sk_delete_if BORINGSSL_PREFIX %+ _OPENSSL_sk_delete_if
%xdefine OPENSSL_sk_delete_ptr BORINGSSL_PREFIX %+ _OPENSSL_sk_delete_ptr
%xdefine OPENSSL_sk_dup BORINGSSL_PREFIX %+ _OPENSSL_sk_dup
%xdefine OPENSSL_sk_find BORINGSSL_PREFIX %+ _OPENSSL_sk_find
%xdefine OPENSSL_sk_free BORINGSSL_PREFIX %+ _OPENSSL_sk_free
%xdefine OPENSSL_sk_insert BORINGSSL_PREFIX %+ _OPENSSL_sk_insert
%xdefine OPENSSL_sk_is_sorted BORINGSSL_PREFIX %+ _OPENSSL_sk_is_sorted
%xdefine OPENSSL_sk_new BORINGSSL_PREFIX %+ _OPENSSL_sk_new
%xdefine OPENSSL_sk_new_null BORINGSSL_PREFIX %+ _OPENSSL_sk_new_null
%xdefine OPENSSL_sk_num BORINGSSL_PREFIX %+ _OPENSSL_sk_num
%xdefine OPENSSL_sk_pop BORINGSSL_PREFIX %+ _OPENSSL_sk_pop
%xdefine OPENSSL_sk_pop_free_ex BORINGSSL_PREFIX %+ _OPENSSL_sk_pop_free_ex
%xdefine OPENSSL_sk_push BORINGSSL_PREFIX %+ _OPENSSL_sk_push
%xdefine OPENSSL_sk_set BORINGSSL_PREFIX %+ _OPENSSL_sk_set
%xdefine OPENSSL_sk_set_cmp_func BORINGSSL_PREFIX %+ _OPENSSL_sk_set_cmp_func
%xdefine OPENSSL_sk_shift BORINGSSL_PREFIX %+ _OPENSSL_sk_shift
%xdefine OPENSSL_sk_sort BORINGSSL_PREFIX %+ _OPENSSL_sk_sort
%xdefine OPENSSL_sk_unshift BORINGSSL_PREFIX %+ _OPENSSL_sk_unshift
%xdefine OPENSSL_sk_value BORINGSSL_PREFIX %+ _OPENSSL_sk_value
%xdefine OPENSSL_sk_zero BORINGSSL_PREFIX %+ _OPENSSL_sk_zero
%xdefine OPENSSL_strcasecmp BORINGSSL_PREFIX %+ _OPENSSL_strcasecmp
%xdefine OPENSSL_strdup BORINGSSL_PREFIX %+ _OPENSSL_strdup
%xdefine OPENSSL_strhash BORINGSSL_PREFIX %+ _OPENSSL_strhash
%xdefine OPENSSL_strlcat BORINGSSL_PREFIX %+ _OPENSSL_strlcat
%xdefine OPENSSL_strlcpy BORINGSSL_PREFIX %+ _OPENSSL_strlcpy
%xdefine OPENSSL_strncasecmp BORINGSSL_PREFIX %+ _OPENSSL_strncasecmp
%xdefine OPENSSL_strndup BORINGSSL_PREFIX %+ _OPENSSL_strndup
%xdefine OPENSSL_strnlen BORINGSSL_PREFIX %+ _OPENSSL_strnlen
%xdefine OPENSSL_timegm BORINGSSL_PREFIX %+ _OPENSSL_timegm
%xdefine OPENSSL_tm_to_posix BORINGSSL_PREFIX %+ _OPENSSL_tm_to_posix
%xdefine OPENSSL_tolower BORINGSSL_PREFIX %+ _OPENSSL_tolower
%xdefine OPENSSL_vasprintf BORINGSSL_PREFIX %+ _OPENSSL_vasprintf
%xdefine OPENSSL_vasprintf_internal BORINGSSL_PREFIX %+ _OPENSSL_vasprintf_internal
%xdefine OPENSSL_zalloc BORINGSSL_PREFIX %+ _OPENSSL_zalloc
%xdefine OTHERNAME_free BORINGSSL_PREFIX %+ _OTHERNAME_free
%xdefine OTHERNAME_it BORINGSSL_PREFIX %+ _OTHERNAME_it
%xdefine OTHERNAME_new BORINGSSL_PREFIX %+ _OTHERNAME_new
%xdefine OpenSSL_add_all_algorithms BORINGSSL_PREFIX %+ _OpenSSL_add_all_algorithms
%xdefine OpenSSL_add_all_ciphers BORINGSSL_PREFIX %+ _OpenSSL_add_all_ciphers
%xdefine OpenSSL_add_all_digests BORINGSSL_PREFIX %+ _OpenSSL_add_all_digests
%xdefine OpenSSL_version BORINGSSL_PREFIX %+ _OpenSSL_version
%xdefine OpenSSL_version_num BORINGSSL_PREFIX %+ _OpenSSL_version_num
%xdefine PEM_ASN1_read BORINGSSL_PREFIX %+ _PEM_ASN1_read
%xdefine PEM_ASN1_read_bio BORINGSSL_PREFIX %+ _PEM_ASN1_read_bio
%xdefine PEM_ASN1_write BORINGSSL_PREFIX %+ _PEM_ASN1_write
%xdefine PEM_ASN1_write_bio BORINGSSL_PREFIX %+ _PEM_ASN1_write_bio
%xdefine PEM_X509_INFO_read BORINGSSL_PREFIX %+ _PEM_X509_INFO_read
%xdefine PEM_X509_INFO_read_bio BORINGSSL_PREFIX %+ _PEM_X509_INFO_read_bio
%xdefine PEM_X509_INFO_write_bio BORINGSSL_PREFIX %+ _PEM_X509_INFO_write_bio
%xdefine PEM_bytes_read_bio BORINGSSL_PREFIX %+ _PEM_bytes_read_bio
%xdefine PEM_def_callback BORINGSSL_PREFIX %+ _PEM_def_callback
%xdefine PEM_dek_info BORINGSSL_PREFIX %+ _PEM_dek_info
%xdefine PEM_do_header BORINGSSL_PREFIX %+ _PEM_do_header
%xdefine PEM_get_EVP_CIPHER_INFO BORINGSSL_PREFIX %+ _PEM_get_EVP_CIPHER_INFO
%xdefine PEM_proc_type BORINGSSL_PREFIX %+ _PEM_proc_type
%xdefine PEM_read BORINGSSL_PREFIX %+ _PEM_read
%xdefine PEM_read_DHparams BORINGSSL_PREFIX %+ _PEM_read_DHparams
%xdefine PEM_read_DSAPrivateKey BORINGSSL_PREFIX %+ _PEM_read_DSAPrivateKey
%xdefine PEM_read_DSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_DSA_PUBKEY
%xdefine PEM_read_DSAparams BORINGSSL_PREFIX %+ _PEM_read_DSAparams
%xdefine PEM_read_ECPrivateKey BORINGSSL_PREFIX %+ _PEM_read_ECPrivateKey
%xdefine PEM_read_EC_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_EC_PUBKEY
%xdefine PEM_read_PKCS7 BORINGSSL_PREFIX %+ _PEM_read_PKCS7
%xdefine PEM_read_PKCS8 BORINGSSL_PREFIX %+ _PEM_read_PKCS8
%xdefine PEM_read_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _PEM_read_PKCS8_PRIV_KEY_INFO
%xdefine PEM_read_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_PUBKEY
%xdefine PEM_read_PrivateKey BORINGSSL_PREFIX %+ _PEM_read_PrivateKey
%xdefine PEM_read_RSAPrivateKey BORINGSSL_PREFIX %+ _PEM_read_RSAPrivateKey
%xdefine PEM_read_RSAPublicKey BORINGSSL_PREFIX %+ _PEM_read_RSAPublicKey
%xdefine PEM_read_RSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_RSA_PUBKEY
%xdefine PEM_read_X509 BORINGSSL_PREFIX %+ _PEM_read_X509
%xdefine PEM_read_X509_AUX BORINGSSL_PREFIX %+ _PEM_read_X509_AUX
%xdefine PEM_read_X509_CRL BORINGSSL_PREFIX %+ _PEM_read_X509_CRL
%xdefine PEM_read_X509_REQ BORINGSSL_PREFIX %+ _PEM_read_X509_REQ
%xdefine PEM_read_bio BORINGSSL_PREFIX %+ _PEM_read_bio
%xdefine PEM_read_bio_DHparams BORINGSSL_PREFIX %+ _PEM_read_bio_DHparams
%xdefine PEM_read_bio_DSAPrivateKey BORINGSSL_PREFIX %+ _PEM_read_bio_DSAPrivateKey
%xdefine PEM_read_bio_DSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_bio_DSA_PUBKEY
%xdefine PEM_read_bio_DSAparams BORINGSSL_PREFIX %+ _PEM_read_bio_DSAparams
%xdefine PEM_read_bio_ECPKParameters BORINGSSL_PREFIX %+ _PEM_read_bio_ECPKParameters
%xdefine PEM_read_bio_ECPrivateKey BORINGSSL_PREFIX %+ _PEM_read_bio_ECPrivateKey
%xdefine PEM_read_bio_EC_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_bio_EC_PUBKEY
%xdefine PEM_read_bio_PKCS7 BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS7
%xdefine PEM_read_bio_PKCS8 BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS8
%xdefine PEM_read_bio_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _PEM_read_bio_PKCS8_PRIV_KEY_INFO
%xdefine PEM_read_bio_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_bio_PUBKEY
%xdefine PEM_read_bio_Parameters BORINGSSL_PREFIX %+ _PEM_read_bio_Parameters
%xdefine PEM_read_bio_PrivateKey BORINGSSL_PREFIX %+ _PEM_read_bio_PrivateKey
%xdefine PEM_read_bio_RSAPrivateKey BORINGSSL_PREFIX %+ _PEM_read_bio_RSAPrivateKey
%xdefine PEM_read_bio_RSAPublicKey BORINGSSL_PREFIX %+ _PEM_read_bio_RSAPublicKey
%xdefine PEM_read_bio_RSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_read_bio_RSA_PUBKEY
%xdefine PEM_read_bio_X509 BORINGSSL_PREFIX %+ _PEM_read_bio_X509
%xdefine PEM_read_bio_X509_AUX BORINGSSL_PREFIX %+ _PEM_read_bio_X509_AUX
%xdefine PEM_read_bio_X509_CRL BORINGSSL_PREFIX %+ _PEM_read_bio_X509_CRL
%xdefine PEM_read_bio_X509_REQ BORINGSSL_PREFIX %+ _PEM_read_bio_X509_REQ
%xdefine PEM_write BORINGSSL_PREFIX %+ _PEM_write
%xdefine PEM_write_DHparams BORINGSSL_PREFIX %+ _PEM_write_DHparams
%xdefine PEM_write_DSAPrivateKey BORINGSSL_PREFIX %+ _PEM_write_DSAPrivateKey
%xdefine PEM_write_DSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_DSA_PUBKEY
%xdefine PEM_write_DSAparams BORINGSSL_PREFIX %+ _PEM_write_DSAparams
%xdefine PEM_write_ECPrivateKey BORINGSSL_PREFIX %+ _PEM_write_ECPrivateKey
%xdefine PEM_write_EC_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_EC_PUBKEY
%xdefine PEM_write_PKCS7 BORINGSSL_PREFIX %+ _PEM_write_PKCS7
%xdefine PEM_write_PKCS8 BORINGSSL_PREFIX %+ _PEM_write_PKCS8
%xdefine PEM_write_PKCS8PrivateKey BORINGSSL_PREFIX %+ _PEM_write_PKCS8PrivateKey
%xdefine PEM_write_PKCS8PrivateKey_nid BORINGSSL_PREFIX %+ _PEM_write_PKCS8PrivateKey_nid
%xdefine PEM_write_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _PEM_write_PKCS8_PRIV_KEY_INFO
%xdefine PEM_write_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_PUBKEY
%xdefine PEM_write_PrivateKey BORINGSSL_PREFIX %+ _PEM_write_PrivateKey
%xdefine PEM_write_RSAPrivateKey BORINGSSL_PREFIX %+ _PEM_write_RSAPrivateKey
%xdefine PEM_write_RSAPublicKey BORINGSSL_PREFIX %+ _PEM_write_RSAPublicKey
%xdefine PEM_write_RSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_RSA_PUBKEY
%xdefine PEM_write_X509 BORINGSSL_PREFIX %+ _PEM_write_X509
%xdefine PEM_write_X509_AUX BORINGSSL_PREFIX %+ _PEM_write_X509_AUX
%xdefine PEM_write_X509_CRL BORINGSSL_PREFIX %+ _PEM_write_X509_CRL
%xdefine PEM_write_X509_REQ BORINGSSL_PREFIX %+ _PEM_write_X509_REQ
%xdefine PEM_write_X509_REQ_NEW BORINGSSL_PREFIX %+ _PEM_write_X509_REQ_NEW
%xdefine PEM_write_bio BORINGSSL_PREFIX %+ _PEM_write_bio
%xdefine PEM_write_bio_DHparams BORINGSSL_PREFIX %+ _PEM_write_bio_DHparams
%xdefine PEM_write_bio_DSAPrivateKey BORINGSSL_PREFIX %+ _PEM_write_bio_DSAPrivateKey
%xdefine PEM_write_bio_DSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_bio_DSA_PUBKEY
%xdefine PEM_write_bio_DSAparams BORINGSSL_PREFIX %+ _PEM_write_bio_DSAparams
%xdefine PEM_write_bio_ECPKParameters BORINGSSL_PREFIX %+ _PEM_write_bio_ECPKParameters
%xdefine PEM_write_bio_ECPrivateKey BORINGSSL_PREFIX %+ _PEM_write_bio_ECPrivateKey
%xdefine PEM_write_bio_EC_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_bio_EC_PUBKEY
%xdefine PEM_write_bio_PKCS7 BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS7
%xdefine PEM_write_bio_PKCS8 BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8
%xdefine PEM_write_bio_PKCS8PrivateKey BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8PrivateKey
%xdefine PEM_write_bio_PKCS8PrivateKey_nid BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8PrivateKey_nid
%xdefine PEM_write_bio_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _PEM_write_bio_PKCS8_PRIV_KEY_INFO
%xdefine PEM_write_bio_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_bio_PUBKEY
%xdefine PEM_write_bio_Parameters BORINGSSL_PREFIX %+ _PEM_write_bio_Parameters
%xdefine PEM_write_bio_PrivateKey BORINGSSL_PREFIX %+ _PEM_write_bio_PrivateKey
%xdefine PEM_write_bio_PrivateKey_traditional BORINGSSL_PREFIX %+ _PEM_write_bio_PrivateKey_traditional
%xdefine PEM_write_bio_RSAPrivateKey BORINGSSL_PREFIX %+ _PEM_write_bio_RSAPrivateKey
%xdefine PEM_write_bio_RSAPublicKey BORINGSSL_PREFIX %+ _PEM_write_bio_RSAPublicKey
%xdefine PEM_write_bio_RSA_PUBKEY BORINGSSL_PREFIX %+ _PEM_write_bio_RSA_PUBKEY
%xdefine PEM_write_bio_X509 BORINGSSL_PREFIX %+ _PEM_write_bio_X509
%xdefine PEM_write_bio_X509_AUX BORINGSSL_PREFIX %+ _PEM_write_bio_X509_AUX
%xdefine PEM_write_bio_X509_CRL BORINGSSL_PREFIX %+ _PEM_write_bio_X509_CRL
%xdefine PEM_write_bio_X509_REQ BORINGSSL_PREFIX %+ _PEM_write_bio_X509_REQ
%xdefine PEM_write_bio_X509_REQ_NEW BORINGSSL_PREFIX %+ _PEM_write_bio_X509_REQ_NEW
%xdefine PKCS12_PBE_add BORINGSSL_PREFIX %+ _PKCS12_PBE_add
%xdefine PKCS12_create BORINGSSL_PREFIX %+ _PKCS12_create
%xdefine PKCS12_free BORINGSSL_PREFIX %+ _PKCS12_free
%xdefine PKCS12_get_key_and_certs BORINGSSL_PREFIX %+ _PKCS12_get_key_and_certs
%xdefine PKCS12_new BORINGSSL_PREFIX %+ _PKCS12_new
%xdefine PKCS12_parse BORINGSSL_PREFIX %+ _PKCS12_parse
%xdefine PKCS12_set_mac BORINGSSL_PREFIX %+ _PKCS12_set_mac
%xdefine PKCS12_verify_mac BORINGSSL_PREFIX %+ _PKCS12_verify_mac
%xdefine PKCS1_MGF1 BORINGSSL_PREFIX %+ _PKCS1_MGF1
%xdefine PKCS5_PBKDF2_HMAC BORINGSSL_PREFIX %+ _PKCS5_PBKDF2_HMAC
%xdefine PKCS5_PBKDF2_HMAC_SHA1 BORINGSSL_PREFIX %+ _PKCS5_PBKDF2_HMAC_SHA1
%xdefine PKCS5_pbe2_decrypt_init BORINGSSL_PREFIX %+ _PKCS5_pbe2_decrypt_init
%xdefine PKCS5_pbe2_encrypt_init BORINGSSL_PREFIX %+ _PKCS5_pbe2_encrypt_init
%xdefine PKCS7_ATTR_VERIFY_it BORINGSSL_PREFIX %+ _PKCS7_ATTR_VERIFY_it
%xdefine PKCS7_DIGEST_free BORINGSSL_PREFIX %+ _PKCS7_DIGEST_free
%xdefine PKCS7_DIGEST_it BORINGSSL_PREFIX %+ _PKCS7_DIGEST_it
%xdefine PKCS7_DIGEST_new BORINGSSL_PREFIX %+ _PKCS7_DIGEST_new
%xdefine PKCS7_ENCRYPT_free BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_free
%xdefine PKCS7_ENCRYPT_it BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_it
%xdefine PKCS7_ENCRYPT_new BORINGSSL_PREFIX %+ _PKCS7_ENCRYPT_new
%xdefine PKCS7_ENC_CONTENT_free BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_free
%xdefine PKCS7_ENC_CONTENT_it BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_it
%xdefine PKCS7_ENC_CONTENT_new BORINGSSL_PREFIX %+ _PKCS7_ENC_CONTENT_new
%xdefine PKCS7_ENVELOPE_free BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_free
%xdefine PKCS7_ENVELOPE_it BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_it
%xdefine PKCS7_ENVELOPE_new BORINGSSL_PREFIX %+ _PKCS7_ENVELOPE_new
%xdefine PKCS7_ISSUER_AND_SERIAL_free BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_free
%xdefine PKCS7_ISSUER_AND_SERIAL_it BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_it
%xdefine PKCS7_ISSUER_AND_SERIAL_new BORINGSSL_PREFIX %+ _PKCS7_ISSUER_AND_SERIAL_new
%xdefine PKCS7_RECIP_INFO_free BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_free
%xdefine PKCS7_RECIP_INFO_get0_alg BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_get0_alg
%xdefine PKCS7_RECIP_INFO_it BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_it
%xdefine PKCS7_RECIP_INFO_new BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_new
%xdefine PKCS7_RECIP_INFO_set BORINGSSL_PREFIX %+ _PKCS7_RECIP_INFO_set
%xdefine PKCS7_SIGNED_free BORINGSSL_PREFIX %+ _PKCS7_SIGNED_free
%xdefine PKCS7_SIGNED_it BORINGSSL_PREFIX %+ _PKCS7_SIGNED_it
%xdefine PKCS7_SIGNED_new BORINGSSL_PREFIX %+ _PKCS7_SIGNED_new
%xdefine PKCS7_SIGNER_INFO_free BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_free
%xdefine PKCS7_SIGNER_INFO_get0_algs BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_get0_algs
%xdefine PKCS7_SIGNER_INFO_it BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_it
%xdefine PKCS7_SIGNER_INFO_new BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_new
%xdefine PKCS7_SIGNER_INFO_set BORINGSSL_PREFIX %+ _PKCS7_SIGNER_INFO_set
%xdefine PKCS7_SIGN_ENVELOPE_free BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_free
%xdefine PKCS7_SIGN_ENVELOPE_it BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_it
%xdefine PKCS7_SIGN_ENVELOPE_new BORINGSSL_PREFIX %+ _PKCS7_SIGN_ENVELOPE_new
%xdefine PKCS7_add_certificate BORINGSSL_PREFIX %+ _PKCS7_add_certificate
%xdefine PKCS7_add_crl BORINGSSL_PREFIX %+ _PKCS7_add_crl
%xdefine PKCS7_add_recipient BORINGSSL_PREFIX %+ _PKCS7_add_recipient
%xdefine PKCS7_add_recipient_info BORINGSSL_PREFIX %+ _PKCS7_add_recipient_info
%xdefine PKCS7_add_signer BORINGSSL_PREFIX %+ _PKCS7_add_signer
%xdefine PKCS7_bundle_CRLs BORINGSSL_PREFIX %+ _PKCS7_bundle_CRLs
%xdefine PKCS7_bundle_certificates BORINGSSL_PREFIX %+ _PKCS7_bundle_certificates
%xdefine PKCS7_bundle_raw_certificates BORINGSSL_PREFIX %+ _PKCS7_bundle_raw_certificates
%xdefine PKCS7_content_new BORINGSSL_PREFIX %+ _PKCS7_content_new
%xdefine PKCS7_dataFinal BORINGSSL_PREFIX %+ _PKCS7_dataFinal
%xdefine PKCS7_dataInit BORINGSSL_PREFIX %+ _PKCS7_dataInit
%xdefine PKCS7_decrypt BORINGSSL_PREFIX %+ _PKCS7_decrypt
%xdefine PKCS7_dup BORINGSSL_PREFIX %+ _PKCS7_dup
%xdefine PKCS7_encrypt BORINGSSL_PREFIX %+ _PKCS7_encrypt
%xdefine PKCS7_free BORINGSSL_PREFIX %+ _PKCS7_free
%xdefine PKCS7_get0_signers BORINGSSL_PREFIX %+ _PKCS7_get0_signers
%xdefine PKCS7_get_CRLs BORINGSSL_PREFIX %+ _PKCS7_get_CRLs
%xdefine PKCS7_get_PEM_CRLs BORINGSSL_PREFIX %+ _PKCS7_get_PEM_CRLs
%xdefine PKCS7_get_PEM_certificates BORINGSSL_PREFIX %+ _PKCS7_get_PEM_certificates
%xdefine PKCS7_get_certificates BORINGSSL_PREFIX %+ _PKCS7_get_certificates
%xdefine PKCS7_get_detached BORINGSSL_PREFIX %+ _PKCS7_get_detached
%xdefine PKCS7_get_raw_certificates BORINGSSL_PREFIX %+ _PKCS7_get_raw_certificates
%xdefine PKCS7_get_recipient_info BORINGSSL_PREFIX %+ _PKCS7_get_recipient_info
%xdefine PKCS7_get_signed_attribute BORINGSSL_PREFIX %+ _PKCS7_get_signed_attribute
%xdefine PKCS7_get_signer_info BORINGSSL_PREFIX %+ _PKCS7_get_signer_info
%xdefine PKCS7_is_detached BORINGSSL_PREFIX %+ _PKCS7_is_detached
%xdefine PKCS7_it BORINGSSL_PREFIX %+ _PKCS7_it
%xdefine PKCS7_new BORINGSSL_PREFIX %+ _PKCS7_new
%xdefine PKCS7_print_ctx BORINGSSL_PREFIX %+ _PKCS7_print_ctx
%xdefine PKCS7_set_cipher BORINGSSL_PREFIX %+ _PKCS7_set_cipher
%xdefine PKCS7_set_content BORINGSSL_PREFIX %+ _PKCS7_set_content
%xdefine PKCS7_set_detached BORINGSSL_PREFIX %+ _PKCS7_set_detached
%xdefine PKCS7_set_digest BORINGSSL_PREFIX %+ _PKCS7_set_digest
%xdefine PKCS7_set_type BORINGSSL_PREFIX %+ _PKCS7_set_type
%xdefine PKCS7_sign BORINGSSL_PREFIX %+ _PKCS7_sign
%xdefine PKCS7_type_is_data BORINGSSL_PREFIX %+ _PKCS7_type_is_data
%xdefine PKCS7_type_is_digest BORINGSSL_PREFIX %+ _PKCS7_type_is_digest
%xdefine PKCS7_type_is_encrypted BORINGSSL_PREFIX %+ _PKCS7_type_is_encrypted
%xdefine PKCS7_type_is_enveloped BORINGSSL_PREFIX %+ _PKCS7_type_is_enveloped
%xdefine PKCS7_type_is_signed BORINGSSL_PREFIX %+ _PKCS7_type_is_signed
%xdefine PKCS7_type_is_signedAndEnveloped BORINGSSL_PREFIX %+ _PKCS7_type_is_signedAndEnveloped
%xdefine PKCS7_verify BORINGSSL_PREFIX %+ _PKCS7_verify
%xdefine PKCS8_PRIV_KEY_INFO_free BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_free
%xdefine PKCS8_PRIV_KEY_INFO_it BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_it
%xdefine PKCS8_PRIV_KEY_INFO_new BORINGSSL_PREFIX %+ _PKCS8_PRIV_KEY_INFO_new
%xdefine PKCS8_decrypt BORINGSSL_PREFIX %+ _PKCS8_decrypt
%xdefine PKCS8_encrypt BORINGSSL_PREFIX %+ _PKCS8_encrypt
%xdefine PKCS8_marshal_encrypted_private_key BORINGSSL_PREFIX %+ _PKCS8_marshal_encrypted_private_key
%xdefine PKCS8_parse_encrypted_private_key BORINGSSL_PREFIX %+ _PKCS8_parse_encrypted_private_key
%xdefine POLICYINFO_free BORINGSSL_PREFIX %+ _POLICYINFO_free
%xdefine POLICYINFO_it BORINGSSL_PREFIX %+ _POLICYINFO_it
%xdefine POLICYINFO_new BORINGSSL_PREFIX %+ _POLICYINFO_new
%xdefine POLICYQUALINFO_free BORINGSSL_PREFIX %+ _POLICYQUALINFO_free
%xdefine POLICYQUALINFO_it BORINGSSL_PREFIX %+ _POLICYQUALINFO_it
%xdefine POLICYQUALINFO_new BORINGSSL_PREFIX %+ _POLICYQUALINFO_new
%xdefine POLICY_CONSTRAINTS_free BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_free
%xdefine POLICY_CONSTRAINTS_it BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_it
%xdefine POLICY_CONSTRAINTS_new BORINGSSL_PREFIX %+ _POLICY_CONSTRAINTS_new
%xdefine POLICY_MAPPINGS_it BORINGSSL_PREFIX %+ _POLICY_MAPPINGS_it
%xdefine POLICY_MAPPING_free BORINGSSL_PREFIX %+ _POLICY_MAPPING_free
%xdefine POLICY_MAPPING_it BORINGSSL_PREFIX %+ _POLICY_MAPPING_it
%xdefine POLICY_MAPPING_new BORINGSSL_PREFIX %+ _POLICY_MAPPING_new
%xdefine PQDSA_KEY_free BORINGSSL_PREFIX %+ _PQDSA_KEY_free
%xdefine PQDSA_KEY_get0_dsa BORINGSSL_PREFIX %+ _PQDSA_KEY_get0_dsa
%xdefine PQDSA_KEY_init BORINGSSL_PREFIX %+ _PQDSA_KEY_init
%xdefine PQDSA_KEY_new BORINGSSL_PREFIX %+ _PQDSA_KEY_new
%xdefine PQDSA_KEY_set_raw_keypair_from_both BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_keypair_from_both
%xdefine PQDSA_KEY_set_raw_keypair_from_seed BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_keypair_from_seed
%xdefine PQDSA_KEY_set_raw_private_key BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_private_key
%xdefine PQDSA_KEY_set_raw_public_key BORINGSSL_PREFIX %+ _PQDSA_KEY_set_raw_public_key
%xdefine PQDSA_find_asn1_by_nid BORINGSSL_PREFIX %+ _PQDSA_find_asn1_by_nid
%xdefine PQDSA_find_dsa_by_nid BORINGSSL_PREFIX %+ _PQDSA_find_dsa_by_nid
%xdefine RAND_OpenSSL BORINGSSL_PREFIX %+ _RAND_OpenSSL
%xdefine RAND_SSLeay BORINGSSL_PREFIX %+ _RAND_SSLeay
%xdefine RAND_add BORINGSSL_PREFIX %+ _RAND_add
%xdefine RAND_bytes BORINGSSL_PREFIX %+ _RAND_bytes
%xdefine RAND_bytes_with_user_prediction_resistance BORINGSSL_PREFIX %+ _RAND_bytes_with_user_prediction_resistance
%xdefine RAND_cleanup BORINGSSL_PREFIX %+ _RAND_cleanup
%xdefine RAND_egd BORINGSSL_PREFIX %+ _RAND_egd
%xdefine RAND_egd_bytes BORINGSSL_PREFIX %+ _RAND_egd_bytes
%xdefine RAND_file_name BORINGSSL_PREFIX %+ _RAND_file_name
%xdefine RAND_get_rand_method BORINGSSL_PREFIX %+ _RAND_get_rand_method
%xdefine RAND_keep_random_devices_open BORINGSSL_PREFIX %+ _RAND_keep_random_devices_open
%xdefine RAND_load_file BORINGSSL_PREFIX %+ _RAND_load_file
%xdefine RAND_poll BORINGSSL_PREFIX %+ _RAND_poll
%xdefine RAND_priv_bytes BORINGSSL_PREFIX %+ _RAND_priv_bytes
%xdefine RAND_pseudo_bytes BORINGSSL_PREFIX %+ _RAND_pseudo_bytes
%xdefine RAND_seed BORINGSSL_PREFIX %+ _RAND_seed
%xdefine RAND_set_rand_method BORINGSSL_PREFIX %+ _RAND_set_rand_method
%xdefine RAND_status BORINGSSL_PREFIX %+ _RAND_status
%xdefine RAND_write_file BORINGSSL_PREFIX %+ _RAND_write_file
%xdefine RC4 BORINGSSL_PREFIX %+ _RC4
%xdefine RC4_options BORINGSSL_PREFIX %+ _RC4_options
%xdefine RC4_set_key BORINGSSL_PREFIX %+ _RC4_set_key
%xdefine RFC8032_DOM2_PREFIX BORINGSSL_PREFIX %+ _RFC8032_DOM2_PREFIX
%xdefine RIPEMD160 BORINGSSL_PREFIX %+ _RIPEMD160
%xdefine RIPEMD160_Final BORINGSSL_PREFIX %+ _RIPEMD160_Final
%xdefine RIPEMD160_Init BORINGSSL_PREFIX %+ _RIPEMD160_Init
%xdefine RIPEMD160_Update BORINGSSL_PREFIX %+ _RIPEMD160_Update
%xdefine RSAPrivateKey_dup BORINGSSL_PREFIX %+ _RSAPrivateKey_dup
%xdefine RSAPublicKey_dup BORINGSSL_PREFIX %+ _RSAPublicKey_dup
%xdefine RSASSA_PSS_PARAMS_create BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_create
%xdefine RSASSA_PSS_PARAMS_free BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_free
%xdefine RSASSA_PSS_PARAMS_get BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_get
%xdefine RSASSA_PSS_PARAMS_new BORINGSSL_PREFIX %+ _RSASSA_PSS_PARAMS_new
%xdefine RSASSA_PSS_parse_params BORINGSSL_PREFIX %+ _RSASSA_PSS_parse_params
%xdefine RSAZ_1024_mod_exp_avx2 BORINGSSL_PREFIX %+ _RSAZ_1024_mod_exp_avx2
%xdefine RSAZ_mod_exp_avx512_x2 BORINGSSL_PREFIX %+ _RSAZ_mod_exp_avx512_x2
%xdefine RSA_ALGOR_IDENTIFIER_free BORINGSSL_PREFIX %+ _RSA_ALGOR_IDENTIFIER_free
%xdefine RSA_ALGOR_IDENTIFIER_new BORINGSSL_PREFIX %+ _RSA_ALGOR_IDENTIFIER_new
%xdefine RSA_INTEGER_free BORINGSSL_PREFIX %+ _RSA_INTEGER_free
%xdefine RSA_INTEGER_new BORINGSSL_PREFIX %+ _RSA_INTEGER_new
%xdefine RSA_MGA_IDENTIFIER_free BORINGSSL_PREFIX %+ _RSA_MGA_IDENTIFIER_free
%xdefine RSA_MGA_IDENTIFIER_new BORINGSSL_PREFIX %+ _RSA_MGA_IDENTIFIER_new
%xdefine RSA_PSS_PARAMS_free BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_free
%xdefine RSA_PSS_PARAMS_it BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_it
%xdefine RSA_PSS_PARAMS_new BORINGSSL_PREFIX %+ _RSA_PSS_PARAMS_new
%xdefine RSA_add_pkcs1_prefix BORINGSSL_PREFIX %+ _RSA_add_pkcs1_prefix
%xdefine RSA_bits BORINGSSL_PREFIX %+ _RSA_bits
%xdefine RSA_blinding_off_temp_for_accp_compatibility BORINGSSL_PREFIX %+ _RSA_blinding_off_temp_for_accp_compatibility
%xdefine RSA_blinding_on BORINGSSL_PREFIX %+ _RSA_blinding_on
%xdefine RSA_check_fips BORINGSSL_PREFIX %+ _RSA_check_fips
%xdefine RSA_check_key BORINGSSL_PREFIX %+ _RSA_check_key
%xdefine RSA_decrypt BORINGSSL_PREFIX %+ _RSA_decrypt
%xdefine RSA_encrypt BORINGSSL_PREFIX %+ _RSA_encrypt
%xdefine RSA_flags BORINGSSL_PREFIX %+ _RSA_flags
%xdefine RSA_free BORINGSSL_PREFIX %+ _RSA_free
%xdefine RSA_generate_key BORINGSSL_PREFIX %+ _RSA_generate_key
%xdefine RSA_generate_key_ex BORINGSSL_PREFIX %+ _RSA_generate_key_ex
%xdefine RSA_generate_key_fips BORINGSSL_PREFIX %+ _RSA_generate_key_fips
%xdefine RSA_get0_crt_params BORINGSSL_PREFIX %+ _RSA_get0_crt_params
%xdefine RSA_get0_d BORINGSSL_PREFIX %+ _RSA_get0_d
%xdefine RSA_get0_dmp1 BORINGSSL_PREFIX %+ _RSA_get0_dmp1
%xdefine RSA_get0_dmq1 BORINGSSL_PREFIX %+ _RSA_get0_dmq1
%xdefine RSA_get0_e BORINGSSL_PREFIX %+ _RSA_get0_e
%xdefine RSA_get0_factors BORINGSSL_PREFIX %+ _RSA_get0_factors
%xdefine RSA_get0_iqmp BORINGSSL_PREFIX %+ _RSA_get0_iqmp
%xdefine RSA_get0_key BORINGSSL_PREFIX %+ _RSA_get0_key
%xdefine RSA_get0_n BORINGSSL_PREFIX %+ _RSA_get0_n
%xdefine RSA_get0_p BORINGSSL_PREFIX %+ _RSA_get0_p
%xdefine RSA_get0_pss_params BORINGSSL_PREFIX %+ _RSA_get0_pss_params
%xdefine RSA_get0_q BORINGSSL_PREFIX %+ _RSA_get0_q
%xdefine RSA_get_default_method BORINGSSL_PREFIX %+ _RSA_get_default_method
%xdefine RSA_get_ex_data BORINGSSL_PREFIX %+ _RSA_get_ex_data
%xdefine RSA_get_ex_new_index BORINGSSL_PREFIX %+ _RSA_get_ex_new_index
%xdefine RSA_get_method BORINGSSL_PREFIX %+ _RSA_get_method
%xdefine RSA_is_opaque BORINGSSL_PREFIX %+ _RSA_is_opaque
%xdefine RSA_marshal_private_key BORINGSSL_PREFIX %+ _RSA_marshal_private_key
%xdefine RSA_marshal_public_key BORINGSSL_PREFIX %+ _RSA_marshal_public_key
%xdefine RSA_meth_free BORINGSSL_PREFIX %+ _RSA_meth_free
%xdefine RSA_meth_new BORINGSSL_PREFIX %+ _RSA_meth_new
%xdefine RSA_meth_set0_app_data BORINGSSL_PREFIX %+ _RSA_meth_set0_app_data
%xdefine RSA_meth_set_finish BORINGSSL_PREFIX %+ _RSA_meth_set_finish
%xdefine RSA_meth_set_init BORINGSSL_PREFIX %+ _RSA_meth_set_init
%xdefine RSA_meth_set_priv_dec BORINGSSL_PREFIX %+ _RSA_meth_set_priv_dec
%xdefine RSA_meth_set_priv_enc BORINGSSL_PREFIX %+ _RSA_meth_set_priv_enc
%xdefine RSA_meth_set_pub_dec BORINGSSL_PREFIX %+ _RSA_meth_set_pub_dec
%xdefine RSA_meth_set_pub_enc BORINGSSL_PREFIX %+ _RSA_meth_set_pub_enc
%xdefine RSA_meth_set_sign BORINGSSL_PREFIX %+ _RSA_meth_set_sign
%xdefine RSA_new BORINGSSL_PREFIX %+ _RSA_new
%xdefine RSA_new_method BORINGSSL_PREFIX %+ _RSA_new_method
%xdefine RSA_new_method_no_e BORINGSSL_PREFIX %+ _RSA_new_method_no_e
%xdefine RSA_new_private_key BORINGSSL_PREFIX %+ _RSA_new_private_key
%xdefine RSA_new_private_key_large_e BORINGSSL_PREFIX %+ _RSA_new_private_key_large_e
%xdefine RSA_new_private_key_no_crt BORINGSSL_PREFIX %+ _RSA_new_private_key_no_crt
%xdefine RSA_new_private_key_no_e BORINGSSL_PREFIX %+ _RSA_new_private_key_no_e
%xdefine RSA_new_public_key BORINGSSL_PREFIX %+ _RSA_new_public_key
%xdefine RSA_new_public_key_large_e BORINGSSL_PREFIX %+ _RSA_new_public_key_large_e
%xdefine RSA_padding_add_PKCS1_OAEP BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_OAEP
%xdefine RSA_padding_add_PKCS1_OAEP_mgf1 BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_OAEP_mgf1
%xdefine RSA_padding_add_PKCS1_PSS BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_PSS
%xdefine RSA_padding_add_PKCS1_PSS_mgf1 BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_PSS_mgf1
%xdefine RSA_padding_add_PKCS1_type_1 BORINGSSL_PREFIX %+ _RSA_padding_add_PKCS1_type_1
%xdefine RSA_padding_add_none BORINGSSL_PREFIX %+ _RSA_padding_add_none
%xdefine RSA_padding_check_PKCS1_OAEP_mgf1 BORINGSSL_PREFIX %+ _RSA_padding_check_PKCS1_OAEP_mgf1
%xdefine RSA_padding_check_PKCS1_type_1 BORINGSSL_PREFIX %+ _RSA_padding_check_PKCS1_type_1
%xdefine RSA_parse_private_key BORINGSSL_PREFIX %+ _RSA_parse_private_key
%xdefine RSA_parse_public_key BORINGSSL_PREFIX %+ _RSA_parse_public_key
%xdefine RSA_pkey_ctx_ctrl BORINGSSL_PREFIX %+ _RSA_pkey_ctx_ctrl
%xdefine RSA_print BORINGSSL_PREFIX %+ _RSA_print
%xdefine RSA_print_fp BORINGSSL_PREFIX %+ _RSA_print_fp
%xdefine RSA_private_decrypt BORINGSSL_PREFIX %+ _RSA_private_decrypt
%xdefine RSA_private_encrypt BORINGSSL_PREFIX %+ _RSA_private_encrypt
%xdefine RSA_private_key_from_bytes BORINGSSL_PREFIX %+ _RSA_private_key_from_bytes
%xdefine RSA_private_key_to_bytes BORINGSSL_PREFIX %+ _RSA_private_key_to_bytes
%xdefine RSA_public_decrypt BORINGSSL_PREFIX %+ _RSA_public_decrypt
%xdefine RSA_public_encrypt BORINGSSL_PREFIX %+ _RSA_public_encrypt
%xdefine RSA_public_key_from_bytes BORINGSSL_PREFIX %+ _RSA_public_key_from_bytes
%xdefine RSA_public_key_to_bytes BORINGSSL_PREFIX %+ _RSA_public_key_to_bytes
%xdefine RSA_set0_crt_params BORINGSSL_PREFIX %+ _RSA_set0_crt_params
%xdefine RSA_set0_factors BORINGSSL_PREFIX %+ _RSA_set0_factors
%xdefine RSA_set0_key BORINGSSL_PREFIX %+ _RSA_set0_key
%xdefine RSA_set_ex_data BORINGSSL_PREFIX %+ _RSA_set_ex_data
%xdefine RSA_set_flags BORINGSSL_PREFIX %+ _RSA_set_flags
%xdefine RSA_set_method BORINGSSL_PREFIX %+ _RSA_set_method
%xdefine RSA_sign BORINGSSL_PREFIX %+ _RSA_sign
%xdefine RSA_sign_pss_mgf1 BORINGSSL_PREFIX %+ _RSA_sign_pss_mgf1
%xdefine RSA_sign_raw BORINGSSL_PREFIX %+ _RSA_sign_raw
%xdefine RSA_size BORINGSSL_PREFIX %+ _RSA_size
%xdefine RSA_test_flags BORINGSSL_PREFIX %+ _RSA_test_flags
%xdefine RSA_up_ref BORINGSSL_PREFIX %+ _RSA_up_ref
%xdefine RSA_verify BORINGSSL_PREFIX %+ _RSA_verify
%xdefine RSA_verify_PKCS1_PSS BORINGSSL_PREFIX %+ _RSA_verify_PKCS1_PSS
%xdefine RSA_verify_PKCS1_PSS_mgf1 BORINGSSL_PREFIX %+ _RSA_verify_PKCS1_PSS_mgf1
%xdefine RSA_verify_pss_mgf1 BORINGSSL_PREFIX %+ _RSA_verify_pss_mgf1
%xdefine RSA_verify_raw BORINGSSL_PREFIX %+ _RSA_verify_raw
%xdefine SHA1 BORINGSSL_PREFIX %+ _SHA1
%xdefine SHA1_Final BORINGSSL_PREFIX %+ _SHA1_Final
%xdefine SHA1_Init BORINGSSL_PREFIX %+ _SHA1_Init
%xdefine SHA1_Init_from_state BORINGSSL_PREFIX %+ _SHA1_Init_from_state
%xdefine SHA1_Transform BORINGSSL_PREFIX %+ _SHA1_Transform
%xdefine SHA1_Update BORINGSSL_PREFIX %+ _SHA1_Update
%xdefine SHA1_get_state BORINGSSL_PREFIX %+ _SHA1_get_state
%xdefine SHA224 BORINGSSL_PREFIX %+ _SHA224
%xdefine SHA224_Final BORINGSSL_PREFIX %+ _SHA224_Final
%xdefine SHA224_Init BORINGSSL_PREFIX %+ _SHA224_Init
%xdefine SHA224_Init_from_state BORINGSSL_PREFIX %+ _SHA224_Init_from_state
%xdefine SHA224_Update BORINGSSL_PREFIX %+ _SHA224_Update
%xdefine SHA224_get_state BORINGSSL_PREFIX %+ _SHA224_get_state
%xdefine SHA256 BORINGSSL_PREFIX %+ _SHA256
%xdefine SHA256_Final BORINGSSL_PREFIX %+ _SHA256_Final
%xdefine SHA256_Init BORINGSSL_PREFIX %+ _SHA256_Init
%xdefine SHA256_Init_from_state BORINGSSL_PREFIX %+ _SHA256_Init_from_state
%xdefine SHA256_Transform BORINGSSL_PREFIX %+ _SHA256_Transform
%xdefine SHA256_TransformBlocks BORINGSSL_PREFIX %+ _SHA256_TransformBlocks
%xdefine SHA256_Update BORINGSSL_PREFIX %+ _SHA256_Update
%xdefine SHA256_get_state BORINGSSL_PREFIX %+ _SHA256_get_state
%xdefine SHA384 BORINGSSL_PREFIX %+ _SHA384
%xdefine SHA384_Final BORINGSSL_PREFIX %+ _SHA384_Final
%xdefine SHA384_Init BORINGSSL_PREFIX %+ _SHA384_Init
%xdefine SHA384_Init_from_state BORINGSSL_PREFIX %+ _SHA384_Init_from_state
%xdefine SHA384_Update BORINGSSL_PREFIX %+ _SHA384_Update
%xdefine SHA384_get_state BORINGSSL_PREFIX %+ _SHA384_get_state
%xdefine SHA3_224 BORINGSSL_PREFIX %+ _SHA3_224
%xdefine SHA3_224_Final BORINGSSL_PREFIX %+ _SHA3_224_Final
%xdefine SHA3_224_Init BORINGSSL_PREFIX %+ _SHA3_224_Init
%xdefine SHA3_224_Update BORINGSSL_PREFIX %+ _SHA3_224_Update
%xdefine SHA3_256 BORINGSSL_PREFIX %+ _SHA3_256
%xdefine SHA3_256_Final BORINGSSL_PREFIX %+ _SHA3_256_Final
%xdefine SHA3_256_Init BORINGSSL_PREFIX %+ _SHA3_256_Init
%xdefine SHA3_256_Update BORINGSSL_PREFIX %+ _SHA3_256_Update
%xdefine SHA3_384 BORINGSSL_PREFIX %+ _SHA3_384
%xdefine SHA3_384_Final BORINGSSL_PREFIX %+ _SHA3_384_Final
%xdefine SHA3_384_Init BORINGSSL_PREFIX %+ _SHA3_384_Init
%xdefine SHA3_384_Update BORINGSSL_PREFIX %+ _SHA3_384_Update
%xdefine SHA3_512 BORINGSSL_PREFIX %+ _SHA3_512
%xdefine SHA3_512_Final BORINGSSL_PREFIX %+ _SHA3_512_Final
%xdefine SHA3_512_Init BORINGSSL_PREFIX %+ _SHA3_512_Init
%xdefine SHA3_512_Update BORINGSSL_PREFIX %+ _SHA3_512_Update
%xdefine SHA3_Final BORINGSSL_PREFIX %+ _SHA3_Final
%xdefine SHA3_Init BORINGSSL_PREFIX %+ _SHA3_Init
%xdefine SHA3_Update BORINGSSL_PREFIX %+ _SHA3_Update
%xdefine SHA512 BORINGSSL_PREFIX %+ _SHA512
%xdefine SHA512_224 BORINGSSL_PREFIX %+ _SHA512_224
%xdefine SHA512_224_Final BORINGSSL_PREFIX %+ _SHA512_224_Final
%xdefine SHA512_224_Init BORINGSSL_PREFIX %+ _SHA512_224_Init
%xdefine SHA512_224_Init_from_state BORINGSSL_PREFIX %+ _SHA512_224_Init_from_state
%xdefine SHA512_224_Update BORINGSSL_PREFIX %+ _SHA512_224_Update
%xdefine SHA512_224_get_state BORINGSSL_PREFIX %+ _SHA512_224_get_state
%xdefine SHA512_256 BORINGSSL_PREFIX %+ _SHA512_256
%xdefine SHA512_256_Final BORINGSSL_PREFIX %+ _SHA512_256_Final
%xdefine SHA512_256_Init BORINGSSL_PREFIX %+ _SHA512_256_Init
%xdefine SHA512_256_Init_from_state BORINGSSL_PREFIX %+ _SHA512_256_Init_from_state
%xdefine SHA512_256_Update BORINGSSL_PREFIX %+ _SHA512_256_Update
%xdefine SHA512_256_get_state BORINGSSL_PREFIX %+ _SHA512_256_get_state
%xdefine SHA512_Final BORINGSSL_PREFIX %+ _SHA512_Final
%xdefine SHA512_Init BORINGSSL_PREFIX %+ _SHA512_Init
%xdefine SHA512_Init_from_state BORINGSSL_PREFIX %+ _SHA512_Init_from_state
%xdefine SHA512_Transform BORINGSSL_PREFIX %+ _SHA512_Transform
%xdefine SHA512_Update BORINGSSL_PREFIX %+ _SHA512_Update
%xdefine SHA512_get_state BORINGSSL_PREFIX %+ _SHA512_get_state
%xdefine SHAKE128 BORINGSSL_PREFIX %+ _SHAKE128
%xdefine SHAKE128_Absorb_once_x4 BORINGSSL_PREFIX %+ _SHAKE128_Absorb_once_x4
%xdefine SHAKE128_Init_x4 BORINGSSL_PREFIX %+ _SHAKE128_Init_x4
%xdefine SHAKE128_Squeezeblocks_x4 BORINGSSL_PREFIX %+ _SHAKE128_Squeezeblocks_x4
%xdefine SHAKE256 BORINGSSL_PREFIX %+ _SHAKE256
%xdefine SHAKE256_x4 BORINGSSL_PREFIX %+ _SHAKE256_x4
%xdefine SHAKE_Absorb BORINGSSL_PREFIX %+ _SHAKE_Absorb
%xdefine SHAKE_Final BORINGSSL_PREFIX %+ _SHAKE_Final
%xdefine SHAKE_Init BORINGSSL_PREFIX %+ _SHAKE_Init
%xdefine SHAKE_Squeeze BORINGSSL_PREFIX %+ _SHAKE_Squeeze
%xdefine SIPHASH_24 BORINGSSL_PREFIX %+ _SIPHASH_24
%xdefine SMIME_read_PKCS7 BORINGSSL_PREFIX %+ _SMIME_read_PKCS7
%xdefine SMIME_write_PKCS7 BORINGSSL_PREFIX %+ _SMIME_write_PKCS7
%xdefine SPAKE2_CTX_free BORINGSSL_PREFIX %+ _SPAKE2_CTX_free
%xdefine SPAKE2_CTX_new BORINGSSL_PREFIX %+ _SPAKE2_CTX_new
%xdefine SPAKE2_generate_msg BORINGSSL_PREFIX %+ _SPAKE2_generate_msg
%xdefine SPAKE2_process_msg BORINGSSL_PREFIX %+ _SPAKE2_process_msg
%xdefine SSHKDF BORINGSSL_PREFIX %+ _SSHKDF
%xdefine SSKDF_digest BORINGSSL_PREFIX %+ _SSKDF_digest
%xdefine SSKDF_hmac BORINGSSL_PREFIX %+ _SSKDF_hmac
%xdefine SSLeay BORINGSSL_PREFIX %+ _SSLeay
%xdefine SSLeay_version BORINGSSL_PREFIX %+ _SSLeay_version
%xdefine TRUST_TOKEN_CLIENT_add_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_add_key
%xdefine TRUST_TOKEN_CLIENT_begin_issuance BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_issuance
%xdefine TRUST_TOKEN_CLIENT_begin_issuance_over_message BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_issuance_over_message
%xdefine TRUST_TOKEN_CLIENT_begin_redemption BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_begin_redemption
%xdefine TRUST_TOKEN_CLIENT_finish_issuance BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_finish_issuance
%xdefine TRUST_TOKEN_CLIENT_finish_redemption BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_finish_redemption
%xdefine TRUST_TOKEN_CLIENT_free BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_free
%xdefine TRUST_TOKEN_CLIENT_new BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_new
%xdefine TRUST_TOKEN_CLIENT_set_srr_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_CLIENT_set_srr_key
%xdefine TRUST_TOKEN_ISSUER_add_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_add_key
%xdefine TRUST_TOKEN_ISSUER_free BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_free
%xdefine TRUST_TOKEN_ISSUER_issue BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_issue
%xdefine TRUST_TOKEN_ISSUER_new BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_new
%xdefine TRUST_TOKEN_ISSUER_redeem BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_redeem
%xdefine TRUST_TOKEN_ISSUER_redeem_over_message BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_redeem_over_message
%xdefine TRUST_TOKEN_ISSUER_set_metadata_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_set_metadata_key
%xdefine TRUST_TOKEN_ISSUER_set_srr_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_ISSUER_set_srr_key
%xdefine TRUST_TOKEN_PRETOKEN_free BORINGSSL_PREFIX %+ _TRUST_TOKEN_PRETOKEN_free
%xdefine TRUST_TOKEN_decode_private_metadata BORINGSSL_PREFIX %+ _TRUST_TOKEN_decode_private_metadata
%xdefine TRUST_TOKEN_derive_key_from_secret BORINGSSL_PREFIX %+ _TRUST_TOKEN_derive_key_from_secret
%xdefine TRUST_TOKEN_experiment_v1 BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v1
%xdefine TRUST_TOKEN_experiment_v2_pmb BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v2_pmb
%xdefine TRUST_TOKEN_experiment_v2_voprf BORINGSSL_PREFIX %+ _TRUST_TOKEN_experiment_v2_voprf
%xdefine TRUST_TOKEN_free BORINGSSL_PREFIX %+ _TRUST_TOKEN_free
%xdefine TRUST_TOKEN_generate_key BORINGSSL_PREFIX %+ _TRUST_TOKEN_generate_key
%xdefine TRUST_TOKEN_new BORINGSSL_PREFIX %+ _TRUST_TOKEN_new
%xdefine TRUST_TOKEN_pst_v1_pmb BORINGSSL_PREFIX %+ _TRUST_TOKEN_pst_v1_pmb
%xdefine TRUST_TOKEN_pst_v1_voprf BORINGSSL_PREFIX %+ _TRUST_TOKEN_pst_v1_voprf
%xdefine UI_add_info_string BORINGSSL_PREFIX %+ _UI_add_info_string
%xdefine UI_add_input_string BORINGSSL_PREFIX %+ _UI_add_input_string
%xdefine UI_add_verify_string BORINGSSL_PREFIX %+ _UI_add_verify_string
%xdefine UI_free BORINGSSL_PREFIX %+ _UI_free
%xdefine UI_new BORINGSSL_PREFIX %+ _UI_new
%xdefine UI_process BORINGSSL_PREFIX %+ _UI_process
%xdefine USERNOTICE_free BORINGSSL_PREFIX %+ _USERNOTICE_free
%xdefine USERNOTICE_it BORINGSSL_PREFIX %+ _USERNOTICE_it
%xdefine USERNOTICE_new BORINGSSL_PREFIX %+ _USERNOTICE_new
%xdefine UTF8_getc BORINGSSL_PREFIX %+ _UTF8_getc
%xdefine UTF8_putc BORINGSSL_PREFIX %+ _UTF8_putc
%xdefine X25519 BORINGSSL_PREFIX %+ _X25519
%xdefine X25519_keypair BORINGSSL_PREFIX %+ _X25519_keypair
%xdefine X25519_public_from_private BORINGSSL_PREFIX %+ _X25519_public_from_private
%xdefine X509V3_EXT_CRL_add_nconf BORINGSSL_PREFIX %+ _X509V3_EXT_CRL_add_nconf
%xdefine X509V3_EXT_REQ_add_nconf BORINGSSL_PREFIX %+ _X509V3_EXT_REQ_add_nconf
%xdefine X509V3_EXT_add BORINGSSL_PREFIX %+ _X509V3_EXT_add
%xdefine X509V3_EXT_add_alias BORINGSSL_PREFIX %+ _X509V3_EXT_add_alias
%xdefine X509V3_EXT_add_nconf BORINGSSL_PREFIX %+ _X509V3_EXT_add_nconf
%xdefine X509V3_EXT_add_nconf_sk BORINGSSL_PREFIX %+ _X509V3_EXT_add_nconf_sk
%xdefine X509V3_EXT_conf BORINGSSL_PREFIX %+ _X509V3_EXT_conf
%xdefine X509V3_EXT_conf_nid BORINGSSL_PREFIX %+ _X509V3_EXT_conf_nid
%xdefine X509V3_EXT_d2i BORINGSSL_PREFIX %+ _X509V3_EXT_d2i
%xdefine X509V3_EXT_free BORINGSSL_PREFIX %+ _X509V3_EXT_free
%xdefine X509V3_EXT_get BORINGSSL_PREFIX %+ _X509V3_EXT_get
%xdefine X509V3_EXT_get_nid BORINGSSL_PREFIX %+ _X509V3_EXT_get_nid
%xdefine X509V3_EXT_i2d BORINGSSL_PREFIX %+ _X509V3_EXT_i2d
%xdefine X509V3_EXT_nconf BORINGSSL_PREFIX %+ _X509V3_EXT_nconf
%xdefine X509V3_EXT_nconf_nid BORINGSSL_PREFIX %+ _X509V3_EXT_nconf_nid
%xdefine X509V3_EXT_print BORINGSSL_PREFIX %+ _X509V3_EXT_print
%xdefine X509V3_EXT_print_fp BORINGSSL_PREFIX %+ _X509V3_EXT_print_fp
%xdefine X509V3_NAME_from_section BORINGSSL_PREFIX %+ _X509V3_NAME_from_section
%xdefine X509V3_add1_i2d BORINGSSL_PREFIX %+ _X509V3_add1_i2d
%xdefine X509V3_add_standard_extensions BORINGSSL_PREFIX %+ _X509V3_add_standard_extensions
%xdefine X509V3_add_value BORINGSSL_PREFIX %+ _X509V3_add_value
%xdefine X509V3_add_value_bool BORINGSSL_PREFIX %+ _X509V3_add_value_bool
%xdefine X509V3_add_value_int BORINGSSL_PREFIX %+ _X509V3_add_value_int
%xdefine X509V3_bool_from_string BORINGSSL_PREFIX %+ _X509V3_bool_from_string
%xdefine X509V3_conf_free BORINGSSL_PREFIX %+ _X509V3_conf_free
%xdefine X509V3_extensions_print BORINGSSL_PREFIX %+ _X509V3_extensions_print
%xdefine X509V3_get_d2i BORINGSSL_PREFIX %+ _X509V3_get_d2i
%xdefine X509V3_get_section BORINGSSL_PREFIX %+ _X509V3_get_section
%xdefine X509V3_get_value_bool BORINGSSL_PREFIX %+ _X509V3_get_value_bool
%xdefine X509V3_get_value_int BORINGSSL_PREFIX %+ _X509V3_get_value_int
%xdefine X509V3_parse_list BORINGSSL_PREFIX %+ _X509V3_parse_list
%xdefine X509V3_set_ctx BORINGSSL_PREFIX %+ _X509V3_set_ctx
%xdefine X509V3_set_nconf BORINGSSL_PREFIX %+ _X509V3_set_nconf
%xdefine X509_ALGOR_cmp BORINGSSL_PREFIX %+ _X509_ALGOR_cmp
%xdefine X509_ALGOR_dup BORINGSSL_PREFIX %+ _X509_ALGOR_dup
%xdefine X509_ALGOR_free BORINGSSL_PREFIX %+ _X509_ALGOR_free
%xdefine X509_ALGOR_get0 BORINGSSL_PREFIX %+ _X509_ALGOR_get0
%xdefine X509_ALGOR_it BORINGSSL_PREFIX %+ _X509_ALGOR_it
%xdefine X509_ALGOR_new BORINGSSL_PREFIX %+ _X509_ALGOR_new
%xdefine X509_ALGOR_set0 BORINGSSL_PREFIX %+ _X509_ALGOR_set0
%xdefine X509_ALGOR_set_md BORINGSSL_PREFIX %+ _X509_ALGOR_set_md
%xdefine X509_ATTRIBUTE_count BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_count
%xdefine X509_ATTRIBUTE_create BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create
%xdefine X509_ATTRIBUTE_create_by_NID BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_NID
%xdefine X509_ATTRIBUTE_create_by_OBJ BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_OBJ
%xdefine X509_ATTRIBUTE_create_by_txt BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_create_by_txt
%xdefine X509_ATTRIBUTE_dup BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_dup
%xdefine X509_ATTRIBUTE_free BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_free
%xdefine X509_ATTRIBUTE_get0_data BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_data
%xdefine X509_ATTRIBUTE_get0_object BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_object
%xdefine X509_ATTRIBUTE_get0_type BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_get0_type
%xdefine X509_ATTRIBUTE_it BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_it
%xdefine X509_ATTRIBUTE_new BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_new
%xdefine X509_ATTRIBUTE_set1_data BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_set1_data
%xdefine X509_ATTRIBUTE_set1_object BORINGSSL_PREFIX %+ _X509_ATTRIBUTE_set1_object
%xdefine X509_CERT_AUX_free BORINGSSL_PREFIX %+ _X509_CERT_AUX_free
%xdefine X509_CERT_AUX_it BORINGSSL_PREFIX %+ _X509_CERT_AUX_it
%xdefine X509_CERT_AUX_new BORINGSSL_PREFIX %+ _X509_CERT_AUX_new
%xdefine X509_CERT_AUX_print BORINGSSL_PREFIX %+ _X509_CERT_AUX_print
%xdefine X509_CINF_free BORINGSSL_PREFIX %+ _X509_CINF_free
%xdefine X509_CINF_it BORINGSSL_PREFIX %+ _X509_CINF_it
%xdefine X509_CINF_new BORINGSSL_PREFIX %+ _X509_CINF_new
%xdefine X509_CRL_INFO_free BORINGSSL_PREFIX %+ _X509_CRL_INFO_free
%xdefine X509_CRL_INFO_it BORINGSSL_PREFIX %+ _X509_CRL_INFO_it
%xdefine X509_CRL_INFO_new BORINGSSL_PREFIX %+ _X509_CRL_INFO_new
%xdefine X509_CRL_add0_revoked BORINGSSL_PREFIX %+ _X509_CRL_add0_revoked
%xdefine X509_CRL_add1_ext_i2d BORINGSSL_PREFIX %+ _X509_CRL_add1_ext_i2d
%xdefine X509_CRL_add_ext BORINGSSL_PREFIX %+ _X509_CRL_add_ext
%xdefine X509_CRL_cmp BORINGSSL_PREFIX %+ _X509_CRL_cmp
%xdefine X509_CRL_delete_ext BORINGSSL_PREFIX %+ _X509_CRL_delete_ext
%xdefine X509_CRL_digest BORINGSSL_PREFIX %+ _X509_CRL_digest
%xdefine X509_CRL_dup BORINGSSL_PREFIX %+ _X509_CRL_dup
%xdefine X509_CRL_free BORINGSSL_PREFIX %+ _X509_CRL_free
%xdefine X509_CRL_get0_by_cert BORINGSSL_PREFIX %+ _X509_CRL_get0_by_cert
%xdefine X509_CRL_get0_by_serial BORINGSSL_PREFIX %+ _X509_CRL_get0_by_serial
%xdefine X509_CRL_get0_extensions BORINGSSL_PREFIX %+ _X509_CRL_get0_extensions
%xdefine X509_CRL_get0_lastUpdate BORINGSSL_PREFIX %+ _X509_CRL_get0_lastUpdate
%xdefine X509_CRL_get0_nextUpdate BORINGSSL_PREFIX %+ _X509_CRL_get0_nextUpdate
%xdefine X509_CRL_get0_signature BORINGSSL_PREFIX %+ _X509_CRL_get0_signature
%xdefine X509_CRL_get_REVOKED BORINGSSL_PREFIX %+ _X509_CRL_get_REVOKED
%xdefine X509_CRL_get_ext BORINGSSL_PREFIX %+ _X509_CRL_get_ext
%xdefine X509_CRL_get_ext_by_NID BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_NID
%xdefine X509_CRL_get_ext_by_OBJ BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_OBJ
%xdefine X509_CRL_get_ext_by_critical BORINGSSL_PREFIX %+ _X509_CRL_get_ext_by_critical
%xdefine X509_CRL_get_ext_count BORINGSSL_PREFIX %+ _X509_CRL_get_ext_count
%xdefine X509_CRL_get_ext_d2i BORINGSSL_PREFIX %+ _X509_CRL_get_ext_d2i
%xdefine X509_CRL_get_issuer BORINGSSL_PREFIX %+ _X509_CRL_get_issuer
%xdefine X509_CRL_get_lastUpdate BORINGSSL_PREFIX %+ _X509_CRL_get_lastUpdate
%xdefine X509_CRL_get_nextUpdate BORINGSSL_PREFIX %+ _X509_CRL_get_nextUpdate
%xdefine X509_CRL_get_signature_nid BORINGSSL_PREFIX %+ _X509_CRL_get_signature_nid
%xdefine X509_CRL_get_version BORINGSSL_PREFIX %+ _X509_CRL_get_version
%xdefine X509_CRL_http_nbio BORINGSSL_PREFIX %+ _X509_CRL_http_nbio
%xdefine X509_CRL_it BORINGSSL_PREFIX %+ _X509_CRL_it
%xdefine X509_CRL_match BORINGSSL_PREFIX %+ _X509_CRL_match
%xdefine X509_CRL_new BORINGSSL_PREFIX %+ _X509_CRL_new
%xdefine X509_CRL_print BORINGSSL_PREFIX %+ _X509_CRL_print
%xdefine X509_CRL_print_fp BORINGSSL_PREFIX %+ _X509_CRL_print_fp
%xdefine X509_CRL_set1_lastUpdate BORINGSSL_PREFIX %+ _X509_CRL_set1_lastUpdate
%xdefine X509_CRL_set1_nextUpdate BORINGSSL_PREFIX %+ _X509_CRL_set1_nextUpdate
%xdefine X509_CRL_set1_signature_algo BORINGSSL_PREFIX %+ _X509_CRL_set1_signature_algo
%xdefine X509_CRL_set1_signature_value BORINGSSL_PREFIX %+ _X509_CRL_set1_signature_value
%xdefine X509_CRL_set_issuer_name BORINGSSL_PREFIX %+ _X509_CRL_set_issuer_name
%xdefine X509_CRL_set_version BORINGSSL_PREFIX %+ _X509_CRL_set_version
%xdefine X509_CRL_sign BORINGSSL_PREFIX %+ _X509_CRL_sign
%xdefine X509_CRL_sign_ctx BORINGSSL_PREFIX %+ _X509_CRL_sign_ctx
%xdefine X509_CRL_sort BORINGSSL_PREFIX %+ _X509_CRL_sort
%xdefine X509_CRL_up_ref BORINGSSL_PREFIX %+ _X509_CRL_up_ref
%xdefine X509_CRL_verify BORINGSSL_PREFIX %+ _X509_CRL_verify
%xdefine X509_EXTENSIONS_it BORINGSSL_PREFIX %+ _X509_EXTENSIONS_it
%xdefine X509_EXTENSION_create_by_NID BORINGSSL_PREFIX %+ _X509_EXTENSION_create_by_NID
%xdefine X509_EXTENSION_create_by_OBJ BORINGSSL_PREFIX %+ _X509_EXTENSION_create_by_OBJ
%xdefine X509_EXTENSION_dup BORINGSSL_PREFIX %+ _X509_EXTENSION_dup
%xdefine X509_EXTENSION_free BORINGSSL_PREFIX %+ _X509_EXTENSION_free
%xdefine X509_EXTENSION_get_critical BORINGSSL_PREFIX %+ _X509_EXTENSION_get_critical
%xdefine X509_EXTENSION_get_data BORINGSSL_PREFIX %+ _X509_EXTENSION_get_data
%xdefine X509_EXTENSION_get_object BORINGSSL_PREFIX %+ _X509_EXTENSION_get_object
%xdefine X509_EXTENSION_it BORINGSSL_PREFIX %+ _X509_EXTENSION_it
%xdefine X509_EXTENSION_new BORINGSSL_PREFIX %+ _X509_EXTENSION_new
%xdefine X509_EXTENSION_set_critical BORINGSSL_PREFIX %+ _X509_EXTENSION_set_critical
%xdefine X509_EXTENSION_set_data BORINGSSL_PREFIX %+ _X509_EXTENSION_set_data
%xdefine X509_EXTENSION_set_object BORINGSSL_PREFIX %+ _X509_EXTENSION_set_object
%xdefine X509_INFO_free BORINGSSL_PREFIX %+ _X509_INFO_free
%xdefine X509_LOOKUP_add_dir BORINGSSL_PREFIX %+ _X509_LOOKUP_add_dir
%xdefine X509_LOOKUP_ctrl BORINGSSL_PREFIX %+ _X509_LOOKUP_ctrl
%xdefine X509_LOOKUP_file BORINGSSL_PREFIX %+ _X509_LOOKUP_file
%xdefine X509_LOOKUP_free BORINGSSL_PREFIX %+ _X509_LOOKUP_free
%xdefine X509_LOOKUP_hash_dir BORINGSSL_PREFIX %+ _X509_LOOKUP_hash_dir
%xdefine X509_LOOKUP_load_file BORINGSSL_PREFIX %+ _X509_LOOKUP_load_file
%xdefine X509_NAME_ENTRIES_it BORINGSSL_PREFIX %+ _X509_NAME_ENTRIES_it
%xdefine X509_NAME_ENTRY_create_by_NID BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_NID
%xdefine X509_NAME_ENTRY_create_by_OBJ BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_OBJ
%xdefine X509_NAME_ENTRY_create_by_txt BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_create_by_txt
%xdefine X509_NAME_ENTRY_dup BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_dup
%xdefine X509_NAME_ENTRY_free BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_free
%xdefine X509_NAME_ENTRY_get_data BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_get_data
%xdefine X509_NAME_ENTRY_get_object BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_get_object
%xdefine X509_NAME_ENTRY_it BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_it
%xdefine X509_NAME_ENTRY_new BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_new
%xdefine X509_NAME_ENTRY_set BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set
%xdefine X509_NAME_ENTRY_set_data BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set_data
%xdefine X509_NAME_ENTRY_set_object BORINGSSL_PREFIX %+ _X509_NAME_ENTRY_set_object
%xdefine X509_NAME_INTERNAL_it BORINGSSL_PREFIX %+ _X509_NAME_INTERNAL_it
%xdefine X509_NAME_add_entry BORINGSSL_PREFIX %+ _X509_NAME_add_entry
%xdefine X509_NAME_add_entry_by_NID BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_NID
%xdefine X509_NAME_add_entry_by_OBJ BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_OBJ
%xdefine X509_NAME_add_entry_by_txt BORINGSSL_PREFIX %+ _X509_NAME_add_entry_by_txt
%xdefine X509_NAME_cmp BORINGSSL_PREFIX %+ _X509_NAME_cmp
%xdefine X509_NAME_delete_entry BORINGSSL_PREFIX %+ _X509_NAME_delete_entry
%xdefine X509_NAME_digest BORINGSSL_PREFIX %+ _X509_NAME_digest
%xdefine X509_NAME_dup BORINGSSL_PREFIX %+ _X509_NAME_dup
%xdefine X509_NAME_entry_count BORINGSSL_PREFIX %+ _X509_NAME_entry_count
%xdefine X509_NAME_free BORINGSSL_PREFIX %+ _X509_NAME_free
%xdefine X509_NAME_get0_der BORINGSSL_PREFIX %+ _X509_NAME_get0_der
%xdefine X509_NAME_get_entry BORINGSSL_PREFIX %+ _X509_NAME_get_entry
%xdefine X509_NAME_get_index_by_NID BORINGSSL_PREFIX %+ _X509_NAME_get_index_by_NID
%xdefine X509_NAME_get_index_by_OBJ BORINGSSL_PREFIX %+ _X509_NAME_get_index_by_OBJ
%xdefine X509_NAME_get_text_by_NID BORINGSSL_PREFIX %+ _X509_NAME_get_text_by_NID
%xdefine X509_NAME_get_text_by_OBJ BORINGSSL_PREFIX %+ _X509_NAME_get_text_by_OBJ
%xdefine X509_NAME_hash BORINGSSL_PREFIX %+ _X509_NAME_hash
%xdefine X509_NAME_hash_old BORINGSSL_PREFIX %+ _X509_NAME_hash_old
%xdefine X509_NAME_it BORINGSSL_PREFIX %+ _X509_NAME_it
%xdefine X509_NAME_new BORINGSSL_PREFIX %+ _X509_NAME_new
%xdefine X509_NAME_oneline BORINGSSL_PREFIX %+ _X509_NAME_oneline
%xdefine X509_NAME_print BORINGSSL_PREFIX %+ _X509_NAME_print
%xdefine X509_NAME_print_ex BORINGSSL_PREFIX %+ _X509_NAME_print_ex
%xdefine X509_NAME_print_ex_fp BORINGSSL_PREFIX %+ _X509_NAME_print_ex_fp
%xdefine X509_NAME_set BORINGSSL_PREFIX %+ _X509_NAME_set
%xdefine X509_OBJECT_free BORINGSSL_PREFIX %+ _X509_OBJECT_free
%xdefine X509_OBJECT_free_contents BORINGSSL_PREFIX %+ _X509_OBJECT_free_contents
%xdefine X509_OBJECT_get0_X509 BORINGSSL_PREFIX %+ _X509_OBJECT_get0_X509
%xdefine X509_OBJECT_get0_X509_CRL BORINGSSL_PREFIX %+ _X509_OBJECT_get0_X509_CRL
%xdefine X509_OBJECT_get_type BORINGSSL_PREFIX %+ _X509_OBJECT_get_type
%xdefine X509_OBJECT_new BORINGSSL_PREFIX %+ _X509_OBJECT_new
%xdefine X509_OBJECT_set1_X509 BORINGSSL_PREFIX %+ _X509_OBJECT_set1_X509
%xdefine X509_OBJECT_set1_X509_CRL BORINGSSL_PREFIX %+ _X509_OBJECT_set1_X509_CRL
%xdefine X509_PUBKEY_free BORINGSSL_PREFIX %+ _X509_PUBKEY_free
%xdefine X509_PUBKEY_get BORINGSSL_PREFIX %+ _X509_PUBKEY_get
%xdefine X509_PUBKEY_get0 BORINGSSL_PREFIX %+ _X509_PUBKEY_get0
%xdefine X509_PUBKEY_get0_param BORINGSSL_PREFIX %+ _X509_PUBKEY_get0_param
%xdefine X509_PUBKEY_get0_public_key BORINGSSL_PREFIX %+ _X509_PUBKEY_get0_public_key
%xdefine X509_PUBKEY_it BORINGSSL_PREFIX %+ _X509_PUBKEY_it
%xdefine X509_PUBKEY_new BORINGSSL_PREFIX %+ _X509_PUBKEY_new
%xdefine X509_PUBKEY_set BORINGSSL_PREFIX %+ _X509_PUBKEY_set
%xdefine X509_PUBKEY_set0_param BORINGSSL_PREFIX %+ _X509_PUBKEY_set0_param
%xdefine X509_PURPOSE_get0 BORINGSSL_PREFIX %+ _X509_PURPOSE_get0
%xdefine X509_PURPOSE_get0_name BORINGSSL_PREFIX %+ _X509_PURPOSE_get0_name
%xdefine X509_PURPOSE_get0_sname BORINGSSL_PREFIX %+ _X509_PURPOSE_get0_sname
%xdefine X509_PURPOSE_get_by_id BORINGSSL_PREFIX %+ _X509_PURPOSE_get_by_id
%xdefine X509_PURPOSE_get_by_sname BORINGSSL_PREFIX %+ _X509_PURPOSE_get_by_sname
%xdefine X509_PURPOSE_get_count BORINGSSL_PREFIX %+ _X509_PURPOSE_get_count
%xdefine X509_PURPOSE_get_id BORINGSSL_PREFIX %+ _X509_PURPOSE_get_id
%xdefine X509_PURPOSE_get_trust BORINGSSL_PREFIX %+ _X509_PURPOSE_get_trust
%xdefine X509_PURPOSE_set BORINGSSL_PREFIX %+ _X509_PURPOSE_set
%xdefine X509_REQ_INFO_free BORINGSSL_PREFIX %+ _X509_REQ_INFO_free
%xdefine X509_REQ_INFO_it BORINGSSL_PREFIX %+ _X509_REQ_INFO_it
%xdefine X509_REQ_INFO_new BORINGSSL_PREFIX %+ _X509_REQ_INFO_new
%xdefine X509_REQ_add1_attr BORINGSSL_PREFIX %+ _X509_REQ_add1_attr
%xdefine X509_REQ_add1_attr_by_NID BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_NID
%xdefine X509_REQ_add1_attr_by_OBJ BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_OBJ
%xdefine X509_REQ_add1_attr_by_txt BORINGSSL_PREFIX %+ _X509_REQ_add1_attr_by_txt
%xdefine X509_REQ_add_extensions BORINGSSL_PREFIX %+ _X509_REQ_add_extensions
%xdefine X509_REQ_add_extensions_nid BORINGSSL_PREFIX %+ _X509_REQ_add_extensions_nid
%xdefine X509_REQ_check_private_key BORINGSSL_PREFIX %+ _X509_REQ_check_private_key
%xdefine X509_REQ_delete_attr BORINGSSL_PREFIX %+ _X509_REQ_delete_attr
%xdefine X509_REQ_digest BORINGSSL_PREFIX %+ _X509_REQ_digest
%xdefine X509_REQ_dup BORINGSSL_PREFIX %+ _X509_REQ_dup
%xdefine X509_REQ_extension_nid BORINGSSL_PREFIX %+ _X509_REQ_extension_nid
%xdefine X509_REQ_free BORINGSSL_PREFIX %+ _X509_REQ_free
%xdefine X509_REQ_get0_pubkey BORINGSSL_PREFIX %+ _X509_REQ_get0_pubkey
%xdefine X509_REQ_get0_signature BORINGSSL_PREFIX %+ _X509_REQ_get0_signature
%xdefine X509_REQ_get1_email BORINGSSL_PREFIX %+ _X509_REQ_get1_email
%xdefine X509_REQ_get_attr BORINGSSL_PREFIX %+ _X509_REQ_get_attr
%xdefine X509_REQ_get_attr_by_NID BORINGSSL_PREFIX %+ _X509_REQ_get_attr_by_NID
%xdefine X509_REQ_get_attr_by_OBJ BORINGSSL_PREFIX %+ _X509_REQ_get_attr_by_OBJ
%xdefine X509_REQ_get_attr_count BORINGSSL_PREFIX %+ _X509_REQ_get_attr_count
%xdefine X509_REQ_get_extensions BORINGSSL_PREFIX %+ _X509_REQ_get_extensions
%xdefine X509_REQ_get_pubkey BORINGSSL_PREFIX %+ _X509_REQ_get_pubkey
%xdefine X509_REQ_get_signature_nid BORINGSSL_PREFIX %+ _X509_REQ_get_signature_nid
%xdefine X509_REQ_get_subject_name BORINGSSL_PREFIX %+ _X509_REQ_get_subject_name
%xdefine X509_REQ_get_version BORINGSSL_PREFIX %+ _X509_REQ_get_version
%xdefine X509_REQ_it BORINGSSL_PREFIX %+ _X509_REQ_it
%xdefine X509_REQ_new BORINGSSL_PREFIX %+ _X509_REQ_new
%xdefine X509_REQ_print BORINGSSL_PREFIX %+ _X509_REQ_print
%xdefine X509_REQ_print_ex BORINGSSL_PREFIX %+ _X509_REQ_print_ex
%xdefine X509_REQ_print_fp BORINGSSL_PREFIX %+ _X509_REQ_print_fp
%xdefine X509_REQ_set1_signature_algo BORINGSSL_PREFIX %+ _X509_REQ_set1_signature_algo
%xdefine X509_REQ_set1_signature_value BORINGSSL_PREFIX %+ _X509_REQ_set1_signature_value
%xdefine X509_REQ_set_pubkey BORINGSSL_PREFIX %+ _X509_REQ_set_pubkey
%xdefine X509_REQ_set_subject_name BORINGSSL_PREFIX %+ _X509_REQ_set_subject_name
%xdefine X509_REQ_set_version BORINGSSL_PREFIX %+ _X509_REQ_set_version
%xdefine X509_REQ_sign BORINGSSL_PREFIX %+ _X509_REQ_sign
%xdefine X509_REQ_sign_ctx BORINGSSL_PREFIX %+ _X509_REQ_sign_ctx
%xdefine X509_REQ_verify BORINGSSL_PREFIX %+ _X509_REQ_verify
%xdefine X509_REVOKED_add1_ext_i2d BORINGSSL_PREFIX %+ _X509_REVOKED_add1_ext_i2d
%xdefine X509_REVOKED_add_ext BORINGSSL_PREFIX %+ _X509_REVOKED_add_ext
%xdefine X509_REVOKED_delete_ext BORINGSSL_PREFIX %+ _X509_REVOKED_delete_ext
%xdefine X509_REVOKED_dup BORINGSSL_PREFIX %+ _X509_REVOKED_dup
%xdefine X509_REVOKED_free BORINGSSL_PREFIX %+ _X509_REVOKED_free
%xdefine X509_REVOKED_get0_extensions BORINGSSL_PREFIX %+ _X509_REVOKED_get0_extensions
%xdefine X509_REVOKED_get0_revocationDate BORINGSSL_PREFIX %+ _X509_REVOKED_get0_revocationDate
%xdefine X509_REVOKED_get0_serialNumber BORINGSSL_PREFIX %+ _X509_REVOKED_get0_serialNumber
%xdefine X509_REVOKED_get_ext BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext
%xdefine X509_REVOKED_get_ext_by_NID BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_NID
%xdefine X509_REVOKED_get_ext_by_OBJ BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_OBJ
%xdefine X509_REVOKED_get_ext_by_critical BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_by_critical
%xdefine X509_REVOKED_get_ext_count BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_count
%xdefine X509_REVOKED_get_ext_d2i BORINGSSL_PREFIX %+ _X509_REVOKED_get_ext_d2i
%xdefine X509_REVOKED_it BORINGSSL_PREFIX %+ _X509_REVOKED_it
%xdefine X509_REVOKED_new BORINGSSL_PREFIX %+ _X509_REVOKED_new
%xdefine X509_REVOKED_set_revocationDate BORINGSSL_PREFIX %+ _X509_REVOKED_set_revocationDate
%xdefine X509_REVOKED_set_serialNumber BORINGSSL_PREFIX %+ _X509_REVOKED_set_serialNumber
%xdefine X509_SIG_free BORINGSSL_PREFIX %+ _X509_SIG_free
%xdefine X509_SIG_get0 BORINGSSL_PREFIX %+ _X509_SIG_get0
%xdefine X509_SIG_getm BORINGSSL_PREFIX %+ _X509_SIG_getm
%xdefine X509_SIG_it BORINGSSL_PREFIX %+ _X509_SIG_it
%xdefine X509_SIG_new BORINGSSL_PREFIX %+ _X509_SIG_new
%xdefine X509_STORE_CTX_add_custom_crit_oid BORINGSSL_PREFIX %+ _X509_STORE_CTX_add_custom_crit_oid
%xdefine X509_STORE_CTX_cleanup BORINGSSL_PREFIX %+ _X509_STORE_CTX_cleanup
%xdefine X509_STORE_CTX_free BORINGSSL_PREFIX %+ _X509_STORE_CTX_free
%xdefine X509_STORE_CTX_get0_cert BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_cert
%xdefine X509_STORE_CTX_get0_chain BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_chain
%xdefine X509_STORE_CTX_get0_current_crl BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_current_crl
%xdefine X509_STORE_CTX_get0_current_issuer BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_current_issuer
%xdefine X509_STORE_CTX_get0_param BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_param
%xdefine X509_STORE_CTX_get0_parent_ctx BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_parent_ctx
%xdefine X509_STORE_CTX_get0_store BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_store
%xdefine X509_STORE_CTX_get0_untrusted BORINGSSL_PREFIX %+ _X509_STORE_CTX_get0_untrusted
%xdefine X509_STORE_CTX_get1_certs BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_certs
%xdefine X509_STORE_CTX_get1_chain BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_chain
%xdefine X509_STORE_CTX_get1_crls BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_crls
%xdefine X509_STORE_CTX_get1_issuer BORINGSSL_PREFIX %+ _X509_STORE_CTX_get1_issuer
%xdefine X509_STORE_CTX_get_by_subject BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_by_subject
%xdefine X509_STORE_CTX_get_chain BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_chain
%xdefine X509_STORE_CTX_get_current_cert BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_current_cert
%xdefine X509_STORE_CTX_get_error BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_error
%xdefine X509_STORE_CTX_get_error_depth BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_error_depth
%xdefine X509_STORE_CTX_get_ex_data BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_ex_data
%xdefine X509_STORE_CTX_get_ex_new_index BORINGSSL_PREFIX %+ _X509_STORE_CTX_get_ex_new_index
%xdefine X509_STORE_CTX_init BORINGSSL_PREFIX %+ _X509_STORE_CTX_init
%xdefine X509_STORE_CTX_new BORINGSSL_PREFIX %+ _X509_STORE_CTX_new
%xdefine X509_STORE_CTX_set0_crls BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_crls
%xdefine X509_STORE_CTX_set0_param BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_param
%xdefine X509_STORE_CTX_set0_trusted_stack BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_trusted_stack
%xdefine X509_STORE_CTX_set0_untrusted BORINGSSL_PREFIX %+ _X509_STORE_CTX_set0_untrusted
%xdefine X509_STORE_CTX_set_cert BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_cert
%xdefine X509_STORE_CTX_set_chain BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_chain
%xdefine X509_STORE_CTX_set_default BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_default
%xdefine X509_STORE_CTX_set_depth BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_depth
%xdefine X509_STORE_CTX_set_error BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_error
%xdefine X509_STORE_CTX_set_ex_data BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_ex_data
%xdefine X509_STORE_CTX_set_flags BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_flags
%xdefine X509_STORE_CTX_set_purpose BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_purpose
%xdefine X509_STORE_CTX_set_time BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_time
%xdefine X509_STORE_CTX_set_time_posix BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_time_posix
%xdefine X509_STORE_CTX_set_trust BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_trust
%xdefine X509_STORE_CTX_set_verify_cb BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_verify_cb
%xdefine X509_STORE_CTX_set_verify_crit_oids BORINGSSL_PREFIX %+ _X509_STORE_CTX_set_verify_crit_oids
%xdefine X509_STORE_CTX_trusted_stack BORINGSSL_PREFIX %+ _X509_STORE_CTX_trusted_stack
%xdefine X509_STORE_add_cert BORINGSSL_PREFIX %+ _X509_STORE_add_cert
%xdefine X509_STORE_add_crl BORINGSSL_PREFIX %+ _X509_STORE_add_crl
%xdefine X509_STORE_add_lookup BORINGSSL_PREFIX %+ _X509_STORE_add_lookup
%xdefine X509_STORE_free BORINGSSL_PREFIX %+ _X509_STORE_free
%xdefine X509_STORE_get0_objects BORINGSSL_PREFIX %+ _X509_STORE_get0_objects
%xdefine X509_STORE_get0_param BORINGSSL_PREFIX %+ _X509_STORE_get0_param
%xdefine X509_STORE_get_ex_data BORINGSSL_PREFIX %+ _X509_STORE_get_ex_data
%xdefine X509_STORE_get_ex_new_index BORINGSSL_PREFIX %+ _X509_STORE_get_ex_new_index
%xdefine X509_STORE_get_lookup_crls BORINGSSL_PREFIX %+ _X509_STORE_get_lookup_crls
%xdefine X509_STORE_get_verify_cb BORINGSSL_PREFIX %+ _X509_STORE_get_verify_cb
%xdefine X509_STORE_load_locations BORINGSSL_PREFIX %+ _X509_STORE_load_locations
%xdefine X509_STORE_lock BORINGSSL_PREFIX %+ _X509_STORE_lock
%xdefine X509_STORE_new BORINGSSL_PREFIX %+ _X509_STORE_new
%xdefine X509_STORE_set1_param BORINGSSL_PREFIX %+ _X509_STORE_set1_param
%xdefine X509_STORE_set_check_crl BORINGSSL_PREFIX %+ _X509_STORE_set_check_crl
%xdefine X509_STORE_set_default_paths BORINGSSL_PREFIX %+ _X509_STORE_set_default_paths
%xdefine X509_STORE_set_depth BORINGSSL_PREFIX %+ _X509_STORE_set_depth
%xdefine X509_STORE_set_ex_data BORINGSSL_PREFIX %+ _X509_STORE_set_ex_data
%xdefine X509_STORE_set_flags BORINGSSL_PREFIX %+ _X509_STORE_set_flags
%xdefine X509_STORE_set_get_crl BORINGSSL_PREFIX %+ _X509_STORE_set_get_crl
%xdefine X509_STORE_set_lookup_crls BORINGSSL_PREFIX %+ _X509_STORE_set_lookup_crls
%xdefine X509_STORE_set_purpose BORINGSSL_PREFIX %+ _X509_STORE_set_purpose
%xdefine X509_STORE_set_trust BORINGSSL_PREFIX %+ _X509_STORE_set_trust
%xdefine X509_STORE_set_verify_cb BORINGSSL_PREFIX %+ _X509_STORE_set_verify_cb
%xdefine X509_STORE_unlock BORINGSSL_PREFIX %+ _X509_STORE_unlock
%xdefine X509_STORE_up_ref BORINGSSL_PREFIX %+ _X509_STORE_up_ref
%xdefine X509_TRUST_cleanup BORINGSSL_PREFIX %+ _X509_TRUST_cleanup
%xdefine X509_TRUST_get0 BORINGSSL_PREFIX %+ _X509_TRUST_get0
%xdefine X509_TRUST_get0_name BORINGSSL_PREFIX %+ _X509_TRUST_get0_name
%xdefine X509_TRUST_get_by_id BORINGSSL_PREFIX %+ _X509_TRUST_get_by_id
%xdefine X509_TRUST_get_count BORINGSSL_PREFIX %+ _X509_TRUST_get_count
%xdefine X509_TRUST_get_flags BORINGSSL_PREFIX %+ _X509_TRUST_get_flags
%xdefine X509_TRUST_get_trust BORINGSSL_PREFIX %+ _X509_TRUST_get_trust
%xdefine X509_TRUST_set BORINGSSL_PREFIX %+ _X509_TRUST_set
%xdefine X509_VAL_free BORINGSSL_PREFIX %+ _X509_VAL_free
%xdefine X509_VAL_it BORINGSSL_PREFIX %+ _X509_VAL_it
%xdefine X509_VAL_new BORINGSSL_PREFIX %+ _X509_VAL_new
%xdefine X509_VERIFY_PARAM_add0_policy BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_add0_policy
%xdefine X509_VERIFY_PARAM_add1_host BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_add1_host
%xdefine X509_VERIFY_PARAM_clear_flags BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_clear_flags
%xdefine X509_VERIFY_PARAM_disable_ec_key_explicit_params BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_disable_ec_key_explicit_params
%xdefine X509_VERIFY_PARAM_enable_ec_key_explicit_params BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_enable_ec_key_explicit_params
%xdefine X509_VERIFY_PARAM_free BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_free
%xdefine X509_VERIFY_PARAM_get_depth BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_depth
%xdefine X509_VERIFY_PARAM_get_flags BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_flags
%xdefine X509_VERIFY_PARAM_get_hostflags BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_get_hostflags
%xdefine X509_VERIFY_PARAM_inherit BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_inherit
%xdefine X509_VERIFY_PARAM_lookup BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_lookup
%xdefine X509_VERIFY_PARAM_new BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_new
%xdefine X509_VERIFY_PARAM_set1 BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1
%xdefine X509_VERIFY_PARAM_set1_email BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_email
%xdefine X509_VERIFY_PARAM_set1_host BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_host
%xdefine X509_VERIFY_PARAM_set1_ip BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_ip
%xdefine X509_VERIFY_PARAM_set1_ip_asc BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_ip_asc
%xdefine X509_VERIFY_PARAM_set1_policies BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set1_policies
%xdefine X509_VERIFY_PARAM_set_depth BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_depth
%xdefine X509_VERIFY_PARAM_set_flags BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_flags
%xdefine X509_VERIFY_PARAM_set_hostflags BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_hostflags
%xdefine X509_VERIFY_PARAM_set_purpose BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_purpose
%xdefine X509_VERIFY_PARAM_set_time BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_time
%xdefine X509_VERIFY_PARAM_set_time_posix BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_time_posix
%xdefine X509_VERIFY_PARAM_set_trust BORINGSSL_PREFIX %+ _X509_VERIFY_PARAM_set_trust
%xdefine X509_add1_ext_i2d BORINGSSL_PREFIX %+ _X509_add1_ext_i2d
%xdefine X509_add1_reject_object BORINGSSL_PREFIX %+ _X509_add1_reject_object
%xdefine X509_add1_trust_object BORINGSSL_PREFIX %+ _X509_add1_trust_object
%xdefine X509_add_ext BORINGSSL_PREFIX %+ _X509_add_ext
%xdefine X509_alias_get0 BORINGSSL_PREFIX %+ _X509_alias_get0
%xdefine X509_alias_set1 BORINGSSL_PREFIX %+ _X509_alias_set1
%xdefine X509_chain_up_ref BORINGSSL_PREFIX %+ _X509_chain_up_ref
%xdefine X509_check_akid BORINGSSL_PREFIX %+ _X509_check_akid
%xdefine X509_check_ca BORINGSSL_PREFIX %+ _X509_check_ca
%xdefine X509_check_email BORINGSSL_PREFIX %+ _X509_check_email
%xdefine X509_check_host BORINGSSL_PREFIX %+ _X509_check_host
%xdefine X509_check_ip BORINGSSL_PREFIX %+ _X509_check_ip
%xdefine X509_check_ip_asc BORINGSSL_PREFIX %+ _X509_check_ip_asc
%xdefine X509_check_issued BORINGSSL_PREFIX %+ _X509_check_issued
%xdefine X509_check_private_key BORINGSSL_PREFIX %+ _X509_check_private_key
%xdefine X509_check_purpose BORINGSSL_PREFIX %+ _X509_check_purpose
%xdefine X509_check_trust BORINGSSL_PREFIX %+ _X509_check_trust
%xdefine X509_cmp BORINGSSL_PREFIX %+ _X509_cmp
%xdefine X509_cmp_current_time BORINGSSL_PREFIX %+ _X509_cmp_current_time
%xdefine X509_cmp_time BORINGSSL_PREFIX %+ _X509_cmp_time
%xdefine X509_cmp_time_posix BORINGSSL_PREFIX %+ _X509_cmp_time_posix
%xdefine X509_delete_ext BORINGSSL_PREFIX %+ _X509_delete_ext
%xdefine X509_digest BORINGSSL_PREFIX %+ _X509_digest
%xdefine X509_dup BORINGSSL_PREFIX %+ _X509_dup
%xdefine X509_email_free BORINGSSL_PREFIX %+ _X509_email_free
%xdefine X509_find_by_issuer_and_serial BORINGSSL_PREFIX %+ _X509_find_by_issuer_and_serial
%xdefine X509_find_by_subject BORINGSSL_PREFIX %+ _X509_find_by_subject
%xdefine X509_free BORINGSSL_PREFIX %+ _X509_free
%xdefine X509_get0_authority_issuer BORINGSSL_PREFIX %+ _X509_get0_authority_issuer
%xdefine X509_get0_authority_key_id BORINGSSL_PREFIX %+ _X509_get0_authority_key_id
%xdefine X509_get0_authority_serial BORINGSSL_PREFIX %+ _X509_get0_authority_serial
%xdefine X509_get0_extensions BORINGSSL_PREFIX %+ _X509_get0_extensions
%xdefine X509_get0_notAfter BORINGSSL_PREFIX %+ _X509_get0_notAfter
%xdefine X509_get0_notBefore BORINGSSL_PREFIX %+ _X509_get0_notBefore
%xdefine X509_get0_pubkey BORINGSSL_PREFIX %+ _X509_get0_pubkey
%xdefine X509_get0_pubkey_bitstr BORINGSSL_PREFIX %+ _X509_get0_pubkey_bitstr
%xdefine X509_get0_serialNumber BORINGSSL_PREFIX %+ _X509_get0_serialNumber
%xdefine X509_get0_signature BORINGSSL_PREFIX %+ _X509_get0_signature
%xdefine X509_get0_subject_key_id BORINGSSL_PREFIX %+ _X509_get0_subject_key_id
%xdefine X509_get0_tbs_sigalg BORINGSSL_PREFIX %+ _X509_get0_tbs_sigalg
%xdefine X509_get0_uids BORINGSSL_PREFIX %+ _X509_get0_uids
%xdefine X509_get1_email BORINGSSL_PREFIX %+ _X509_get1_email
%xdefine X509_get1_ocsp BORINGSSL_PREFIX %+ _X509_get1_ocsp
%xdefine X509_get_X509_PUBKEY BORINGSSL_PREFIX %+ _X509_get_X509_PUBKEY
%xdefine X509_get_default_cert_area BORINGSSL_PREFIX %+ _X509_get_default_cert_area
%xdefine X509_get_default_cert_dir BORINGSSL_PREFIX %+ _X509_get_default_cert_dir
%xdefine X509_get_default_cert_dir_env BORINGSSL_PREFIX %+ _X509_get_default_cert_dir_env
%xdefine X509_get_default_cert_file BORINGSSL_PREFIX %+ _X509_get_default_cert_file
%xdefine X509_get_default_cert_file_env BORINGSSL_PREFIX %+ _X509_get_default_cert_file_env
%xdefine X509_get_default_private_dir BORINGSSL_PREFIX %+ _X509_get_default_private_dir
%xdefine X509_get_ex_data BORINGSSL_PREFIX %+ _X509_get_ex_data
%xdefine X509_get_ex_new_index BORINGSSL_PREFIX %+ _X509_get_ex_new_index
%xdefine X509_get_ext BORINGSSL_PREFIX %+ _X509_get_ext
%xdefine X509_get_ext_by_NID BORINGSSL_PREFIX %+ _X509_get_ext_by_NID
%xdefine X509_get_ext_by_OBJ BORINGSSL_PREFIX %+ _X509_get_ext_by_OBJ
%xdefine X509_get_ext_by_critical BORINGSSL_PREFIX %+ _X509_get_ext_by_critical
%xdefine X509_get_ext_count BORINGSSL_PREFIX %+ _X509_get_ext_count
%xdefine X509_get_ext_d2i BORINGSSL_PREFIX %+ _X509_get_ext_d2i
%xdefine X509_get_extended_key_usage BORINGSSL_PREFIX %+ _X509_get_extended_key_usage
%xdefine X509_get_extension_flags BORINGSSL_PREFIX %+ _X509_get_extension_flags
%xdefine X509_get_issuer_name BORINGSSL_PREFIX %+ _X509_get_issuer_name
%xdefine X509_get_key_usage BORINGSSL_PREFIX %+ _X509_get_key_usage
%xdefine X509_get_notAfter BORINGSSL_PREFIX %+ _X509_get_notAfter
%xdefine X509_get_notBefore BORINGSSL_PREFIX %+ _X509_get_notBefore
%xdefine X509_get_pathlen BORINGSSL_PREFIX %+ _X509_get_pathlen
%xdefine X509_get_pubkey BORINGSSL_PREFIX %+ _X509_get_pubkey
%xdefine X509_get_serialNumber BORINGSSL_PREFIX %+ _X509_get_serialNumber
%xdefine X509_get_signature_info BORINGSSL_PREFIX %+ _X509_get_signature_info
%xdefine X509_get_signature_nid BORINGSSL_PREFIX %+ _X509_get_signature_nid
%xdefine X509_get_subject_name BORINGSSL_PREFIX %+ _X509_get_subject_name
%xdefine X509_get_version BORINGSSL_PREFIX %+ _X509_get_version
%xdefine X509_getm_notAfter BORINGSSL_PREFIX %+ _X509_getm_notAfter
%xdefine X509_getm_notBefore BORINGSSL_PREFIX %+ _X509_getm_notBefore
%xdefine X509_gmtime_adj BORINGSSL_PREFIX %+ _X509_gmtime_adj
%xdefine X509_issuer_name_cmp BORINGSSL_PREFIX %+ _X509_issuer_name_cmp
%xdefine X509_issuer_name_hash BORINGSSL_PREFIX %+ _X509_issuer_name_hash
%xdefine X509_issuer_name_hash_old BORINGSSL_PREFIX %+ _X509_issuer_name_hash_old
%xdefine X509_it BORINGSSL_PREFIX %+ _X509_it
%xdefine X509_keyid_get0 BORINGSSL_PREFIX %+ _X509_keyid_get0
%xdefine X509_keyid_set1 BORINGSSL_PREFIX %+ _X509_keyid_set1
%xdefine X509_load_cert_crl_file BORINGSSL_PREFIX %+ _X509_load_cert_crl_file
%xdefine X509_load_cert_file BORINGSSL_PREFIX %+ _X509_load_cert_file
%xdefine X509_load_crl_file BORINGSSL_PREFIX %+ _X509_load_crl_file
%xdefine X509_new BORINGSSL_PREFIX %+ _X509_new
%xdefine X509_parse_from_buffer BORINGSSL_PREFIX %+ _X509_parse_from_buffer
%xdefine X509_policy_check BORINGSSL_PREFIX %+ _X509_policy_check
%xdefine X509_print BORINGSSL_PREFIX %+ _X509_print
%xdefine X509_print_ex BORINGSSL_PREFIX %+ _X509_print_ex
%xdefine X509_print_ex_fp BORINGSSL_PREFIX %+ _X509_print_ex_fp
%xdefine X509_print_fp BORINGSSL_PREFIX %+ _X509_print_fp
%xdefine X509_pubkey_digest BORINGSSL_PREFIX %+ _X509_pubkey_digest
%xdefine X509_reject_clear BORINGSSL_PREFIX %+ _X509_reject_clear
%xdefine X509_set1_notAfter BORINGSSL_PREFIX %+ _X509_set1_notAfter
%xdefine X509_set1_notBefore BORINGSSL_PREFIX %+ _X509_set1_notBefore
%xdefine X509_set1_signature_algo BORINGSSL_PREFIX %+ _X509_set1_signature_algo
%xdefine X509_set1_signature_value BORINGSSL_PREFIX %+ _X509_set1_signature_value
%xdefine X509_set_ex_data BORINGSSL_PREFIX %+ _X509_set_ex_data
%xdefine X509_set_issuer_name BORINGSSL_PREFIX %+ _X509_set_issuer_name
%xdefine X509_set_notAfter BORINGSSL_PREFIX %+ _X509_set_notAfter
%xdefine X509_set_notBefore BORINGSSL_PREFIX %+ _X509_set_notBefore
%xdefine X509_set_pubkey BORINGSSL_PREFIX %+ _X509_set_pubkey
%xdefine X509_set_serialNumber BORINGSSL_PREFIX %+ _X509_set_serialNumber
%xdefine X509_set_subject_name BORINGSSL_PREFIX %+ _X509_set_subject_name
%xdefine X509_set_version BORINGSSL_PREFIX %+ _X509_set_version
%xdefine X509_sign BORINGSSL_PREFIX %+ _X509_sign
%xdefine X509_sign_ctx BORINGSSL_PREFIX %+ _X509_sign_ctx
%xdefine X509_signature_dump BORINGSSL_PREFIX %+ _X509_signature_dump
%xdefine X509_signature_print BORINGSSL_PREFIX %+ _X509_signature_print
%xdefine X509_subject_name_cmp BORINGSSL_PREFIX %+ _X509_subject_name_cmp
%xdefine X509_subject_name_hash BORINGSSL_PREFIX %+ _X509_subject_name_hash
%xdefine X509_subject_name_hash_old BORINGSSL_PREFIX %+ _X509_subject_name_hash_old
%xdefine X509_supported_extension BORINGSSL_PREFIX %+ _X509_supported_extension
%xdefine X509_time_adj BORINGSSL_PREFIX %+ _X509_time_adj
%xdefine X509_time_adj_ex BORINGSSL_PREFIX %+ _X509_time_adj_ex
%xdefine X509_trust_clear BORINGSSL_PREFIX %+ _X509_trust_clear
%xdefine X509_up_ref BORINGSSL_PREFIX %+ _X509_up_ref
%xdefine X509_verify BORINGSSL_PREFIX %+ _X509_verify
%xdefine X509_verify_cert BORINGSSL_PREFIX %+ _X509_verify_cert
%xdefine X509_verify_cert_error_string BORINGSSL_PREFIX %+ _X509_verify_cert_error_string
%xdefine X509v3_add_ext BORINGSSL_PREFIX %+ _X509v3_add_ext
%xdefine X509v3_delete_ext BORINGSSL_PREFIX %+ _X509v3_delete_ext
%xdefine X509v3_get_ext BORINGSSL_PREFIX %+ _X509v3_get_ext
%xdefine X509v3_get_ext_by_NID BORINGSSL_PREFIX %+ _X509v3_get_ext_by_NID
%xdefine X509v3_get_ext_by_OBJ BORINGSSL_PREFIX %+ _X509v3_get_ext_by_OBJ
%xdefine X509v3_get_ext_by_critical BORINGSSL_PREFIX %+ _X509v3_get_ext_by_critical
%xdefine X509v3_get_ext_count BORINGSSL_PREFIX %+ _X509v3_get_ext_count
%xdefine __local_stdio_printf_options BORINGSSL_PREFIX %+ ___local_stdio_printf_options
%xdefine __local_stdio_scanf_options BORINGSSL_PREFIX %+ ___local_stdio_scanf_options
%xdefine a2i_IPADDRESS BORINGSSL_PREFIX %+ _a2i_IPADDRESS
%xdefine a2i_IPADDRESS_NC BORINGSSL_PREFIX %+ _a2i_IPADDRESS_NC
%xdefine abi_test_bad_unwind_epilog BORINGSSL_PREFIX %+ _abi_test_bad_unwind_epilog
%xdefine abi_test_bad_unwind_temporary BORINGSSL_PREFIX %+ _abi_test_bad_unwind_temporary
%xdefine abi_test_bad_unwind_wrong_register BORINGSSL_PREFIX %+ _abi_test_bad_unwind_wrong_register
%xdefine abi_test_clobber_cr0 BORINGSSL_PREFIX %+ _abi_test_clobber_cr0
%xdefine abi_test_clobber_cr1 BORINGSSL_PREFIX %+ _abi_test_clobber_cr1
%xdefine abi_test_clobber_cr2 BORINGSSL_PREFIX %+ _abi_test_clobber_cr2
%xdefine abi_test_clobber_cr3 BORINGSSL_PREFIX %+ _abi_test_clobber_cr3
%xdefine abi_test_clobber_cr4 BORINGSSL_PREFIX %+ _abi_test_clobber_cr4
%xdefine abi_test_clobber_cr5 BORINGSSL_PREFIX %+ _abi_test_clobber_cr5
%xdefine abi_test_clobber_cr6 BORINGSSL_PREFIX %+ _abi_test_clobber_cr6
%xdefine abi_test_clobber_cr7 BORINGSSL_PREFIX %+ _abi_test_clobber_cr7
%xdefine abi_test_clobber_ctr BORINGSSL_PREFIX %+ _abi_test_clobber_ctr
%xdefine abi_test_clobber_d0 BORINGSSL_PREFIX %+ _abi_test_clobber_d0
%xdefine abi_test_clobber_d1 BORINGSSL_PREFIX %+ _abi_test_clobber_d1
%xdefine abi_test_clobber_d10 BORINGSSL_PREFIX %+ _abi_test_clobber_d10
%xdefine abi_test_clobber_d11 BORINGSSL_PREFIX %+ _abi_test_clobber_d11
%xdefine abi_test_clobber_d12 BORINGSSL_PREFIX %+ _abi_test_clobber_d12
%xdefine abi_test_clobber_d13 BORINGSSL_PREFIX %+ _abi_test_clobber_d13
%xdefine abi_test_clobber_d14 BORINGSSL_PREFIX %+ _abi_test_clobber_d14
%xdefine abi_test_clobber_d15 BORINGSSL_PREFIX %+ _abi_test_clobber_d15
%xdefine abi_test_clobber_d16 BORINGSSL_PREFIX %+ _abi_test_clobber_d16
%xdefine abi_test_clobber_d17 BORINGSSL_PREFIX %+ _abi_test_clobber_d17
%xdefine abi_test_clobber_d18 BORINGSSL_PREFIX %+ _abi_test_clobber_d18
%xdefine abi_test_clobber_d19 BORINGSSL_PREFIX %+ _abi_test_clobber_d19
%xdefine abi_test_clobber_d2 BORINGSSL_PREFIX %+ _abi_test_clobber_d2
%xdefine abi_test_clobber_d20 BORINGSSL_PREFIX %+ _abi_test_clobber_d20
%xdefine abi_test_clobber_d21 BORINGSSL_PREFIX %+ _abi_test_clobber_d21
%xdefine abi_test_clobber_d22 BORINGSSL_PREFIX %+ _abi_test_clobber_d22
%xdefine abi_test_clobber_d23 BORINGSSL_PREFIX %+ _abi_test_clobber_d23
%xdefine abi_test_clobber_d24 BORINGSSL_PREFIX %+ _abi_test_clobber_d24
%xdefine abi_test_clobber_d25 BORINGSSL_PREFIX %+ _abi_test_clobber_d25
%xdefine abi_test_clobber_d26 BORINGSSL_PREFIX %+ _abi_test_clobber_d26
%xdefine abi_test_clobber_d27 BORINGSSL_PREFIX %+ _abi_test_clobber_d27
%xdefine abi_test_clobber_d28 BORINGSSL_PREFIX %+ _abi_test_clobber_d28
%xdefine abi_test_clobber_d29 BORINGSSL_PREFIX %+ _abi_test_clobber_d29
%xdefine abi_test_clobber_d3 BORINGSSL_PREFIX %+ _abi_test_clobber_d3
%xdefine abi_test_clobber_d30 BORINGSSL_PREFIX %+ _abi_test_clobber_d30
%xdefine abi_test_clobber_d31 BORINGSSL_PREFIX %+ _abi_test_clobber_d31
%xdefine abi_test_clobber_d4 BORINGSSL_PREFIX %+ _abi_test_clobber_d4
%xdefine abi_test_clobber_d5 BORINGSSL_PREFIX %+ _abi_test_clobber_d5
%xdefine abi_test_clobber_d6 BORINGSSL_PREFIX %+ _abi_test_clobber_d6
%xdefine abi_test_clobber_d7 BORINGSSL_PREFIX %+ _abi_test_clobber_d7
%xdefine abi_test_clobber_d8 BORINGSSL_PREFIX %+ _abi_test_clobber_d8
%xdefine abi_test_clobber_d9 BORINGSSL_PREFIX %+ _abi_test_clobber_d9
%xdefine abi_test_clobber_eax BORINGSSL_PREFIX %+ _abi_test_clobber_eax
%xdefine abi_test_clobber_ebp BORINGSSL_PREFIX %+ _abi_test_clobber_ebp
%xdefine abi_test_clobber_ebx BORINGSSL_PREFIX %+ _abi_test_clobber_ebx
%xdefine abi_test_clobber_ecx BORINGSSL_PREFIX %+ _abi_test_clobber_ecx
%xdefine abi_test_clobber_edi BORINGSSL_PREFIX %+ _abi_test_clobber_edi
%xdefine abi_test_clobber_edx BORINGSSL_PREFIX %+ _abi_test_clobber_edx
%xdefine abi_test_clobber_esi BORINGSSL_PREFIX %+ _abi_test_clobber_esi
%xdefine abi_test_clobber_f0 BORINGSSL_PREFIX %+ _abi_test_clobber_f0
%xdefine abi_test_clobber_f1 BORINGSSL_PREFIX %+ _abi_test_clobber_f1
%xdefine abi_test_clobber_f10 BORINGSSL_PREFIX %+ _abi_test_clobber_f10
%xdefine abi_test_clobber_f11 BORINGSSL_PREFIX %+ _abi_test_clobber_f11
%xdefine abi_test_clobber_f12 BORINGSSL_PREFIX %+ _abi_test_clobber_f12
%xdefine abi_test_clobber_f13 BORINGSSL_PREFIX %+ _abi_test_clobber_f13
%xdefine abi_test_clobber_f14 BORINGSSL_PREFIX %+ _abi_test_clobber_f14
%xdefine abi_test_clobber_f15 BORINGSSL_PREFIX %+ _abi_test_clobber_f15
%xdefine abi_test_clobber_f16 BORINGSSL_PREFIX %+ _abi_test_clobber_f16
%xdefine abi_test_clobber_f17 BORINGSSL_PREFIX %+ _abi_test_clobber_f17
%xdefine abi_test_clobber_f18 BORINGSSL_PREFIX %+ _abi_test_clobber_f18
%xdefine abi_test_clobber_f19 BORINGSSL_PREFIX %+ _abi_test_clobber_f19
%xdefine abi_test_clobber_f2 BORINGSSL_PREFIX %+ _abi_test_clobber_f2
%xdefine abi_test_clobber_f20 BORINGSSL_PREFIX %+ _abi_test_clobber_f20
%xdefine abi_test_clobber_f21 BORINGSSL_PREFIX %+ _abi_test_clobber_f21
%xdefine abi_test_clobber_f22 BORINGSSL_PREFIX %+ _abi_test_clobber_f22
%xdefine abi_test_clobber_f23 BORINGSSL_PREFIX %+ _abi_test_clobber_f23
%xdefine abi_test_clobber_f24 BORINGSSL_PREFIX %+ _abi_test_clobber_f24
%xdefine abi_test_clobber_f25 BORINGSSL_PREFIX %+ _abi_test_clobber_f25
%xdefine abi_test_clobber_f26 BORINGSSL_PREFIX %+ _abi_test_clobber_f26
%xdefine abi_test_clobber_f27 BORINGSSL_PREFIX %+ _abi_test_clobber_f27
%xdefine abi_test_clobber_f28 BORINGSSL_PREFIX %+ _abi_test_clobber_f28
%xdefine abi_test_clobber_f29 BORINGSSL_PREFIX %+ _abi_test_clobber_f29
%xdefine abi_test_clobber_f3 BORINGSSL_PREFIX %+ _abi_test_clobber_f3
%xdefine abi_test_clobber_f30 BORINGSSL_PREFIX %+ _abi_test_clobber_f30
%xdefine abi_test_clobber_f31 BORINGSSL_PREFIX %+ _abi_test_clobber_f31
%xdefine abi_test_clobber_f4 BORINGSSL_PREFIX %+ _abi_test_clobber_f4
%xdefine abi_test_clobber_f5 BORINGSSL_PREFIX %+ _abi_test_clobber_f5
%xdefine abi_test_clobber_f6 BORINGSSL_PREFIX %+ _abi_test_clobber_f6
%xdefine abi_test_clobber_f7 BORINGSSL_PREFIX %+ _abi_test_clobber_f7
%xdefine abi_test_clobber_f8 BORINGSSL_PREFIX %+ _abi_test_clobber_f8
%xdefine abi_test_clobber_f9 BORINGSSL_PREFIX %+ _abi_test_clobber_f9
%xdefine abi_test_clobber_lr BORINGSSL_PREFIX %+ _abi_test_clobber_lr
%xdefine abi_test_clobber_r0 BORINGSSL_PREFIX %+ _abi_test_clobber_r0
%xdefine abi_test_clobber_r1 BORINGSSL_PREFIX %+ _abi_test_clobber_r1
%xdefine abi_test_clobber_r10 BORINGSSL_PREFIX %+ _abi_test_clobber_r10
%xdefine abi_test_clobber_r11 BORINGSSL_PREFIX %+ _abi_test_clobber_r11
%xdefine abi_test_clobber_r12 BORINGSSL_PREFIX %+ _abi_test_clobber_r12
%xdefine abi_test_clobber_r13 BORINGSSL_PREFIX %+ _abi_test_clobber_r13
%xdefine abi_test_clobber_r14 BORINGSSL_PREFIX %+ _abi_test_clobber_r14
%xdefine abi_test_clobber_r15 BORINGSSL_PREFIX %+ _abi_test_clobber_r15
%xdefine abi_test_clobber_r16 BORINGSSL_PREFIX %+ _abi_test_clobber_r16
%xdefine abi_test_clobber_r17 BORINGSSL_PREFIX %+ _abi_test_clobber_r17
%xdefine abi_test_clobber_r18 BORINGSSL_PREFIX %+ _abi_test_clobber_r18
%xdefine abi_test_clobber_r19 BORINGSSL_PREFIX %+ _abi_test_clobber_r19
%xdefine abi_test_clobber_r2 BORINGSSL_PREFIX %+ _abi_test_clobber_r2
%xdefine abi_test_clobber_r20 BORINGSSL_PREFIX %+ _abi_test_clobber_r20
%xdefine abi_test_clobber_r21 BORINGSSL_PREFIX %+ _abi_test_clobber_r21
%xdefine abi_test_clobber_r22 BORINGSSL_PREFIX %+ _abi_test_clobber_r22
%xdefine abi_test_clobber_r23 BORINGSSL_PREFIX %+ _abi_test_clobber_r23
%xdefine abi_test_clobber_r24 BORINGSSL_PREFIX %+ _abi_test_clobber_r24
%xdefine abi_test_clobber_r25 BORINGSSL_PREFIX %+ _abi_test_clobber_r25
%xdefine abi_test_clobber_r26 BORINGSSL_PREFIX %+ _abi_test_clobber_r26
%xdefine abi_test_clobber_r27 BORINGSSL_PREFIX %+ _abi_test_clobber_r27
%xdefine abi_test_clobber_r28 BORINGSSL_PREFIX %+ _abi_test_clobber_r28
%xdefine abi_test_clobber_r29 BORINGSSL_PREFIX %+ _abi_test_clobber_r29
%xdefine abi_test_clobber_r3 BORINGSSL_PREFIX %+ _abi_test_clobber_r3
%xdefine abi_test_clobber_r30 BORINGSSL_PREFIX %+ _abi_test_clobber_r30
%xdefine abi_test_clobber_r31 BORINGSSL_PREFIX %+ _abi_test_clobber_r31
%xdefine abi_test_clobber_r4 BORINGSSL_PREFIX %+ _abi_test_clobber_r4
%xdefine abi_test_clobber_r5 BORINGSSL_PREFIX %+ _abi_test_clobber_r5
%xdefine abi_test_clobber_r6 BORINGSSL_PREFIX %+ _abi_test_clobber_r6
%xdefine abi_test_clobber_r7 BORINGSSL_PREFIX %+ _abi_test_clobber_r7
%xdefine abi_test_clobber_r8 BORINGSSL_PREFIX %+ _abi_test_clobber_r8
%xdefine abi_test_clobber_r9 BORINGSSL_PREFIX %+ _abi_test_clobber_r9
%xdefine abi_test_clobber_rax BORINGSSL_PREFIX %+ _abi_test_clobber_rax
%xdefine abi_test_clobber_rbp BORINGSSL_PREFIX %+ _abi_test_clobber_rbp
%xdefine abi_test_clobber_rbx BORINGSSL_PREFIX %+ _abi_test_clobber_rbx
%xdefine abi_test_clobber_rcx BORINGSSL_PREFIX %+ _abi_test_clobber_rcx
%xdefine abi_test_clobber_rdi BORINGSSL_PREFIX %+ _abi_test_clobber_rdi
%xdefine abi_test_clobber_rdx BORINGSSL_PREFIX %+ _abi_test_clobber_rdx
%xdefine abi_test_clobber_rsi BORINGSSL_PREFIX %+ _abi_test_clobber_rsi
%xdefine abi_test_clobber_v0 BORINGSSL_PREFIX %+ _abi_test_clobber_v0
%xdefine abi_test_clobber_v1 BORINGSSL_PREFIX %+ _abi_test_clobber_v1
%xdefine abi_test_clobber_v10 BORINGSSL_PREFIX %+ _abi_test_clobber_v10
%xdefine abi_test_clobber_v10_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v10_upper
%xdefine abi_test_clobber_v11 BORINGSSL_PREFIX %+ _abi_test_clobber_v11
%xdefine abi_test_clobber_v11_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v11_upper
%xdefine abi_test_clobber_v12 BORINGSSL_PREFIX %+ _abi_test_clobber_v12
%xdefine abi_test_clobber_v12_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v12_upper
%xdefine abi_test_clobber_v13 BORINGSSL_PREFIX %+ _abi_test_clobber_v13
%xdefine abi_test_clobber_v13_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v13_upper
%xdefine abi_test_clobber_v14 BORINGSSL_PREFIX %+ _abi_test_clobber_v14
%xdefine abi_test_clobber_v14_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v14_upper
%xdefine abi_test_clobber_v15 BORINGSSL_PREFIX %+ _abi_test_clobber_v15
%xdefine abi_test_clobber_v15_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v15_upper
%xdefine abi_test_clobber_v16 BORINGSSL_PREFIX %+ _abi_test_clobber_v16
%xdefine abi_test_clobber_v17 BORINGSSL_PREFIX %+ _abi_test_clobber_v17
%xdefine abi_test_clobber_v18 BORINGSSL_PREFIX %+ _abi_test_clobber_v18
%xdefine abi_test_clobber_v19 BORINGSSL_PREFIX %+ _abi_test_clobber_v19
%xdefine abi_test_clobber_v2 BORINGSSL_PREFIX %+ _abi_test_clobber_v2
%xdefine abi_test_clobber_v20 BORINGSSL_PREFIX %+ _abi_test_clobber_v20
%xdefine abi_test_clobber_v21 BORINGSSL_PREFIX %+ _abi_test_clobber_v21
%xdefine abi_test_clobber_v22 BORINGSSL_PREFIX %+ _abi_test_clobber_v22
%xdefine abi_test_clobber_v23 BORINGSSL_PREFIX %+ _abi_test_clobber_v23
%xdefine abi_test_clobber_v24 BORINGSSL_PREFIX %+ _abi_test_clobber_v24
%xdefine abi_test_clobber_v25 BORINGSSL_PREFIX %+ _abi_test_clobber_v25
%xdefine abi_test_clobber_v26 BORINGSSL_PREFIX %+ _abi_test_clobber_v26
%xdefine abi_test_clobber_v27 BORINGSSL_PREFIX %+ _abi_test_clobber_v27
%xdefine abi_test_clobber_v28 BORINGSSL_PREFIX %+ _abi_test_clobber_v28
%xdefine abi_test_clobber_v29 BORINGSSL_PREFIX %+ _abi_test_clobber_v29
%xdefine abi_test_clobber_v3 BORINGSSL_PREFIX %+ _abi_test_clobber_v3
%xdefine abi_test_clobber_v30 BORINGSSL_PREFIX %+ _abi_test_clobber_v30
%xdefine abi_test_clobber_v31 BORINGSSL_PREFIX %+ _abi_test_clobber_v31
%xdefine abi_test_clobber_v4 BORINGSSL_PREFIX %+ _abi_test_clobber_v4
%xdefine abi_test_clobber_v5 BORINGSSL_PREFIX %+ _abi_test_clobber_v5
%xdefine abi_test_clobber_v6 BORINGSSL_PREFIX %+ _abi_test_clobber_v6
%xdefine abi_test_clobber_v7 BORINGSSL_PREFIX %+ _abi_test_clobber_v7
%xdefine abi_test_clobber_v8 BORINGSSL_PREFIX %+ _abi_test_clobber_v8
%xdefine abi_test_clobber_v8_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v8_upper
%xdefine abi_test_clobber_v9 BORINGSSL_PREFIX %+ _abi_test_clobber_v9
%xdefine abi_test_clobber_v9_upper BORINGSSL_PREFIX %+ _abi_test_clobber_v9_upper
%xdefine abi_test_clobber_x0 BORINGSSL_PREFIX %+ _abi_test_clobber_x0
%xdefine abi_test_clobber_x1 BORINGSSL_PREFIX %+ _abi_test_clobber_x1
%xdefine abi_test_clobber_x10 BORINGSSL_PREFIX %+ _abi_test_clobber_x10
%xdefine abi_test_clobber_x11 BORINGSSL_PREFIX %+ _abi_test_clobber_x11
%xdefine abi_test_clobber_x12 BORINGSSL_PREFIX %+ _abi_test_clobber_x12
%xdefine abi_test_clobber_x13 BORINGSSL_PREFIX %+ _abi_test_clobber_x13
%xdefine abi_test_clobber_x14 BORINGSSL_PREFIX %+ _abi_test_clobber_x14
%xdefine abi_test_clobber_x15 BORINGSSL_PREFIX %+ _abi_test_clobber_x15
%xdefine abi_test_clobber_x16 BORINGSSL_PREFIX %+ _abi_test_clobber_x16
%xdefine abi_test_clobber_x17 BORINGSSL_PREFIX %+ _abi_test_clobber_x17
%xdefine abi_test_clobber_x19 BORINGSSL_PREFIX %+ _abi_test_clobber_x19
%xdefine abi_test_clobber_x2 BORINGSSL_PREFIX %+ _abi_test_clobber_x2
%xdefine abi_test_clobber_x20 BORINGSSL_PREFIX %+ _abi_test_clobber_x20
%xdefine abi_test_clobber_x21 BORINGSSL_PREFIX %+ _abi_test_clobber_x21
%xdefine abi_test_clobber_x22 BORINGSSL_PREFIX %+ _abi_test_clobber_x22
%xdefine abi_test_clobber_x23 BORINGSSL_PREFIX %+ _abi_test_clobber_x23
%xdefine abi_test_clobber_x24 BORINGSSL_PREFIX %+ _abi_test_clobber_x24
%xdefine abi_test_clobber_x25 BORINGSSL_PREFIX %+ _abi_test_clobber_x25
%xdefine abi_test_clobber_x26 BORINGSSL_PREFIX %+ _abi_test_clobber_x26
%xdefine abi_test_clobber_x27 BORINGSSL_PREFIX %+ _abi_test_clobber_x27
%xdefine abi_test_clobber_x28 BORINGSSL_PREFIX %+ _abi_test_clobber_x28
%xdefine abi_test_clobber_x29 BORINGSSL_PREFIX %+ _abi_test_clobber_x29
%xdefine abi_test_clobber_x3 BORINGSSL_PREFIX %+ _abi_test_clobber_x3
%xdefine abi_test_clobber_x4 BORINGSSL_PREFIX %+ _abi_test_clobber_x4
%xdefine abi_test_clobber_x5 BORINGSSL_PREFIX %+ _abi_test_clobber_x5
%xdefine abi_test_clobber_x6 BORINGSSL_PREFIX %+ _abi_test_clobber_x6
%xdefine abi_test_clobber_x7 BORINGSSL_PREFIX %+ _abi_test_clobber_x7
%xdefine abi_test_clobber_x8 BORINGSSL_PREFIX %+ _abi_test_clobber_x8
%xdefine abi_test_clobber_x9 BORINGSSL_PREFIX %+ _abi_test_clobber_x9
%xdefine abi_test_clobber_xmm0 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm0
%xdefine abi_test_clobber_xmm1 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm1
%xdefine abi_test_clobber_xmm10 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm10
%xdefine abi_test_clobber_xmm11 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm11
%xdefine abi_test_clobber_xmm12 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm12
%xdefine abi_test_clobber_xmm13 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm13
%xdefine abi_test_clobber_xmm14 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm14
%xdefine abi_test_clobber_xmm15 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm15
%xdefine abi_test_clobber_xmm2 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm2
%xdefine abi_test_clobber_xmm3 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm3
%xdefine abi_test_clobber_xmm4 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm4
%xdefine abi_test_clobber_xmm5 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm5
%xdefine abi_test_clobber_xmm6 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm6
%xdefine abi_test_clobber_xmm7 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm7
%xdefine abi_test_clobber_xmm8 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm8
%xdefine abi_test_clobber_xmm9 BORINGSSL_PREFIX %+ _abi_test_clobber_xmm9
%xdefine abi_test_get_and_clear_direction_flag BORINGSSL_PREFIX %+ _abi_test_get_and_clear_direction_flag
%xdefine abi_test_set_direction_flag BORINGSSL_PREFIX %+ _abi_test_set_direction_flag
%xdefine abi_test_trampoline BORINGSSL_PREFIX %+ _abi_test_trampoline
%xdefine abi_test_unwind_return BORINGSSL_PREFIX %+ _abi_test_unwind_return
%xdefine abi_test_unwind_start BORINGSSL_PREFIX %+ _abi_test_unwind_start
%xdefine abi_test_unwind_stop BORINGSSL_PREFIX %+ _abi_test_unwind_stop
%xdefine aes128gcmsiv_aes_ks BORINGSSL_PREFIX %+ _aes128gcmsiv_aes_ks
%xdefine aes128gcmsiv_aes_ks_enc_x1 BORINGSSL_PREFIX %+ _aes128gcmsiv_aes_ks_enc_x1
%xdefine aes128gcmsiv_dec BORINGSSL_PREFIX %+ _aes128gcmsiv_dec
%xdefine aes128gcmsiv_ecb_enc_block BORINGSSL_PREFIX %+ _aes128gcmsiv_ecb_enc_block
%xdefine aes128gcmsiv_enc_msg_x4 BORINGSSL_PREFIX %+ _aes128gcmsiv_enc_msg_x4
%xdefine aes128gcmsiv_enc_msg_x8 BORINGSSL_PREFIX %+ _aes128gcmsiv_enc_msg_x8
%xdefine aes128gcmsiv_kdf BORINGSSL_PREFIX %+ _aes128gcmsiv_kdf
%xdefine aes256gcmsiv_aes_ks BORINGSSL_PREFIX %+ _aes256gcmsiv_aes_ks
%xdefine aes256gcmsiv_aes_ks_enc_x1 BORINGSSL_PREFIX %+ _aes256gcmsiv_aes_ks_enc_x1
%xdefine aes256gcmsiv_dec BORINGSSL_PREFIX %+ _aes256gcmsiv_dec
%xdefine aes256gcmsiv_ecb_enc_block BORINGSSL_PREFIX %+ _aes256gcmsiv_ecb_enc_block
%xdefine aes256gcmsiv_enc_msg_x4 BORINGSSL_PREFIX %+ _aes256gcmsiv_enc_msg_x4
%xdefine aes256gcmsiv_enc_msg_x8 BORINGSSL_PREFIX %+ _aes256gcmsiv_enc_msg_x8
%xdefine aes256gcmsiv_kdf BORINGSSL_PREFIX %+ _aes256gcmsiv_kdf
%xdefine aes_ctr_set_key BORINGSSL_PREFIX %+ _aes_ctr_set_key
%xdefine aes_gcm_dec_kernel BORINGSSL_PREFIX %+ _aes_gcm_dec_kernel
%xdefine aes_gcm_decrypt_avx512 BORINGSSL_PREFIX %+ _aes_gcm_decrypt_avx512
%xdefine aes_gcm_enc_kernel BORINGSSL_PREFIX %+ _aes_gcm_enc_kernel
%xdefine aes_gcm_encrypt_avx512 BORINGSSL_PREFIX %+ _aes_gcm_encrypt_avx512
%xdefine aes_hw_cbc_encrypt BORINGSSL_PREFIX %+ _aes_hw_cbc_encrypt
%xdefine aes_hw_ccm64_decrypt_blocks BORINGSSL_PREFIX %+ _aes_hw_ccm64_decrypt_blocks
%xdefine aes_hw_ccm64_encrypt_blocks BORINGSSL_PREFIX %+ _aes_hw_ccm64_encrypt_blocks
%xdefine aes_hw_ctr32_encrypt_blocks BORINGSSL_PREFIX %+ _aes_hw_ctr32_encrypt_blocks
%xdefine aes_hw_decrypt BORINGSSL_PREFIX %+ _aes_hw_decrypt
%xdefine aes_hw_ecb_encrypt BORINGSSL_PREFIX %+ _aes_hw_ecb_encrypt
%xdefine aes_hw_encrypt BORINGSSL_PREFIX %+ _aes_hw_encrypt
%xdefine aes_hw_set_decrypt_key BORINGSSL_PREFIX %+ _aes_hw_set_decrypt_key
%xdefine aes_hw_set_encrypt_key BORINGSSL_PREFIX %+ _aes_hw_set_encrypt_key
%xdefine aes_hw_xts_cipher BORINGSSL_PREFIX %+ _aes_hw_xts_cipher
%xdefine aes_hw_xts_decrypt BORINGSSL_PREFIX %+ _aes_hw_xts_decrypt
%xdefine aes_hw_xts_decrypt_avx512 BORINGSSL_PREFIX %+ _aes_hw_xts_decrypt_avx512
%xdefine aes_hw_xts_encrypt BORINGSSL_PREFIX %+ _aes_hw_xts_encrypt
%xdefine aes_hw_xts_encrypt_avx512 BORINGSSL_PREFIX %+ _aes_hw_xts_encrypt_avx512
%xdefine aes_nohw_cbc_encrypt BORINGSSL_PREFIX %+ _aes_nohw_cbc_encrypt
%xdefine aes_nohw_ctr32_encrypt_blocks BORINGSSL_PREFIX %+ _aes_nohw_ctr32_encrypt_blocks
%xdefine aes_nohw_decrypt BORINGSSL_PREFIX %+ _aes_nohw_decrypt
%xdefine aes_nohw_encrypt BORINGSSL_PREFIX %+ _aes_nohw_encrypt
%xdefine aes_nohw_set_decrypt_key BORINGSSL_PREFIX %+ _aes_nohw_set_decrypt_key
%xdefine aes_nohw_set_encrypt_key BORINGSSL_PREFIX %+ _aes_nohw_set_encrypt_key
%xdefine aesgcmsiv_htable6_init BORINGSSL_PREFIX %+ _aesgcmsiv_htable6_init
%xdefine aesgcmsiv_htable_init BORINGSSL_PREFIX %+ _aesgcmsiv_htable_init
%xdefine aesgcmsiv_htable_polyval BORINGSSL_PREFIX %+ _aesgcmsiv_htable_polyval
%xdefine aesgcmsiv_polyval_horner BORINGSSL_PREFIX %+ _aesgcmsiv_polyval_horner
%xdefine aesni_cbc_sha1_enc BORINGSSL_PREFIX %+ _aesni_cbc_sha1_enc
%xdefine aesni_cbc_sha256_enc BORINGSSL_PREFIX %+ _aesni_cbc_sha256_enc
%xdefine aesni_gcm_decrypt BORINGSSL_PREFIX %+ _aesni_gcm_decrypt
%xdefine aesni_gcm_encrypt BORINGSSL_PREFIX %+ _aesni_gcm_encrypt
%xdefine aesv8_gcm_8x_dec_128 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_128
%xdefine aesv8_gcm_8x_dec_192 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_192
%xdefine aesv8_gcm_8x_dec_256 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_dec_256
%xdefine aesv8_gcm_8x_enc_128 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_128
%xdefine aesv8_gcm_8x_enc_192 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_192
%xdefine aesv8_gcm_8x_enc_256 BORINGSSL_PREFIX %+ _aesv8_gcm_8x_enc_256
%xdefine allow_mocked_ube_detection_FOR_TESTING BORINGSSL_PREFIX %+ _allow_mocked_ube_detection_FOR_TESTING
%xdefine armv8_disable_dit BORINGSSL_PREFIX %+ _armv8_disable_dit
%xdefine armv8_enable_dit BORINGSSL_PREFIX %+ _armv8_enable_dit
%xdefine armv8_get_dit BORINGSSL_PREFIX %+ _armv8_get_dit
%xdefine armv8_restore_dit BORINGSSL_PREFIX %+ _armv8_restore_dit
%xdefine armv8_set_dit BORINGSSL_PREFIX %+ _armv8_set_dit
%xdefine asn1_bit_string_length BORINGSSL_PREFIX %+ _asn1_bit_string_length
%xdefine asn1_do_adb BORINGSSL_PREFIX %+ _asn1_do_adb
%xdefine asn1_enc_free BORINGSSL_PREFIX %+ _asn1_enc_free
%xdefine asn1_enc_init BORINGSSL_PREFIX %+ _asn1_enc_init
%xdefine asn1_enc_restore BORINGSSL_PREFIX %+ _asn1_enc_restore
%xdefine asn1_enc_save BORINGSSL_PREFIX %+ _asn1_enc_save
%xdefine asn1_encoding_clear BORINGSSL_PREFIX %+ _asn1_encoding_clear
%xdefine asn1_evp_pkey_methods BORINGSSL_PREFIX %+ _asn1_evp_pkey_methods
%xdefine asn1_evp_pkey_methods_size BORINGSSL_PREFIX %+ _asn1_evp_pkey_methods_size
%xdefine asn1_generalizedtime_to_tm BORINGSSL_PREFIX %+ _asn1_generalizedtime_to_tm
%xdefine asn1_get_choice_selector BORINGSSL_PREFIX %+ _asn1_get_choice_selector
%xdefine asn1_get_field_ptr BORINGSSL_PREFIX %+ _asn1_get_field_ptr
%xdefine asn1_get_string_table_for_testing BORINGSSL_PREFIX %+ _asn1_get_string_table_for_testing
%xdefine asn1_is_printable BORINGSSL_PREFIX %+ _asn1_is_printable
%xdefine asn1_item_combine_free BORINGSSL_PREFIX %+ _asn1_item_combine_free
%xdefine asn1_refcount_dec_and_test_zero BORINGSSL_PREFIX %+ _asn1_refcount_dec_and_test_zero
%xdefine asn1_refcount_set_one BORINGSSL_PREFIX %+ _asn1_refcount_set_one
%xdefine asn1_set_choice_selector BORINGSSL_PREFIX %+ _asn1_set_choice_selector
%xdefine asn1_type_cleanup BORINGSSL_PREFIX %+ _asn1_type_cleanup
%xdefine asn1_type_set0_string BORINGSSL_PREFIX %+ _asn1_type_set0_string
%xdefine asn1_type_value_as_pointer BORINGSSL_PREFIX %+ _asn1_type_value_as_pointer
%xdefine asn1_utctime_to_tm BORINGSSL_PREFIX %+ _asn1_utctime_to_tm
%xdefine awslc_api_version_num BORINGSSL_PREFIX %+ _awslc_api_version_num
%xdefine awslc_version_string BORINGSSL_PREFIX %+ _awslc_version_string
%xdefine beeu_mod_inverse_vartime BORINGSSL_PREFIX %+ _beeu_mod_inverse_vartime
%xdefine bignum_add_p384 BORINGSSL_PREFIX %+ _bignum_add_p384
%xdefine bignum_add_p521 BORINGSSL_PREFIX %+ _bignum_add_p521
%xdefine bignum_copy_row_from_table BORINGSSL_PREFIX %+ _bignum_copy_row_from_table
%xdefine bignum_copy_row_from_table_16 BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_16
%xdefine bignum_copy_row_from_table_32 BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_32
%xdefine bignum_copy_row_from_table_8n BORINGSSL_PREFIX %+ _bignum_copy_row_from_table_8n
%xdefine bignum_deamont_p384 BORINGSSL_PREFIX %+ _bignum_deamont_p384
%xdefine bignum_deamont_p384_alt BORINGSSL_PREFIX %+ _bignum_deamont_p384_alt
%xdefine bignum_emontredc_8n BORINGSSL_PREFIX %+ _bignum_emontredc_8n
%xdefine bignum_fromlebytes_6 BORINGSSL_PREFIX %+ _bignum_fromlebytes_6
%xdefine bignum_fromlebytes_p521 BORINGSSL_PREFIX %+ _bignum_fromlebytes_p521
%xdefine bignum_ge BORINGSSL_PREFIX %+ _bignum_ge
%xdefine bignum_inv_p521 BORINGSSL_PREFIX %+ _bignum_inv_p521
%xdefine bignum_kmul_16_32 BORINGSSL_PREFIX %+ _bignum_kmul_16_32
%xdefine bignum_kmul_32_64 BORINGSSL_PREFIX %+ _bignum_kmul_32_64
%xdefine bignum_ksqr_16_32 BORINGSSL_PREFIX %+ _bignum_ksqr_16_32
%xdefine bignum_ksqr_32_64 BORINGSSL_PREFIX %+ _bignum_ksqr_32_64
%xdefine bignum_littleendian_6 BORINGSSL_PREFIX %+ _bignum_littleendian_6
%xdefine bignum_madd_n25519 BORINGSSL_PREFIX %+ _bignum_madd_n25519
%xdefine bignum_madd_n25519_alt BORINGSSL_PREFIX %+ _bignum_madd_n25519_alt
%xdefine bignum_mod_n25519 BORINGSSL_PREFIX %+ _bignum_mod_n25519
%xdefine bignum_montinv_p256 BORINGSSL_PREFIX %+ _bignum_montinv_p256
%xdefine bignum_montinv_p384 BORINGSSL_PREFIX %+ _bignum_montinv_p384
%xdefine bignum_montmul_p384 BORINGSSL_PREFIX %+ _bignum_montmul_p384
%xdefine bignum_montmul_p384_alt BORINGSSL_PREFIX %+ _bignum_montmul_p384_alt
%xdefine bignum_montsqr_p384 BORINGSSL_PREFIX %+ _bignum_montsqr_p384
%xdefine bignum_montsqr_p384_alt BORINGSSL_PREFIX %+ _bignum_montsqr_p384_alt
%xdefine bignum_mul BORINGSSL_PREFIX %+ _bignum_mul
%xdefine bignum_mul_p521 BORINGSSL_PREFIX %+ _bignum_mul_p521
%xdefine bignum_mul_p521_alt BORINGSSL_PREFIX %+ _bignum_mul_p521_alt
%xdefine bignum_neg_p25519 BORINGSSL_PREFIX %+ _bignum_neg_p25519
%xdefine bignum_neg_p384 BORINGSSL_PREFIX %+ _bignum_neg_p384
%xdefine bignum_neg_p521 BORINGSSL_PREFIX %+ _bignum_neg_p521
%xdefine bignum_nonzero_6 BORINGSSL_PREFIX %+ _bignum_nonzero_6
%xdefine bignum_optsub BORINGSSL_PREFIX %+ _bignum_optsub
%xdefine bignum_sqr BORINGSSL_PREFIX %+ _bignum_sqr
%xdefine bignum_sqr_p521 BORINGSSL_PREFIX %+ _bignum_sqr_p521
%xdefine bignum_sqr_p521_alt BORINGSSL_PREFIX %+ _bignum_sqr_p521_alt
%xdefine bignum_sub_p384 BORINGSSL_PREFIX %+ _bignum_sub_p384
%xdefine bignum_sub_p521 BORINGSSL_PREFIX %+ _bignum_sub_p521
%xdefine bignum_tolebytes_6 BORINGSSL_PREFIX %+ _bignum_tolebytes_6
%xdefine bignum_tolebytes_p521 BORINGSSL_PREFIX %+ _bignum_tolebytes_p521
%xdefine bignum_tomont_p384 BORINGSSL_PREFIX %+ _bignum_tomont_p384
%xdefine bignum_tomont_p384_alt BORINGSSL_PREFIX %+ _bignum_tomont_p384_alt
%xdefine bio_clear_socket_error BORINGSSL_PREFIX %+ _bio_clear_socket_error
%xdefine bio_errno_should_retry BORINGSSL_PREFIX %+ _bio_errno_should_retry
%xdefine bio_ip_and_port_to_socket_and_addr BORINGSSL_PREFIX %+ _bio_ip_and_port_to_socket_and_addr
%xdefine bio_sock_error_get_and_clear BORINGSSL_PREFIX %+ _bio_sock_error_get_and_clear
%xdefine bio_socket_nbio BORINGSSL_PREFIX %+ _bio_socket_nbio
%xdefine bio_socket_should_retry BORINGSSL_PREFIX %+ _bio_socket_should_retry
%xdefine bn_abs_sub_consttime BORINGSSL_PREFIX %+ _bn_abs_sub_consttime
%xdefine bn_add_words BORINGSSL_PREFIX %+ _bn_add_words
%xdefine bn_assert_fits_in_bytes BORINGSSL_PREFIX %+ _bn_assert_fits_in_bytes
%xdefine bn_big_endian_to_words BORINGSSL_PREFIX %+ _bn_big_endian_to_words
%xdefine bn_copy_words BORINGSSL_PREFIX %+ _bn_copy_words
%xdefine bn_div_consttime BORINGSSL_PREFIX %+ _bn_div_consttime
%xdefine bn_div_words BORINGSSL_PREFIX %+ _bn_div_words
%xdefine bn_expand BORINGSSL_PREFIX %+ _bn_expand
%xdefine bn_fits_in_words BORINGSSL_PREFIX %+ _bn_fits_in_words
%xdefine bn_from_montgomery_small BORINGSSL_PREFIX %+ _bn_from_montgomery_small
%xdefine bn_gather5 BORINGSSL_PREFIX %+ _bn_gather5
%xdefine bn_in_range_words BORINGSSL_PREFIX %+ _bn_in_range_words
%xdefine bn_is_bit_set_words BORINGSSL_PREFIX %+ _bn_is_bit_set_words
%xdefine bn_is_relatively_prime BORINGSSL_PREFIX %+ _bn_is_relatively_prime
%xdefine bn_jacobi BORINGSSL_PREFIX %+ _bn_jacobi
%xdefine bn_lcm_consttime BORINGSSL_PREFIX %+ _bn_lcm_consttime
%xdefine bn_less_than_montgomery_R BORINGSSL_PREFIX %+ _bn_less_than_montgomery_R
%xdefine bn_less_than_words BORINGSSL_PREFIX %+ _bn_less_than_words
%xdefine bn_little_endian_to_words BORINGSSL_PREFIX %+ _bn_little_endian_to_words
%xdefine bn_miller_rabin_init BORINGSSL_PREFIX %+ _bn_miller_rabin_init
%xdefine bn_miller_rabin_iteration BORINGSSL_PREFIX %+ _bn_miller_rabin_iteration
%xdefine bn_minimal_width BORINGSSL_PREFIX %+ _bn_minimal_width
%xdefine bn_mod_add_consttime BORINGSSL_PREFIX %+ _bn_mod_add_consttime
%xdefine bn_mod_add_words BORINGSSL_PREFIX %+ _bn_mod_add_words
%xdefine bn_mod_exp_mont_small BORINGSSL_PREFIX %+ _bn_mod_exp_mont_small
%xdefine bn_mod_inverse0_prime_mont_small BORINGSSL_PREFIX %+ _bn_mod_inverse0_prime_mont_small
%xdefine bn_mod_inverse_consttime BORINGSSL_PREFIX %+ _bn_mod_inverse_consttime
%xdefine bn_mod_inverse_prime BORINGSSL_PREFIX %+ _bn_mod_inverse_prime
%xdefine bn_mod_inverse_secret_prime BORINGSSL_PREFIX %+ _bn_mod_inverse_secret_prime
%xdefine bn_mod_lshift1_consttime BORINGSSL_PREFIX %+ _bn_mod_lshift1_consttime
%xdefine bn_mod_lshift_consttime BORINGSSL_PREFIX %+ _bn_mod_lshift_consttime
%xdefine bn_mod_mul_montgomery_small BORINGSSL_PREFIX %+ _bn_mod_mul_montgomery_small
%xdefine bn_mod_sub_consttime BORINGSSL_PREFIX %+ _bn_mod_sub_consttime
%xdefine bn_mod_sub_words BORINGSSL_PREFIX %+ _bn_mod_sub_words
%xdefine bn_mod_u16_consttime BORINGSSL_PREFIX %+ _bn_mod_u16_consttime
%xdefine bn_mont_ctx_cleanup BORINGSSL_PREFIX %+ _bn_mont_ctx_cleanup
%xdefine bn_mont_ctx_init BORINGSSL_PREFIX %+ _bn_mont_ctx_init
%xdefine bn_mont_ctx_set_RR_consttime BORINGSSL_PREFIX %+ _bn_mont_ctx_set_RR_consttime
%xdefine bn_mont_n0 BORINGSSL_PREFIX %+ _bn_mont_n0
%xdefine bn_mul4x_mont BORINGSSL_PREFIX %+ _bn_mul4x_mont
%xdefine bn_mul8x_mont_neon BORINGSSL_PREFIX %+ _bn_mul8x_mont_neon
%xdefine bn_mul_add_words BORINGSSL_PREFIX %+ _bn_mul_add_words
%xdefine bn_mul_comba4 BORINGSSL_PREFIX %+ _bn_mul_comba4
%xdefine bn_mul_comba8 BORINGSSL_PREFIX %+ _bn_mul_comba8
%xdefine bn_mul_consttime BORINGSSL_PREFIX %+ _bn_mul_consttime
%xdefine bn_mul_mont BORINGSSL_PREFIX %+ _bn_mul_mont
%xdefine bn_mul_mont_gather5 BORINGSSL_PREFIX %+ _bn_mul_mont_gather5
%xdefine bn_mul_mont_nohw BORINGSSL_PREFIX %+ _bn_mul_mont_nohw
%xdefine bn_mul_small BORINGSSL_PREFIX %+ _bn_mul_small
%xdefine bn_mul_words BORINGSSL_PREFIX %+ _bn_mul_words
%xdefine bn_mulx4x_mont BORINGSSL_PREFIX %+ _bn_mulx4x_mont
%xdefine bn_odd_number_is_obviously_composite BORINGSSL_PREFIX %+ _bn_odd_number_is_obviously_composite
%xdefine bn_one_to_montgomery BORINGSSL_PREFIX %+ _bn_one_to_montgomery
%xdefine bn_power5 BORINGSSL_PREFIX %+ _bn_power5
%xdefine bn_rand_range_words BORINGSSL_PREFIX %+ _bn_rand_range_words
%xdefine bn_rand_secret_range BORINGSSL_PREFIX %+ _bn_rand_secret_range
%xdefine bn_reduce_once BORINGSSL_PREFIX %+ _bn_reduce_once
%xdefine bn_reduce_once_in_place BORINGSSL_PREFIX %+ _bn_reduce_once_in_place
%xdefine bn_resize_words BORINGSSL_PREFIX %+ _bn_resize_words
%xdefine bn_rshift1_words BORINGSSL_PREFIX %+ _bn_rshift1_words
%xdefine bn_rshift_secret_shift BORINGSSL_PREFIX %+ _bn_rshift_secret_shift
%xdefine bn_rshift_words BORINGSSL_PREFIX %+ _bn_rshift_words
%xdefine bn_scatter5 BORINGSSL_PREFIX %+ _bn_scatter5
%xdefine bn_select_words BORINGSSL_PREFIX %+ _bn_select_words
%xdefine bn_set_minimal_width BORINGSSL_PREFIX %+ _bn_set_minimal_width
%xdefine bn_set_static_words BORINGSSL_PREFIX %+ _bn_set_static_words
%xdefine bn_set_words BORINGSSL_PREFIX %+ _bn_set_words
%xdefine bn_sqr8x_internal BORINGSSL_PREFIX %+ _bn_sqr8x_internal
%xdefine bn_sqr8x_mont BORINGSSL_PREFIX %+ _bn_sqr8x_mont
%xdefine bn_sqr_comba4 BORINGSSL_PREFIX %+ _bn_sqr_comba4
%xdefine bn_sqr_comba8 BORINGSSL_PREFIX %+ _bn_sqr_comba8
%xdefine bn_sqr_consttime BORINGSSL_PREFIX %+ _bn_sqr_consttime
%xdefine bn_sqr_small BORINGSSL_PREFIX %+ _bn_sqr_small
%xdefine bn_sqr_words BORINGSSL_PREFIX %+ _bn_sqr_words
%xdefine bn_sqrx8x_internal BORINGSSL_PREFIX %+ _bn_sqrx8x_internal
%xdefine bn_sub_words BORINGSSL_PREFIX %+ _bn_sub_words
%xdefine bn_to_montgomery_small BORINGSSL_PREFIX %+ _bn_to_montgomery_small
%xdefine bn_uadd_consttime BORINGSSL_PREFIX %+ _bn_uadd_consttime
%xdefine bn_usub_consttime BORINGSSL_PREFIX %+ _bn_usub_consttime
%xdefine bn_wexpand BORINGSSL_PREFIX %+ _bn_wexpand
%xdefine bn_words_to_big_endian BORINGSSL_PREFIX %+ _bn_words_to_big_endian
%xdefine bn_words_to_little_endian BORINGSSL_PREFIX %+ _bn_words_to_little_endian
%xdefine boringssl_self_test_hmac_sha256 BORINGSSL_PREFIX %+ _boringssl_self_test_hmac_sha256
%xdefine boringssl_self_test_sha256 BORINGSSL_PREFIX %+ _boringssl_self_test_sha256
%xdefine bsaes_cbc_encrypt BORINGSSL_PREFIX %+ _bsaes_cbc_encrypt
%xdefine bsaes_ctr32_encrypt_blocks BORINGSSL_PREFIX %+ _bsaes_ctr32_encrypt_blocks
%xdefine c2i_ASN1_BIT_STRING BORINGSSL_PREFIX %+ _c2i_ASN1_BIT_STRING
%xdefine c2i_ASN1_INTEGER BORINGSSL_PREFIX %+ _c2i_ASN1_INTEGER
%xdefine c2i_ASN1_OBJECT BORINGSSL_PREFIX %+ _c2i_ASN1_OBJECT
%xdefine cbb_add_latin1 BORINGSSL_PREFIX %+ _cbb_add_latin1
%xdefine cbb_add_ucs2_be BORINGSSL_PREFIX %+ _cbb_add_ucs2_be
%xdefine cbb_add_utf32_be BORINGSSL_PREFIX %+ _cbb_add_utf32_be
%xdefine cbb_add_utf8 BORINGSSL_PREFIX %+ _cbb_add_utf8
%xdefine cbb_get_utf8_len BORINGSSL_PREFIX %+ _cbb_get_utf8_len
%xdefine cbs_get_any_asn1_element BORINGSSL_PREFIX %+ _cbs_get_any_asn1_element
%xdefine cbs_get_latin1 BORINGSSL_PREFIX %+ _cbs_get_latin1
%xdefine cbs_get_ucs2_be BORINGSSL_PREFIX %+ _cbs_get_ucs2_be
%xdefine cbs_get_utf32_be BORINGSSL_PREFIX %+ _cbs_get_utf32_be
%xdefine cbs_get_utf8 BORINGSSL_PREFIX %+ _cbs_get_utf8
%xdefine chacha20_poly1305_open BORINGSSL_PREFIX %+ _chacha20_poly1305_open
%xdefine chacha20_poly1305_seal BORINGSSL_PREFIX %+ _chacha20_poly1305_seal
%xdefine cn2dnsid BORINGSSL_PREFIX %+ _cn2dnsid
%xdefine crypto_gcm_avx512_enabled BORINGSSL_PREFIX %+ _crypto_gcm_avx512_enabled
%xdefine crypto_gcm_clmul_enabled BORINGSSL_PREFIX %+ _crypto_gcm_clmul_enabled
%xdefine curve25519_x25519 BORINGSSL_PREFIX %+ _curve25519_x25519
%xdefine curve25519_x25519_alt BORINGSSL_PREFIX %+ _curve25519_x25519_alt
%xdefine curve25519_x25519_byte BORINGSSL_PREFIX %+ _curve25519_x25519_byte
%xdefine curve25519_x25519_byte_alt BORINGSSL_PREFIX %+ _curve25519_x25519_byte_alt
%xdefine curve25519_x25519base BORINGSSL_PREFIX %+ _curve25519_x25519base
%xdefine curve25519_x25519base_alt BORINGSSL_PREFIX %+ _curve25519_x25519base_alt
%xdefine curve25519_x25519base_byte BORINGSSL_PREFIX %+ _curve25519_x25519base_byte
%xdefine curve25519_x25519base_byte_alt BORINGSSL_PREFIX %+ _curve25519_x25519base_byte_alt
%xdefine d2i_ASN1_BIT_STRING BORINGSSL_PREFIX %+ _d2i_ASN1_BIT_STRING
%xdefine d2i_ASN1_BMPSTRING BORINGSSL_PREFIX %+ _d2i_ASN1_BMPSTRING
%xdefine d2i_ASN1_BOOLEAN BORINGSSL_PREFIX %+ _d2i_ASN1_BOOLEAN
%xdefine d2i_ASN1_ENUMERATED BORINGSSL_PREFIX %+ _d2i_ASN1_ENUMERATED
%xdefine d2i_ASN1_GENERALIZEDTIME BORINGSSL_PREFIX %+ _d2i_ASN1_GENERALIZEDTIME
%xdefine d2i_ASN1_GENERALSTRING BORINGSSL_PREFIX %+ _d2i_ASN1_GENERALSTRING
%xdefine d2i_ASN1_IA5STRING BORINGSSL_PREFIX %+ _d2i_ASN1_IA5STRING
%xdefine d2i_ASN1_INTEGER BORINGSSL_PREFIX %+ _d2i_ASN1_INTEGER
%xdefine d2i_ASN1_NULL BORINGSSL_PREFIX %+ _d2i_ASN1_NULL
%xdefine d2i_ASN1_OBJECT BORINGSSL_PREFIX %+ _d2i_ASN1_OBJECT
%xdefine d2i_ASN1_OCTET_STRING BORINGSSL_PREFIX %+ _d2i_ASN1_OCTET_STRING
%xdefine d2i_ASN1_PRINTABLE BORINGSSL_PREFIX %+ _d2i_ASN1_PRINTABLE
%xdefine d2i_ASN1_PRINTABLESTRING BORINGSSL_PREFIX %+ _d2i_ASN1_PRINTABLESTRING
%xdefine d2i_ASN1_SEQUENCE_ANY BORINGSSL_PREFIX %+ _d2i_ASN1_SEQUENCE_ANY
%xdefine d2i_ASN1_SET_ANY BORINGSSL_PREFIX %+ _d2i_ASN1_SET_ANY
%xdefine d2i_ASN1_T61STRING BORINGSSL_PREFIX %+ _d2i_ASN1_T61STRING
%xdefine d2i_ASN1_TIME BORINGSSL_PREFIX %+ _d2i_ASN1_TIME
%xdefine d2i_ASN1_TYPE BORINGSSL_PREFIX %+ _d2i_ASN1_TYPE
%xdefine d2i_ASN1_UNIVERSALSTRING BORINGSSL_PREFIX %+ _d2i_ASN1_UNIVERSALSTRING
%xdefine d2i_ASN1_UTCTIME BORINGSSL_PREFIX %+ _d2i_ASN1_UTCTIME
%xdefine d2i_ASN1_UTF8STRING BORINGSSL_PREFIX %+ _d2i_ASN1_UTF8STRING
%xdefine d2i_ASN1_VISIBLESTRING BORINGSSL_PREFIX %+ _d2i_ASN1_VISIBLESTRING
%xdefine d2i_AUTHORITY_INFO_ACCESS BORINGSSL_PREFIX %+ _d2i_AUTHORITY_INFO_ACCESS
%xdefine d2i_AUTHORITY_KEYID BORINGSSL_PREFIX %+ _d2i_AUTHORITY_KEYID
%xdefine d2i_AutoPrivateKey BORINGSSL_PREFIX %+ _d2i_AutoPrivateKey
%xdefine d2i_BASIC_CONSTRAINTS BORINGSSL_PREFIX %+ _d2i_BASIC_CONSTRAINTS
%xdefine d2i_CERTIFICATEPOLICIES BORINGSSL_PREFIX %+ _d2i_CERTIFICATEPOLICIES
%xdefine d2i_CRL_DIST_POINTS BORINGSSL_PREFIX %+ _d2i_CRL_DIST_POINTS
%xdefine d2i_DHparams BORINGSSL_PREFIX %+ _d2i_DHparams
%xdefine d2i_DHparams_bio BORINGSSL_PREFIX %+ _d2i_DHparams_bio
%xdefine d2i_DIRECTORYSTRING BORINGSSL_PREFIX %+ _d2i_DIRECTORYSTRING
%xdefine d2i_DISPLAYTEXT BORINGSSL_PREFIX %+ _d2i_DISPLAYTEXT
%xdefine d2i_DSAPrivateKey BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey
%xdefine d2i_DSAPrivateKey_bio BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey_bio
%xdefine d2i_DSAPrivateKey_fp BORINGSSL_PREFIX %+ _d2i_DSAPrivateKey_fp
%xdefine d2i_DSAPublicKey BORINGSSL_PREFIX %+ _d2i_DSAPublicKey
%xdefine d2i_DSA_PUBKEY BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY
%xdefine d2i_DSA_PUBKEY_bio BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY_bio
%xdefine d2i_DSA_PUBKEY_fp BORINGSSL_PREFIX %+ _d2i_DSA_PUBKEY_fp
%xdefine d2i_DSA_SIG BORINGSSL_PREFIX %+ _d2i_DSA_SIG
%xdefine d2i_DSAparams BORINGSSL_PREFIX %+ _d2i_DSAparams
%xdefine d2i_ECDSA_SIG BORINGSSL_PREFIX %+ _d2i_ECDSA_SIG
%xdefine d2i_ECPKParameters BORINGSSL_PREFIX %+ _d2i_ECPKParameters
%xdefine d2i_ECPKParameters_bio BORINGSSL_PREFIX %+ _d2i_ECPKParameters_bio
%xdefine d2i_ECParameters BORINGSSL_PREFIX %+ _d2i_ECParameters
%xdefine d2i_ECPrivateKey BORINGSSL_PREFIX %+ _d2i_ECPrivateKey
%xdefine d2i_ECPrivateKey_bio BORINGSSL_PREFIX %+ _d2i_ECPrivateKey_bio
%xdefine d2i_ECPrivateKey_fp BORINGSSL_PREFIX %+ _d2i_ECPrivateKey_fp
%xdefine d2i_EC_PUBKEY BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY
%xdefine d2i_EC_PUBKEY_bio BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY_bio
%xdefine d2i_EC_PUBKEY_fp BORINGSSL_PREFIX %+ _d2i_EC_PUBKEY_fp
%xdefine d2i_EXTENDED_KEY_USAGE BORINGSSL_PREFIX %+ _d2i_EXTENDED_KEY_USAGE
%xdefine d2i_GENERAL_NAME BORINGSSL_PREFIX %+ _d2i_GENERAL_NAME
%xdefine d2i_GENERAL_NAMES BORINGSSL_PREFIX %+ _d2i_GENERAL_NAMES
%xdefine d2i_ISSUING_DIST_POINT BORINGSSL_PREFIX %+ _d2i_ISSUING_DIST_POINT
%xdefine d2i_NETSCAPE_SPKAC BORINGSSL_PREFIX %+ _d2i_NETSCAPE_SPKAC
%xdefine d2i_NETSCAPE_SPKI BORINGSSL_PREFIX %+ _d2i_NETSCAPE_SPKI
%xdefine d2i_OCSP_BASICRESP BORINGSSL_PREFIX %+ _d2i_OCSP_BASICRESP
%xdefine d2i_OCSP_CERTID BORINGSSL_PREFIX %+ _d2i_OCSP_CERTID
%xdefine d2i_OCSP_ONEREQ BORINGSSL_PREFIX %+ _d2i_OCSP_ONEREQ
%xdefine d2i_OCSP_REQINFO BORINGSSL_PREFIX %+ _d2i_OCSP_REQINFO
%xdefine d2i_OCSP_REQUEST BORINGSSL_PREFIX %+ _d2i_OCSP_REQUEST
%xdefine d2i_OCSP_REQUEST_bio BORINGSSL_PREFIX %+ _d2i_OCSP_REQUEST_bio
%xdefine d2i_OCSP_RESPBYTES BORINGSSL_PREFIX %+ _d2i_OCSP_RESPBYTES
%xdefine d2i_OCSP_RESPDATA BORINGSSL_PREFIX %+ _d2i_OCSP_RESPDATA
%xdefine d2i_OCSP_RESPONSE BORINGSSL_PREFIX %+ _d2i_OCSP_RESPONSE
%xdefine d2i_OCSP_RESPONSE_bio BORINGSSL_PREFIX %+ _d2i_OCSP_RESPONSE_bio
%xdefine d2i_OCSP_REVOKEDINFO BORINGSSL_PREFIX %+ _d2i_OCSP_REVOKEDINFO
%xdefine d2i_OCSP_SIGNATURE BORINGSSL_PREFIX %+ _d2i_OCSP_SIGNATURE
%xdefine d2i_OCSP_SINGLERESP BORINGSSL_PREFIX %+ _d2i_OCSP_SINGLERESP
%xdefine d2i_PKCS12 BORINGSSL_PREFIX %+ _d2i_PKCS12
%xdefine d2i_PKCS12_bio BORINGSSL_PREFIX %+ _d2i_PKCS12_bio
%xdefine d2i_PKCS12_fp BORINGSSL_PREFIX %+ _d2i_PKCS12_fp
%xdefine d2i_PKCS7 BORINGSSL_PREFIX %+ _d2i_PKCS7
%xdefine d2i_PKCS7_DIGEST BORINGSSL_PREFIX %+ _d2i_PKCS7_DIGEST
%xdefine d2i_PKCS7_ENCRYPT BORINGSSL_PREFIX %+ _d2i_PKCS7_ENCRYPT
%xdefine d2i_PKCS7_ENC_CONTENT BORINGSSL_PREFIX %+ _d2i_PKCS7_ENC_CONTENT
%xdefine d2i_PKCS7_ENVELOPE BORINGSSL_PREFIX %+ _d2i_PKCS7_ENVELOPE
%xdefine d2i_PKCS7_ISSUER_AND_SERIAL BORINGSSL_PREFIX %+ _d2i_PKCS7_ISSUER_AND_SERIAL
%xdefine d2i_PKCS7_RECIP_INFO BORINGSSL_PREFIX %+ _d2i_PKCS7_RECIP_INFO
%xdefine d2i_PKCS7_SIGNED BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGNED
%xdefine d2i_PKCS7_SIGNER_INFO BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGNER_INFO
%xdefine d2i_PKCS7_SIGN_ENVELOPE BORINGSSL_PREFIX %+ _d2i_PKCS7_SIGN_ENVELOPE
%xdefine d2i_PKCS7_bio BORINGSSL_PREFIX %+ _d2i_PKCS7_bio
%xdefine d2i_PKCS8PrivateKey_bio BORINGSSL_PREFIX %+ _d2i_PKCS8PrivateKey_bio
%xdefine d2i_PKCS8PrivateKey_fp BORINGSSL_PREFIX %+ _d2i_PKCS8PrivateKey_fp
%xdefine d2i_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO
%xdefine d2i_PKCS8_PRIV_KEY_INFO_bio BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO_bio
%xdefine d2i_PKCS8_PRIV_KEY_INFO_fp BORINGSSL_PREFIX %+ _d2i_PKCS8_PRIV_KEY_INFO_fp
%xdefine d2i_PKCS8_bio BORINGSSL_PREFIX %+ _d2i_PKCS8_bio
%xdefine d2i_PKCS8_fp BORINGSSL_PREFIX %+ _d2i_PKCS8_fp
%xdefine d2i_PUBKEY BORINGSSL_PREFIX %+ _d2i_PUBKEY
%xdefine d2i_PUBKEY_bio BORINGSSL_PREFIX %+ _d2i_PUBKEY_bio
%xdefine d2i_PUBKEY_fp BORINGSSL_PREFIX %+ _d2i_PUBKEY_fp
%xdefine d2i_PrivateKey BORINGSSL_PREFIX %+ _d2i_PrivateKey
%xdefine d2i_PrivateKey_bio BORINGSSL_PREFIX %+ _d2i_PrivateKey_bio
%xdefine d2i_PrivateKey_fp BORINGSSL_PREFIX %+ _d2i_PrivateKey_fp
%xdefine d2i_PublicKey BORINGSSL_PREFIX %+ _d2i_PublicKey
%xdefine d2i_RSAPrivateKey BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey
%xdefine d2i_RSAPrivateKey_bio BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey_bio
%xdefine d2i_RSAPrivateKey_fp BORINGSSL_PREFIX %+ _d2i_RSAPrivateKey_fp
%xdefine d2i_RSAPublicKey BORINGSSL_PREFIX %+ _d2i_RSAPublicKey
%xdefine d2i_RSAPublicKey_bio BORINGSSL_PREFIX %+ _d2i_RSAPublicKey_bio
%xdefine d2i_RSAPublicKey_fp BORINGSSL_PREFIX %+ _d2i_RSAPublicKey_fp
%xdefine d2i_RSA_PSS_PARAMS BORINGSSL_PREFIX %+ _d2i_RSA_PSS_PARAMS
%xdefine d2i_RSA_PUBKEY BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY
%xdefine d2i_RSA_PUBKEY_bio BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY_bio
%xdefine d2i_RSA_PUBKEY_fp BORINGSSL_PREFIX %+ _d2i_RSA_PUBKEY_fp
%xdefine d2i_X509 BORINGSSL_PREFIX %+ _d2i_X509
%xdefine d2i_X509_ALGOR BORINGSSL_PREFIX %+ _d2i_X509_ALGOR
%xdefine d2i_X509_ATTRIBUTE BORINGSSL_PREFIX %+ _d2i_X509_ATTRIBUTE
%xdefine d2i_X509_AUX BORINGSSL_PREFIX %+ _d2i_X509_AUX
%xdefine d2i_X509_CERT_AUX BORINGSSL_PREFIX %+ _d2i_X509_CERT_AUX
%xdefine d2i_X509_CINF BORINGSSL_PREFIX %+ _d2i_X509_CINF
%xdefine d2i_X509_CRL BORINGSSL_PREFIX %+ _d2i_X509_CRL
%xdefine d2i_X509_CRL_INFO BORINGSSL_PREFIX %+ _d2i_X509_CRL_INFO
%xdefine d2i_X509_CRL_bio BORINGSSL_PREFIX %+ _d2i_X509_CRL_bio
%xdefine d2i_X509_CRL_fp BORINGSSL_PREFIX %+ _d2i_X509_CRL_fp
%xdefine d2i_X509_EXTENSION BORINGSSL_PREFIX %+ _d2i_X509_EXTENSION
%xdefine d2i_X509_EXTENSIONS BORINGSSL_PREFIX %+ _d2i_X509_EXTENSIONS
%xdefine d2i_X509_NAME BORINGSSL_PREFIX %+ _d2i_X509_NAME
%xdefine d2i_X509_NAME_ENTRY BORINGSSL_PREFIX %+ _d2i_X509_NAME_ENTRY
%xdefine d2i_X509_PUBKEY BORINGSSL_PREFIX %+ _d2i_X509_PUBKEY
%xdefine d2i_X509_REQ BORINGSSL_PREFIX %+ _d2i_X509_REQ
%xdefine d2i_X509_REQ_INFO BORINGSSL_PREFIX %+ _d2i_X509_REQ_INFO
%xdefine d2i_X509_REQ_bio BORINGSSL_PREFIX %+ _d2i_X509_REQ_bio
%xdefine d2i_X509_REQ_fp BORINGSSL_PREFIX %+ _d2i_X509_REQ_fp
%xdefine d2i_X509_REVOKED BORINGSSL_PREFIX %+ _d2i_X509_REVOKED
%xdefine d2i_X509_SIG BORINGSSL_PREFIX %+ _d2i_X509_SIG
%xdefine d2i_X509_VAL BORINGSSL_PREFIX %+ _d2i_X509_VAL
%xdefine d2i_X509_bio BORINGSSL_PREFIX %+ _d2i_X509_bio
%xdefine d2i_X509_fp BORINGSSL_PREFIX %+ _d2i_X509_fp
%xdefine dh_asn1_meth BORINGSSL_PREFIX %+ _dh_asn1_meth
%xdefine dh_check_params_fast BORINGSSL_PREFIX %+ _dh_check_params_fast
%xdefine dh_compute_key_padded_no_self_test BORINGSSL_PREFIX %+ _dh_compute_key_padded_no_self_test
%xdefine dh_pkey_meth BORINGSSL_PREFIX %+ _dh_pkey_meth
%xdefine disable_mocked_ube_detection_FOR_TESTING BORINGSSL_PREFIX %+ _disable_mocked_ube_detection_FOR_TESTING
%xdefine dsa_asn1_meth BORINGSSL_PREFIX %+ _dsa_asn1_meth
%xdefine dsa_check_key BORINGSSL_PREFIX %+ _dsa_check_key
%xdefine dsa_internal_paramgen BORINGSSL_PREFIX %+ _dsa_internal_paramgen
%xdefine dsa_pkey_meth BORINGSSL_PREFIX %+ _dsa_pkey_meth
%xdefine dummy_func_for_constructor BORINGSSL_PREFIX %+ _dummy_func_for_constructor
%xdefine ec_GFp_mont_add BORINGSSL_PREFIX %+ _ec_GFp_mont_add
%xdefine ec_GFp_mont_dbl BORINGSSL_PREFIX %+ _ec_GFp_mont_dbl
%xdefine ec_GFp_mont_felem_exp BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_exp
%xdefine ec_GFp_mont_felem_from_bytes BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_from_bytes
%xdefine ec_GFp_mont_felem_mul BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_mul
%xdefine ec_GFp_mont_felem_reduce BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_reduce
%xdefine ec_GFp_mont_felem_sqr BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_sqr
%xdefine ec_GFp_mont_felem_to_bytes BORINGSSL_PREFIX %+ _ec_GFp_mont_felem_to_bytes
%xdefine ec_GFp_mont_init_precomp BORINGSSL_PREFIX %+ _ec_GFp_mont_init_precomp
%xdefine ec_GFp_mont_mul BORINGSSL_PREFIX %+ _ec_GFp_mont_mul
%xdefine ec_GFp_mont_mul_base BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_base
%xdefine ec_GFp_mont_mul_batch BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_batch
%xdefine ec_GFp_mont_mul_precomp BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_precomp
%xdefine ec_GFp_mont_mul_public_batch BORINGSSL_PREFIX %+ _ec_GFp_mont_mul_public_batch
%xdefine ec_GFp_nistp_recode_scalar_bits BORINGSSL_PREFIX %+ _ec_GFp_nistp_recode_scalar_bits
%xdefine ec_GFp_simple_cmp_x_coordinate BORINGSSL_PREFIX %+ _ec_GFp_simple_cmp_x_coordinate
%xdefine ec_GFp_simple_felem_from_bytes BORINGSSL_PREFIX %+ _ec_GFp_simple_felem_from_bytes
%xdefine ec_GFp_simple_felem_to_bytes BORINGSSL_PREFIX %+ _ec_GFp_simple_felem_to_bytes
%xdefine ec_GFp_simple_group_get_curve BORINGSSL_PREFIX %+ _ec_GFp_simple_group_get_curve
%xdefine ec_GFp_simple_group_set_curve BORINGSSL_PREFIX %+ _ec_GFp_simple_group_set_curve
%xdefine ec_GFp_simple_invert BORINGSSL_PREFIX %+ _ec_GFp_simple_invert
%xdefine ec_GFp_simple_is_at_infinity BORINGSSL_PREFIX %+ _ec_GFp_simple_is_at_infinity
%xdefine ec_GFp_simple_is_on_curve BORINGSSL_PREFIX %+ _ec_GFp_simple_is_on_curve
%xdefine ec_GFp_simple_point_copy BORINGSSL_PREFIX %+ _ec_GFp_simple_point_copy
%xdefine ec_GFp_simple_point_init BORINGSSL_PREFIX %+ _ec_GFp_simple_point_init
%xdefine ec_GFp_simple_point_set_to_infinity BORINGSSL_PREFIX %+ _ec_GFp_simple_point_set_to_infinity
%xdefine ec_GFp_simple_points_equal BORINGSSL_PREFIX %+ _ec_GFp_simple_points_equal
%xdefine ec_affine_jacobian_equal BORINGSSL_PREFIX %+ _ec_affine_jacobian_equal
%xdefine ec_affine_select BORINGSSL_PREFIX %+ _ec_affine_select
%xdefine ec_affine_to_jacobian BORINGSSL_PREFIX %+ _ec_affine_to_jacobian
%xdefine ec_asn1_meth BORINGSSL_PREFIX %+ _ec_asn1_meth
%xdefine ec_bignum_to_felem BORINGSSL_PREFIX %+ _ec_bignum_to_felem
%xdefine ec_bignum_to_scalar BORINGSSL_PREFIX %+ _ec_bignum_to_scalar
%xdefine ec_cmp_x_coordinate BORINGSSL_PREFIX %+ _ec_cmp_x_coordinate
%xdefine ec_compute_wNAF BORINGSSL_PREFIX %+ _ec_compute_wNAF
%xdefine ec_felem_add BORINGSSL_PREFIX %+ _ec_felem_add
%xdefine ec_felem_equal BORINGSSL_PREFIX %+ _ec_felem_equal
%xdefine ec_felem_from_bytes BORINGSSL_PREFIX %+ _ec_felem_from_bytes
%xdefine ec_felem_neg BORINGSSL_PREFIX %+ _ec_felem_neg
%xdefine ec_felem_non_zero_mask BORINGSSL_PREFIX %+ _ec_felem_non_zero_mask
%xdefine ec_felem_one BORINGSSL_PREFIX %+ _ec_felem_one
%xdefine ec_felem_select BORINGSSL_PREFIX %+ _ec_felem_select
%xdefine ec_felem_sub BORINGSSL_PREFIX %+ _ec_felem_sub
%xdefine ec_felem_to_bignum BORINGSSL_PREFIX %+ _ec_felem_to_bignum
%xdefine ec_felem_to_bytes BORINGSSL_PREFIX %+ _ec_felem_to_bytes
%xdefine ec_get_x_coordinate_as_bytes BORINGSSL_PREFIX %+ _ec_get_x_coordinate_as_bytes
%xdefine ec_get_x_coordinate_as_scalar BORINGSSL_PREFIX %+ _ec_get_x_coordinate_as_scalar
%xdefine ec_hash_to_curve_p256_xmd_sha256_sswu BORINGSSL_PREFIX %+ _ec_hash_to_curve_p256_xmd_sha256_sswu
%xdefine ec_hash_to_curve_p384_xmd_sha384_sswu BORINGSSL_PREFIX %+ _ec_hash_to_curve_p384_xmd_sha384_sswu
%xdefine ec_hash_to_curve_p384_xmd_sha512_sswu_draft07 BORINGSSL_PREFIX %+ _ec_hash_to_curve_p384_xmd_sha512_sswu_draft07
%xdefine ec_hash_to_scalar_p384_xmd_sha384 BORINGSSL_PREFIX %+ _ec_hash_to_scalar_p384_xmd_sha384
%xdefine ec_hash_to_scalar_p384_xmd_sha512_draft07 BORINGSSL_PREFIX %+ _ec_hash_to_scalar_p384_xmd_sha512_draft07
%xdefine ec_init_precomp BORINGSSL_PREFIX %+ _ec_init_precomp
%xdefine ec_jacobian_to_affine BORINGSSL_PREFIX %+ _ec_jacobian_to_affine
%xdefine ec_jacobian_to_affine_batch BORINGSSL_PREFIX %+ _ec_jacobian_to_affine_batch
%xdefine ec_nistp_coordinates_to_point BORINGSSL_PREFIX %+ _ec_nistp_coordinates_to_point
%xdefine ec_nistp_point_add BORINGSSL_PREFIX %+ _ec_nistp_point_add
%xdefine ec_nistp_point_double BORINGSSL_PREFIX %+ _ec_nistp_point_double
%xdefine ec_nistp_point_to_coordinates BORINGSSL_PREFIX %+ _ec_nistp_point_to_coordinates
%xdefine ec_nistp_scalar_mul BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul
%xdefine ec_nistp_scalar_mul_base BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul_base
%xdefine ec_nistp_scalar_mul_public BORINGSSL_PREFIX %+ _ec_nistp_scalar_mul_public
%xdefine ec_point_byte_len BORINGSSL_PREFIX %+ _ec_point_byte_len
%xdefine ec_point_from_uncompressed BORINGSSL_PREFIX %+ _ec_point_from_uncompressed
%xdefine ec_point_mul_no_self_test BORINGSSL_PREFIX %+ _ec_point_mul_no_self_test
%xdefine ec_point_mul_scalar BORINGSSL_PREFIX %+ _ec_point_mul_scalar
%xdefine ec_point_mul_scalar_base BORINGSSL_PREFIX %+ _ec_point_mul_scalar_base
%xdefine ec_point_mul_scalar_batch BORINGSSL_PREFIX %+ _ec_point_mul_scalar_batch
%xdefine ec_point_mul_scalar_precomp BORINGSSL_PREFIX %+ _ec_point_mul_scalar_precomp
%xdefine ec_point_mul_scalar_public BORINGSSL_PREFIX %+ _ec_point_mul_scalar_public
%xdefine ec_point_mul_scalar_public_batch BORINGSSL_PREFIX %+ _ec_point_mul_scalar_public_batch
%xdefine ec_point_select BORINGSSL_PREFIX %+ _ec_point_select
%xdefine ec_point_set_affine_coordinates BORINGSSL_PREFIX %+ _ec_point_set_affine_coordinates
%xdefine ec_point_to_bytes BORINGSSL_PREFIX %+ _ec_point_to_bytes
%xdefine ec_precomp_select BORINGSSL_PREFIX %+ _ec_precomp_select
%xdefine ec_random_nonzero_scalar BORINGSSL_PREFIX %+ _ec_random_nonzero_scalar
%xdefine ec_scalar_add BORINGSSL_PREFIX %+ _ec_scalar_add
%xdefine ec_scalar_equal_vartime BORINGSSL_PREFIX %+ _ec_scalar_equal_vartime
%xdefine ec_scalar_from_bytes BORINGSSL_PREFIX %+ _ec_scalar_from_bytes
%xdefine ec_scalar_from_montgomery BORINGSSL_PREFIX %+ _ec_scalar_from_montgomery
%xdefine ec_scalar_inv0_montgomery BORINGSSL_PREFIX %+ _ec_scalar_inv0_montgomery
%xdefine ec_scalar_is_zero BORINGSSL_PREFIX %+ _ec_scalar_is_zero
%xdefine ec_scalar_mul_montgomery BORINGSSL_PREFIX %+ _ec_scalar_mul_montgomery
%xdefine ec_scalar_neg BORINGSSL_PREFIX %+ _ec_scalar_neg
%xdefine ec_scalar_reduce BORINGSSL_PREFIX %+ _ec_scalar_reduce
%xdefine ec_scalar_select BORINGSSL_PREFIX %+ _ec_scalar_select
%xdefine ec_scalar_sub BORINGSSL_PREFIX %+ _ec_scalar_sub
%xdefine ec_scalar_to_bytes BORINGSSL_PREFIX %+ _ec_scalar_to_bytes
%xdefine ec_scalar_to_montgomery BORINGSSL_PREFIX %+ _ec_scalar_to_montgomery
%xdefine ec_scalar_to_montgomery_inv_vartime BORINGSSL_PREFIX %+ _ec_scalar_to_montgomery_inv_vartime
%xdefine ec_set_to_safe_point BORINGSSL_PREFIX %+ _ec_set_to_safe_point
%xdefine ec_simple_scalar_inv0_montgomery BORINGSSL_PREFIX %+ _ec_simple_scalar_inv0_montgomery
%xdefine ec_simple_scalar_to_montgomery_inv_vartime BORINGSSL_PREFIX %+ _ec_simple_scalar_to_montgomery_inv_vartime
%xdefine ecdsa_digestsign_no_self_test BORINGSSL_PREFIX %+ _ecdsa_digestsign_no_self_test
%xdefine ecdsa_digestverify_no_self_test BORINGSSL_PREFIX %+ _ecdsa_digestverify_no_self_test
%xdefine ecdsa_do_verify_no_self_test BORINGSSL_PREFIX %+ _ecdsa_do_verify_no_self_test
%xdefine ecdsa_sign_with_nonce_for_known_answer_test BORINGSSL_PREFIX %+ _ecdsa_sign_with_nonce_for_known_answer_test
%xdefine ecp_nistz256_avx2_select_w7 BORINGSSL_PREFIX %+ _ecp_nistz256_avx2_select_w7
%xdefine ecp_nistz256_div_by_2 BORINGSSL_PREFIX %+ _ecp_nistz256_div_by_2
%xdefine ecp_nistz256_mul_by_2 BORINGSSL_PREFIX %+ _ecp_nistz256_mul_by_2
%xdefine ecp_nistz256_mul_by_3 BORINGSSL_PREFIX %+ _ecp_nistz256_mul_by_3
%xdefine ecp_nistz256_mul_mont BORINGSSL_PREFIX %+ _ecp_nistz256_mul_mont
%xdefine ecp_nistz256_neg BORINGSSL_PREFIX %+ _ecp_nistz256_neg
%xdefine ecp_nistz256_ord_mul_mont BORINGSSL_PREFIX %+ _ecp_nistz256_ord_mul_mont
%xdefine ecp_nistz256_ord_sqr_mont BORINGSSL_PREFIX %+ _ecp_nistz256_ord_sqr_mont
%xdefine ecp_nistz256_point_add BORINGSSL_PREFIX %+ _ecp_nistz256_point_add
%xdefine ecp_nistz256_point_add_affine BORINGSSL_PREFIX %+ _ecp_nistz256_point_add_affine
%xdefine ecp_nistz256_point_double BORINGSSL_PREFIX %+ _ecp_nistz256_point_double
%xdefine ecp_nistz256_select_w5 BORINGSSL_PREFIX %+ _ecp_nistz256_select_w5
%xdefine ecp_nistz256_select_w7 BORINGSSL_PREFIX %+ _ecp_nistz256_select_w7
%xdefine ecp_nistz256_sqr_mont BORINGSSL_PREFIX %+ _ecp_nistz256_sqr_mont
%xdefine ecp_nistz256_sub BORINGSSL_PREFIX %+ _ecp_nistz256_sub
%xdefine ed25519_asn1_meth BORINGSSL_PREFIX %+ _ed25519_asn1_meth
%xdefine ed25519_check_public_key_nohw BORINGSSL_PREFIX %+ _ed25519_check_public_key_nohw
%xdefine ed25519_check_public_key_s2n_bignum BORINGSSL_PREFIX %+ _ed25519_check_public_key_s2n_bignum
%xdefine ed25519_public_key_from_hashed_seed_nohw BORINGSSL_PREFIX %+ _ed25519_public_key_from_hashed_seed_nohw
%xdefine ed25519_public_key_from_hashed_seed_s2n_bignum BORINGSSL_PREFIX %+ _ed25519_public_key_from_hashed_seed_s2n_bignum
%xdefine ed25519_sha512 BORINGSSL_PREFIX %+ _ed25519_sha512
%xdefine ed25519_sign_internal BORINGSSL_PREFIX %+ _ed25519_sign_internal
%xdefine ed25519_sign_nohw BORINGSSL_PREFIX %+ _ed25519_sign_nohw
%xdefine ed25519_sign_s2n_bignum BORINGSSL_PREFIX %+ _ed25519_sign_s2n_bignum
%xdefine ed25519_verify_internal BORINGSSL_PREFIX %+ _ed25519_verify_internal
%xdefine ed25519_verify_nohw BORINGSSL_PREFIX %+ _ed25519_verify_nohw
%xdefine ed25519_verify_s2n_bignum BORINGSSL_PREFIX %+ _ed25519_verify_s2n_bignum
%xdefine ed25519ph_asn1_meth BORINGSSL_PREFIX %+ _ed25519ph_asn1_meth
%xdefine edwards25519_decode BORINGSSL_PREFIX %+ _edwards25519_decode
%xdefine edwards25519_decode_alt BORINGSSL_PREFIX %+ _edwards25519_decode_alt
%xdefine edwards25519_encode BORINGSSL_PREFIX %+ _edwards25519_encode
%xdefine edwards25519_scalarmulbase BORINGSSL_PREFIX %+ _edwards25519_scalarmulbase
%xdefine edwards25519_scalarmulbase_alt BORINGSSL_PREFIX %+ _edwards25519_scalarmulbase_alt
%xdefine edwards25519_scalarmuldouble BORINGSSL_PREFIX %+ _edwards25519_scalarmuldouble
%xdefine edwards25519_scalarmuldouble_alt BORINGSSL_PREFIX %+ _edwards25519_scalarmuldouble_alt
%xdefine evp_pkey_set_cb_translate BORINGSSL_PREFIX %+ _evp_pkey_set_cb_translate
%xdefine evp_pkey_set_method BORINGSSL_PREFIX %+ _evp_pkey_set_method
%xdefine extract_multiplier_2x20_win5 BORINGSSL_PREFIX %+ _extract_multiplier_2x20_win5
%xdefine extract_multiplier_2x30_win5 BORINGSSL_PREFIX %+ _extract_multiplier_2x30_win5
%xdefine extract_multiplier_2x40_win5 BORINGSSL_PREFIX %+ _extract_multiplier_2x40_win5
%xdefine gcm_ghash_avx BORINGSSL_PREFIX %+ _gcm_ghash_avx
%xdefine gcm_ghash_avx512 BORINGSSL_PREFIX %+ _gcm_ghash_avx512
%xdefine gcm_ghash_clmul BORINGSSL_PREFIX %+ _gcm_ghash_clmul
%xdefine gcm_ghash_neon BORINGSSL_PREFIX %+ _gcm_ghash_neon
%xdefine gcm_ghash_nohw BORINGSSL_PREFIX %+ _gcm_ghash_nohw
%xdefine gcm_ghash_p8 BORINGSSL_PREFIX %+ _gcm_ghash_p8
%xdefine gcm_ghash_ssse3 BORINGSSL_PREFIX %+ _gcm_ghash_ssse3
%xdefine gcm_ghash_v8 BORINGSSL_PREFIX %+ _gcm_ghash_v8
%xdefine gcm_gmult_avx BORINGSSL_PREFIX %+ _gcm_gmult_avx
%xdefine gcm_gmult_avx512 BORINGSSL_PREFIX %+ _gcm_gmult_avx512
%xdefine gcm_gmult_clmul BORINGSSL_PREFIX %+ _gcm_gmult_clmul
%xdefine gcm_gmult_neon BORINGSSL_PREFIX %+ _gcm_gmult_neon
%xdefine gcm_gmult_nohw BORINGSSL_PREFIX %+ _gcm_gmult_nohw
%xdefine gcm_gmult_p8 BORINGSSL_PREFIX %+ _gcm_gmult_p8
%xdefine gcm_gmult_ssse3 BORINGSSL_PREFIX %+ _gcm_gmult_ssse3
%xdefine gcm_gmult_v8 BORINGSSL_PREFIX %+ _gcm_gmult_v8
%xdefine gcm_init_avx BORINGSSL_PREFIX %+ _gcm_init_avx
%xdefine gcm_init_avx512 BORINGSSL_PREFIX %+ _gcm_init_avx512
%xdefine gcm_init_clmul BORINGSSL_PREFIX %+ _gcm_init_clmul
%xdefine gcm_init_neon BORINGSSL_PREFIX %+ _gcm_init_neon
%xdefine gcm_init_nohw BORINGSSL_PREFIX %+ _gcm_init_nohw
%xdefine gcm_init_p8 BORINGSSL_PREFIX %+ _gcm_init_p8
%xdefine gcm_init_ssse3 BORINGSSL_PREFIX %+ _gcm_init_ssse3
%xdefine gcm_init_v8 BORINGSSL_PREFIX %+ _gcm_init_v8
%xdefine gcm_setiv_avx512 BORINGSSL_PREFIX %+ _gcm_setiv_avx512
%xdefine get_entropy_source BORINGSSL_PREFIX %+ _get_entropy_source
%xdefine get_entropy_source_method_id_FOR_TESTING BORINGSSL_PREFIX %+ _get_entropy_source_method_id_FOR_TESTING
%xdefine get_thread_and_global_tree_drbg_calls_FOR_TESTING BORINGSSL_PREFIX %+ _get_thread_and_global_tree_drbg_calls_FOR_TESTING
%xdefine get_thread_generate_calls_since_seed BORINGSSL_PREFIX %+ _get_thread_generate_calls_since_seed
%xdefine get_thread_reseed_calls_since_initialization BORINGSSL_PREFIX %+ _get_thread_reseed_calls_since_initialization
%xdefine handle_cpu_env BORINGSSL_PREFIX %+ _handle_cpu_env
%xdefine have_hw_rng_aarch64_for_testing BORINGSSL_PREFIX %+ _have_hw_rng_aarch64_for_testing
%xdefine have_hw_rng_x86_64_for_testing BORINGSSL_PREFIX %+ _have_hw_rng_x86_64_for_testing
%xdefine hmac_asn1_meth BORINGSSL_PREFIX %+ _hmac_asn1_meth
%xdefine i2a_ASN1_ENUMERATED BORINGSSL_PREFIX %+ _i2a_ASN1_ENUMERATED
%xdefine i2a_ASN1_INTEGER BORINGSSL_PREFIX %+ _i2a_ASN1_INTEGER
%xdefine i2a_ASN1_OBJECT BORINGSSL_PREFIX %+ _i2a_ASN1_OBJECT
%xdefine i2a_ASN1_STRING BORINGSSL_PREFIX %+ _i2a_ASN1_STRING
%xdefine i2c_ASN1_BIT_STRING BORINGSSL_PREFIX %+ _i2c_ASN1_BIT_STRING
%xdefine i2c_ASN1_INTEGER BORINGSSL_PREFIX %+ _i2c_ASN1_INTEGER
%xdefine i2d_ASN1_BIT_STRING BORINGSSL_PREFIX %+ _i2d_ASN1_BIT_STRING
%xdefine i2d_ASN1_BMPSTRING BORINGSSL_PREFIX %+ _i2d_ASN1_BMPSTRING
%xdefine i2d_ASN1_BOOLEAN BORINGSSL_PREFIX %+ _i2d_ASN1_BOOLEAN
%xdefine i2d_ASN1_ENUMERATED BORINGSSL_PREFIX %+ _i2d_ASN1_ENUMERATED
%xdefine i2d_ASN1_GENERALIZEDTIME BORINGSSL_PREFIX %+ _i2d_ASN1_GENERALIZEDTIME
%xdefine i2d_ASN1_GENERALSTRING BORINGSSL_PREFIX %+ _i2d_ASN1_GENERALSTRING
%xdefine i2d_ASN1_IA5STRING BORINGSSL_PREFIX %+ _i2d_ASN1_IA5STRING
%xdefine i2d_ASN1_INTEGER BORINGSSL_PREFIX %+ _i2d_ASN1_INTEGER
%xdefine i2d_ASN1_NULL BORINGSSL_PREFIX %+ _i2d_ASN1_NULL
%xdefine i2d_ASN1_OBJECT BORINGSSL_PREFIX %+ _i2d_ASN1_OBJECT
%xdefine i2d_ASN1_OCTET_STRING BORINGSSL_PREFIX %+ _i2d_ASN1_OCTET_STRING
%xdefine i2d_ASN1_PRINTABLE BORINGSSL_PREFIX %+ _i2d_ASN1_PRINTABLE
%xdefine i2d_ASN1_PRINTABLESTRING BORINGSSL_PREFIX %+ _i2d_ASN1_PRINTABLESTRING
%xdefine i2d_ASN1_SEQUENCE_ANY BORINGSSL_PREFIX %+ _i2d_ASN1_SEQUENCE_ANY
%xdefine i2d_ASN1_SET_ANY BORINGSSL_PREFIX %+ _i2d_ASN1_SET_ANY
%xdefine i2d_ASN1_T61STRING BORINGSSL_PREFIX %+ _i2d_ASN1_T61STRING
%xdefine i2d_ASN1_TIME BORINGSSL_PREFIX %+ _i2d_ASN1_TIME
%xdefine i2d_ASN1_TYPE BORINGSSL_PREFIX %+ _i2d_ASN1_TYPE
%xdefine i2d_ASN1_UNIVERSALSTRING BORINGSSL_PREFIX %+ _i2d_ASN1_UNIVERSALSTRING
%xdefine i2d_ASN1_UTCTIME BORINGSSL_PREFIX %+ _i2d_ASN1_UTCTIME
%xdefine i2d_ASN1_UTF8STRING BORINGSSL_PREFIX %+ _i2d_ASN1_UTF8STRING
%xdefine i2d_ASN1_VISIBLESTRING BORINGSSL_PREFIX %+ _i2d_ASN1_VISIBLESTRING
%xdefine i2d_AUTHORITY_INFO_ACCESS BORINGSSL_PREFIX %+ _i2d_AUTHORITY_INFO_ACCESS
%xdefine i2d_AUTHORITY_KEYID BORINGSSL_PREFIX %+ _i2d_AUTHORITY_KEYID
%xdefine i2d_BASIC_CONSTRAINTS BORINGSSL_PREFIX %+ _i2d_BASIC_CONSTRAINTS
%xdefine i2d_CERTIFICATEPOLICIES BORINGSSL_PREFIX %+ _i2d_CERTIFICATEPOLICIES
%xdefine i2d_CRL_DIST_POINTS BORINGSSL_PREFIX %+ _i2d_CRL_DIST_POINTS
%xdefine i2d_DHparams BORINGSSL_PREFIX %+ _i2d_DHparams
%xdefine i2d_DHparams_bio BORINGSSL_PREFIX %+ _i2d_DHparams_bio
%xdefine i2d_DIRECTORYSTRING BORINGSSL_PREFIX %+ _i2d_DIRECTORYSTRING
%xdefine i2d_DISPLAYTEXT BORINGSSL_PREFIX %+ _i2d_DISPLAYTEXT
%xdefine i2d_DSAPrivateKey BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey
%xdefine i2d_DSAPrivateKey_bio BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey_bio
%xdefine i2d_DSAPrivateKey_fp BORINGSSL_PREFIX %+ _i2d_DSAPrivateKey_fp
%xdefine i2d_DSAPublicKey BORINGSSL_PREFIX %+ _i2d_DSAPublicKey
%xdefine i2d_DSA_PUBKEY BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY
%xdefine i2d_DSA_PUBKEY_bio BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY_bio
%xdefine i2d_DSA_PUBKEY_fp BORINGSSL_PREFIX %+ _i2d_DSA_PUBKEY_fp
%xdefine i2d_DSA_SIG BORINGSSL_PREFIX %+ _i2d_DSA_SIG
%xdefine i2d_DSAparams BORINGSSL_PREFIX %+ _i2d_DSAparams
%xdefine i2d_ECDSA_SIG BORINGSSL_PREFIX %+ _i2d_ECDSA_SIG
%xdefine i2d_ECPKParameters BORINGSSL_PREFIX %+ _i2d_ECPKParameters
%xdefine i2d_ECPKParameters_bio BORINGSSL_PREFIX %+ _i2d_ECPKParameters_bio
%xdefine i2d_ECParameters BORINGSSL_PREFIX %+ _i2d_ECParameters
%xdefine i2d_ECPrivateKey BORINGSSL_PREFIX %+ _i2d_ECPrivateKey
%xdefine i2d_ECPrivateKey_bio BORINGSSL_PREFIX %+ _i2d_ECPrivateKey_bio
%xdefine i2d_ECPrivateKey_fp BORINGSSL_PREFIX %+ _i2d_ECPrivateKey_fp
%xdefine i2d_EC_PUBKEY BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY
%xdefine i2d_EC_PUBKEY_bio BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY_bio
%xdefine i2d_EC_PUBKEY_fp BORINGSSL_PREFIX %+ _i2d_EC_PUBKEY_fp
%xdefine i2d_EXTENDED_KEY_USAGE BORINGSSL_PREFIX %+ _i2d_EXTENDED_KEY_USAGE
%xdefine i2d_GENERAL_NAME BORINGSSL_PREFIX %+ _i2d_GENERAL_NAME
%xdefine i2d_GENERAL_NAMES BORINGSSL_PREFIX %+ _i2d_GENERAL_NAMES
%xdefine i2d_ISSUING_DIST_POINT BORINGSSL_PREFIX %+ _i2d_ISSUING_DIST_POINT
%xdefine i2d_NETSCAPE_SPKAC BORINGSSL_PREFIX %+ _i2d_NETSCAPE_SPKAC
%xdefine i2d_NETSCAPE_SPKI BORINGSSL_PREFIX %+ _i2d_NETSCAPE_SPKI
%xdefine i2d_OCSP_BASICRESP BORINGSSL_PREFIX %+ _i2d_OCSP_BASICRESP
%xdefine i2d_OCSP_CERTID BORINGSSL_PREFIX %+ _i2d_OCSP_CERTID
%xdefine i2d_OCSP_ONEREQ BORINGSSL_PREFIX %+ _i2d_OCSP_ONEREQ
%xdefine i2d_OCSP_REQINFO BORINGSSL_PREFIX %+ _i2d_OCSP_REQINFO
%xdefine i2d_OCSP_REQUEST BORINGSSL_PREFIX %+ _i2d_OCSP_REQUEST
%xdefine i2d_OCSP_REQUEST_bio BORINGSSL_PREFIX %+ _i2d_OCSP_REQUEST_bio
%xdefine i2d_OCSP_RESPBYTES BORINGSSL_PREFIX %+ _i2d_OCSP_RESPBYTES
%xdefine i2d_OCSP_RESPDATA BORINGSSL_PREFIX %+ _i2d_OCSP_RESPDATA
%xdefine i2d_OCSP_RESPONSE BORINGSSL_PREFIX %+ _i2d_OCSP_RESPONSE
%xdefine i2d_OCSP_RESPONSE_bio BORINGSSL_PREFIX %+ _i2d_OCSP_RESPONSE_bio
%xdefine i2d_OCSP_REVOKEDINFO BORINGSSL_PREFIX %+ _i2d_OCSP_REVOKEDINFO
%xdefine i2d_OCSP_SIGNATURE BORINGSSL_PREFIX %+ _i2d_OCSP_SIGNATURE
%xdefine i2d_OCSP_SINGLERESP BORINGSSL_PREFIX %+ _i2d_OCSP_SINGLERESP
%xdefine i2d_PKCS12 BORINGSSL_PREFIX %+ _i2d_PKCS12
%xdefine i2d_PKCS12_bio BORINGSSL_PREFIX %+ _i2d_PKCS12_bio
%xdefine i2d_PKCS12_fp BORINGSSL_PREFIX %+ _i2d_PKCS12_fp
%xdefine i2d_PKCS7 BORINGSSL_PREFIX %+ _i2d_PKCS7
%xdefine i2d_PKCS7_DIGEST BORINGSSL_PREFIX %+ _i2d_PKCS7_DIGEST
%xdefine i2d_PKCS7_ENCRYPT BORINGSSL_PREFIX %+ _i2d_PKCS7_ENCRYPT
%xdefine i2d_PKCS7_ENC_CONTENT BORINGSSL_PREFIX %+ _i2d_PKCS7_ENC_CONTENT
%xdefine i2d_PKCS7_ENVELOPE BORINGSSL_PREFIX %+ _i2d_PKCS7_ENVELOPE
%xdefine i2d_PKCS7_ISSUER_AND_SERIAL BORINGSSL_PREFIX %+ _i2d_PKCS7_ISSUER_AND_SERIAL
%xdefine i2d_PKCS7_RECIP_INFO BORINGSSL_PREFIX %+ _i2d_PKCS7_RECIP_INFO
%xdefine i2d_PKCS7_SIGNED BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGNED
%xdefine i2d_PKCS7_SIGNER_INFO BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGNER_INFO
%xdefine i2d_PKCS7_SIGN_ENVELOPE BORINGSSL_PREFIX %+ _i2d_PKCS7_SIGN_ENVELOPE
%xdefine i2d_PKCS7_bio BORINGSSL_PREFIX %+ _i2d_PKCS7_bio
%xdefine i2d_PKCS8PrivateKeyInfo_bio BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKeyInfo_bio
%xdefine i2d_PKCS8PrivateKeyInfo_fp BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKeyInfo_fp
%xdefine i2d_PKCS8PrivateKey_bio BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_bio
%xdefine i2d_PKCS8PrivateKey_fp BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_fp
%xdefine i2d_PKCS8PrivateKey_nid_bio BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_nid_bio
%xdefine i2d_PKCS8PrivateKey_nid_fp BORINGSSL_PREFIX %+ _i2d_PKCS8PrivateKey_nid_fp
%xdefine i2d_PKCS8_PRIV_KEY_INFO BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO
%xdefine i2d_PKCS8_PRIV_KEY_INFO_bio BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO_bio
%xdefine i2d_PKCS8_PRIV_KEY_INFO_fp BORINGSSL_PREFIX %+ _i2d_PKCS8_PRIV_KEY_INFO_fp
%xdefine i2d_PKCS8_bio BORINGSSL_PREFIX %+ _i2d_PKCS8_bio
%xdefine i2d_PKCS8_fp BORINGSSL_PREFIX %+ _i2d_PKCS8_fp
%xdefine i2d_PUBKEY BORINGSSL_PREFIX %+ _i2d_PUBKEY
%xdefine i2d_PUBKEY_bio BORINGSSL_PREFIX %+ _i2d_PUBKEY_bio
%xdefine i2d_PUBKEY_fp BORINGSSL_PREFIX %+ _i2d_PUBKEY_fp
%xdefine i2d_PrivateKey BORINGSSL_PREFIX %+ _i2d_PrivateKey
%xdefine i2d_PrivateKey_bio BORINGSSL_PREFIX %+ _i2d_PrivateKey_bio
%xdefine i2d_PrivateKey_fp BORINGSSL_PREFIX %+ _i2d_PrivateKey_fp
%xdefine i2d_PublicKey BORINGSSL_PREFIX %+ _i2d_PublicKey
%xdefine i2d_RSAPrivateKey BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey
%xdefine i2d_RSAPrivateKey_bio BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey_bio
%xdefine i2d_RSAPrivateKey_fp BORINGSSL_PREFIX %+ _i2d_RSAPrivateKey_fp
%xdefine i2d_RSAPublicKey BORINGSSL_PREFIX %+ _i2d_RSAPublicKey
%xdefine i2d_RSAPublicKey_bio BORINGSSL_PREFIX %+ _i2d_RSAPublicKey_bio
%xdefine i2d_RSAPublicKey_fp BORINGSSL_PREFIX %+ _i2d_RSAPublicKey_fp
%xdefine i2d_RSA_PSS_PARAMS BORINGSSL_PREFIX %+ _i2d_RSA_PSS_PARAMS
%xdefine i2d_RSA_PUBKEY BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY
%xdefine i2d_RSA_PUBKEY_bio BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY_bio
%xdefine i2d_RSA_PUBKEY_fp BORINGSSL_PREFIX %+ _i2d_RSA_PUBKEY_fp
%xdefine i2d_X509 BORINGSSL_PREFIX %+ _i2d_X509
%xdefine i2d_X509_ALGOR BORINGSSL_PREFIX %+ _i2d_X509_ALGOR
%xdefine i2d_X509_ATTRIBUTE BORINGSSL_PREFIX %+ _i2d_X509_ATTRIBUTE
%xdefine i2d_X509_AUX BORINGSSL_PREFIX %+ _i2d_X509_AUX
%xdefine i2d_X509_CERT_AUX BORINGSSL_PREFIX %+ _i2d_X509_CERT_AUX
%xdefine i2d_X509_CINF BORINGSSL_PREFIX %+ _i2d_X509_CINF
%xdefine i2d_X509_CRL BORINGSSL_PREFIX %+ _i2d_X509_CRL
%xdefine i2d_X509_CRL_INFO BORINGSSL_PREFIX %+ _i2d_X509_CRL_INFO
%xdefine i2d_X509_CRL_bio BORINGSSL_PREFIX %+ _i2d_X509_CRL_bio
%xdefine i2d_X509_CRL_fp BORINGSSL_PREFIX %+ _i2d_X509_CRL_fp
%xdefine i2d_X509_CRL_tbs BORINGSSL_PREFIX %+ _i2d_X509_CRL_tbs
%xdefine i2d_X509_EXTENSION BORINGSSL_PREFIX %+ _i2d_X509_EXTENSION
%xdefine i2d_X509_EXTENSIONS BORINGSSL_PREFIX %+ _i2d_X509_EXTENSIONS
%xdefine i2d_X509_NAME BORINGSSL_PREFIX %+ _i2d_X509_NAME
%xdefine i2d_X509_NAME_ENTRY BORINGSSL_PREFIX %+ _i2d_X509_NAME_ENTRY
%xdefine i2d_X509_PUBKEY BORINGSSL_PREFIX %+ _i2d_X509_PUBKEY
%xdefine i2d_X509_REQ BORINGSSL_PREFIX %+ _i2d_X509_REQ
%xdefine i2d_X509_REQ_INFO BORINGSSL_PREFIX %+ _i2d_X509_REQ_INFO
%xdefine i2d_X509_REQ_bio BORINGSSL_PREFIX %+ _i2d_X509_REQ_bio
%xdefine i2d_X509_REQ_fp BORINGSSL_PREFIX %+ _i2d_X509_REQ_fp
%xdefine i2d_X509_REVOKED BORINGSSL_PREFIX %+ _i2d_X509_REVOKED
%xdefine i2d_X509_SIG BORINGSSL_PREFIX %+ _i2d_X509_SIG
%xdefine i2d_X509_VAL BORINGSSL_PREFIX %+ _i2d_X509_VAL
%xdefine i2d_X509_bio BORINGSSL_PREFIX %+ _i2d_X509_bio
%xdefine i2d_X509_fp BORINGSSL_PREFIX %+ _i2d_X509_fp
%xdefine i2d_X509_tbs BORINGSSL_PREFIX %+ _i2d_X509_tbs
%xdefine i2d_re_X509_CRL_tbs BORINGSSL_PREFIX %+ _i2d_re_X509_CRL_tbs
%xdefine i2d_re_X509_REQ_tbs BORINGSSL_PREFIX %+ _i2d_re_X509_REQ_tbs
%xdefine i2d_re_X509_tbs BORINGSSL_PREFIX %+ _i2d_re_X509_tbs
%xdefine i2o_ECPublicKey BORINGSSL_PREFIX %+ _i2o_ECPublicKey
%xdefine i2s_ASN1_ENUMERATED BORINGSSL_PREFIX %+ _i2s_ASN1_ENUMERATED
%xdefine i2s_ASN1_INTEGER BORINGSSL_PREFIX %+ _i2s_ASN1_INTEGER
%xdefine i2s_ASN1_OCTET_STRING BORINGSSL_PREFIX %+ _i2s_ASN1_OCTET_STRING
%xdefine i2t_ASN1_OBJECT BORINGSSL_PREFIX %+ _i2t_ASN1_OBJECT
%xdefine i2v_GENERAL_NAME BORINGSSL_PREFIX %+ _i2v_GENERAL_NAME
%xdefine i2v_GENERAL_NAMES BORINGSSL_PREFIX %+ _i2v_GENERAL_NAMES
%xdefine is_a_tty BORINGSSL_PREFIX %+ _is_a_tty
%xdefine is_fips_build BORINGSSL_PREFIX %+ _is_fips_build
%xdefine is_public_component_of_rsa_key_good BORINGSSL_PREFIX %+ _is_public_component_of_rsa_key_good
%xdefine jent_apt_init BORINGSSL_PREFIX %+ _jent_apt_init
%xdefine jent_apt_reinit BORINGSSL_PREFIX %+ _jent_apt_reinit
%xdefine jent_entropy_collector_alloc BORINGSSL_PREFIX %+ _jent_entropy_collector_alloc
%xdefine jent_entropy_collector_free BORINGSSL_PREFIX %+ _jent_entropy_collector_free
%xdefine jent_entropy_init BORINGSSL_PREFIX %+ _jent_entropy_init
%xdefine jent_entropy_init_ex BORINGSSL_PREFIX %+ _jent_entropy_init_ex
%xdefine jent_entropy_switch_notime_impl BORINGSSL_PREFIX %+ _jent_entropy_switch_notime_impl
%xdefine jent_gcd_analyze BORINGSSL_PREFIX %+ _jent_gcd_analyze
%xdefine jent_gcd_fini BORINGSSL_PREFIX %+ _jent_gcd_fini
%xdefine jent_gcd_get BORINGSSL_PREFIX %+ _jent_gcd_get
%xdefine jent_gcd_init BORINGSSL_PREFIX %+ _jent_gcd_init
%xdefine jent_gcd_selftest BORINGSSL_PREFIX %+ _jent_gcd_selftest
%xdefine jent_health_cb_block_switch BORINGSSL_PREFIX %+ _jent_health_cb_block_switch
%xdefine jent_health_failure BORINGSSL_PREFIX %+ _jent_health_failure
%xdefine jent_lag_init BORINGSSL_PREFIX %+ _jent_lag_init
%xdefine jent_measure_jitter BORINGSSL_PREFIX %+ _jent_measure_jitter
%xdefine jent_random_data BORINGSSL_PREFIX %+ _jent_random_data
%xdefine jent_read_entropy BORINGSSL_PREFIX %+ _jent_read_entropy
%xdefine jent_read_entropy_safe BORINGSSL_PREFIX %+ _jent_read_entropy_safe
%xdefine jent_read_random_block BORINGSSL_PREFIX %+ _jent_read_random_block
%xdefine jent_set_fips_failure_callback BORINGSSL_PREFIX %+ _jent_set_fips_failure_callback
%xdefine jent_set_fips_failure_callback_internal BORINGSSL_PREFIX %+ _jent_set_fips_failure_callback_internal
%xdefine jent_sha3_256_init BORINGSSL_PREFIX %+ _jent_sha3_256_init
%xdefine jent_sha3_alloc BORINGSSL_PREFIX %+ _jent_sha3_alloc
%xdefine jent_sha3_dealloc BORINGSSL_PREFIX %+ _jent_sha3_dealloc
%xdefine jent_sha3_final BORINGSSL_PREFIX %+ _jent_sha3_final
%xdefine jent_sha3_tester BORINGSSL_PREFIX %+ _jent_sha3_tester
%xdefine jent_sha3_update BORINGSSL_PREFIX %+ _jent_sha3_update
%xdefine jent_stuck BORINGSSL_PREFIX %+ _jent_stuck
%xdefine jent_time_entropy_init BORINGSSL_PREFIX %+ _jent_time_entropy_init
%xdefine jent_version BORINGSSL_PREFIX %+ _jent_version
%xdefine kBoringSSLRSASqrtTwo BORINGSSL_PREFIX %+ _kBoringSSLRSASqrtTwo
%xdefine kBoringSSLRSASqrtTwoLen BORINGSSL_PREFIX %+ _kBoringSSLRSASqrtTwoLen
%xdefine kOpenSSLReasonStringData BORINGSSL_PREFIX %+ _kOpenSSLReasonStringData
%xdefine kOpenSSLReasonValues BORINGSSL_PREFIX %+ _kOpenSSLReasonValues
%xdefine kOpenSSLReasonValuesLen BORINGSSL_PREFIX %+ _kOpenSSLReasonValuesLen
%xdefine kem_asn1_meth BORINGSSL_PREFIX %+ _kem_asn1_meth
%xdefine lh_doall_arg BORINGSSL_PREFIX %+ _lh_doall_arg
%xdefine library_init_constructor BORINGSSL_PREFIX %+ _library_init_constructor
%xdefine md4_block_data_order BORINGSSL_PREFIX %+ _md4_block_data_order
%xdefine md5_block_asm_data_order BORINGSSL_PREFIX %+ _md5_block_asm_data_order
%xdefine ml_dsa_44_keypair BORINGSSL_PREFIX %+ _ml_dsa_44_keypair
%xdefine ml_dsa_44_keypair_internal BORINGSSL_PREFIX %+ _ml_dsa_44_keypair_internal
%xdefine ml_dsa_44_keypair_internal_no_self_test BORINGSSL_PREFIX %+ _ml_dsa_44_keypair_internal_no_self_test
%xdefine ml_dsa_44_pack_pk_from_sk BORINGSSL_PREFIX %+ _ml_dsa_44_pack_pk_from_sk
%xdefine ml_dsa_44_sign BORINGSSL_PREFIX %+ _ml_dsa_44_sign
%xdefine ml_dsa_44_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_44_sign_internal
%xdefine ml_dsa_44_sign_internal_no_self_test BORINGSSL_PREFIX %+ _ml_dsa_44_sign_internal_no_self_test
%xdefine ml_dsa_44_verify BORINGSSL_PREFIX %+ _ml_dsa_44_verify
%xdefine ml_dsa_44_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_44_verify_internal
%xdefine ml_dsa_44_verify_internal_no_self_test BORINGSSL_PREFIX %+ _ml_dsa_44_verify_internal_no_self_test
%xdefine ml_dsa_65_keypair BORINGSSL_PREFIX %+ _ml_dsa_65_keypair
%xdefine ml_dsa_65_keypair_internal BORINGSSL_PREFIX %+ _ml_dsa_65_keypair_internal
%xdefine ml_dsa_65_pack_pk_from_sk BORINGSSL_PREFIX %+ _ml_dsa_65_pack_pk_from_sk
%xdefine ml_dsa_65_sign BORINGSSL_PREFIX %+ _ml_dsa_65_sign
%xdefine ml_dsa_65_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_65_sign_internal
%xdefine ml_dsa_65_verify BORINGSSL_PREFIX %+ _ml_dsa_65_verify
%xdefine ml_dsa_65_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_65_verify_internal
%xdefine ml_dsa_87_keypair BORINGSSL_PREFIX %+ _ml_dsa_87_keypair
%xdefine ml_dsa_87_keypair_internal BORINGSSL_PREFIX %+ _ml_dsa_87_keypair_internal
%xdefine ml_dsa_87_pack_pk_from_sk BORINGSSL_PREFIX %+ _ml_dsa_87_pack_pk_from_sk
%xdefine ml_dsa_87_sign BORINGSSL_PREFIX %+ _ml_dsa_87_sign
%xdefine ml_dsa_87_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_87_sign_internal
%xdefine ml_dsa_87_verify BORINGSSL_PREFIX %+ _ml_dsa_87_verify
%xdefine ml_dsa_87_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_87_verify_internal
%xdefine ml_dsa_extmu_44_sign BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_sign
%xdefine ml_dsa_extmu_44_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_sign_internal
%xdefine ml_dsa_extmu_44_verify BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_verify
%xdefine ml_dsa_extmu_44_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_44_verify_internal
%xdefine ml_dsa_extmu_65_sign BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_sign
%xdefine ml_dsa_extmu_65_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_sign_internal
%xdefine ml_dsa_extmu_65_verify BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_verify
%xdefine ml_dsa_extmu_65_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_65_verify_internal
%xdefine ml_dsa_extmu_87_sign BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_sign
%xdefine ml_dsa_extmu_87_sign_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_sign_internal
%xdefine ml_dsa_extmu_87_verify BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_verify
%xdefine ml_dsa_extmu_87_verify_internal BORINGSSL_PREFIX %+ _ml_dsa_extmu_87_verify_internal
%xdefine ml_kem_1024_check_pk BORINGSSL_PREFIX %+ _ml_kem_1024_check_pk
%xdefine ml_kem_1024_check_sk BORINGSSL_PREFIX %+ _ml_kem_1024_check_sk
%xdefine ml_kem_1024_decapsulate BORINGSSL_PREFIX %+ _ml_kem_1024_decapsulate
%xdefine ml_kem_1024_decapsulate_no_self_test BORINGSSL_PREFIX %+ _ml_kem_1024_decapsulate_no_self_test
%xdefine ml_kem_1024_encapsulate BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate
%xdefine ml_kem_1024_encapsulate_deterministic BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate_deterministic
%xdefine ml_kem_1024_encapsulate_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_1024_encapsulate_deterministic_no_self_test
%xdefine ml_kem_1024_keypair BORINGSSL_PREFIX %+ _ml_kem_1024_keypair
%xdefine ml_kem_1024_keypair_deterministic BORINGSSL_PREFIX %+ _ml_kem_1024_keypair_deterministic
%xdefine ml_kem_1024_keypair_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_1024_keypair_deterministic_no_self_test
%xdefine ml_kem_512_check_pk BORINGSSL_PREFIX %+ _ml_kem_512_check_pk
%xdefine ml_kem_512_check_sk BORINGSSL_PREFIX %+ _ml_kem_512_check_sk
%xdefine ml_kem_512_decapsulate BORINGSSL_PREFIX %+ _ml_kem_512_decapsulate
%xdefine ml_kem_512_decapsulate_no_self_test BORINGSSL_PREFIX %+ _ml_kem_512_decapsulate_no_self_test
%xdefine ml_kem_512_encapsulate BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate
%xdefine ml_kem_512_encapsulate_deterministic BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate_deterministic
%xdefine ml_kem_512_encapsulate_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_512_encapsulate_deterministic_no_self_test
%xdefine ml_kem_512_keypair BORINGSSL_PREFIX %+ _ml_kem_512_keypair
%xdefine ml_kem_512_keypair_deterministic BORINGSSL_PREFIX %+ _ml_kem_512_keypair_deterministic
%xdefine ml_kem_512_keypair_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_512_keypair_deterministic_no_self_test
%xdefine ml_kem_768_check_pk BORINGSSL_PREFIX %+ _ml_kem_768_check_pk
%xdefine ml_kem_768_check_sk BORINGSSL_PREFIX %+ _ml_kem_768_check_sk
%xdefine ml_kem_768_decapsulate BORINGSSL_PREFIX %+ _ml_kem_768_decapsulate
%xdefine ml_kem_768_decapsulate_no_self_test BORINGSSL_PREFIX %+ _ml_kem_768_decapsulate_no_self_test
%xdefine ml_kem_768_encapsulate BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate
%xdefine ml_kem_768_encapsulate_deterministic BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate_deterministic
%xdefine ml_kem_768_encapsulate_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_768_encapsulate_deterministic_no_self_test
%xdefine ml_kem_768_keypair BORINGSSL_PREFIX %+ _ml_kem_768_keypair
%xdefine ml_kem_768_keypair_deterministic BORINGSSL_PREFIX %+ _ml_kem_768_keypair_deterministic
%xdefine ml_kem_768_keypair_deterministic_no_self_test BORINGSSL_PREFIX %+ _ml_kem_768_keypair_deterministic_no_self_test
%xdefine ml_kem_common_decapsulate BORINGSSL_PREFIX %+ _ml_kem_common_decapsulate
%xdefine ml_kem_common_encapsulate BORINGSSL_PREFIX %+ _ml_kem_common_encapsulate
%xdefine ml_kem_common_encapsulate_deterministic BORINGSSL_PREFIX %+ _ml_kem_common_encapsulate_deterministic
%xdefine ml_kem_common_keypair BORINGSSL_PREFIX %+ _ml_kem_common_keypair
%xdefine mldsa_ct_opt_blocker_u64 BORINGSSL_PREFIX %+ _mldsa_ct_opt_blocker_u64
%xdefine mlkem_aarch64_invntt_zetas_layer12345 BORINGSSL_PREFIX %+ _mlkem_aarch64_invntt_zetas_layer12345
%xdefine mlkem_aarch64_invntt_zetas_layer67 BORINGSSL_PREFIX %+ _mlkem_aarch64_invntt_zetas_layer67
%xdefine mlkem_aarch64_ntt_zetas_layer12345 BORINGSSL_PREFIX %+ _mlkem_aarch64_ntt_zetas_layer12345
%xdefine mlkem_aarch64_ntt_zetas_layer67 BORINGSSL_PREFIX %+ _mlkem_aarch64_ntt_zetas_layer67
%xdefine mlkem_aarch64_zetas_mulcache_native BORINGSSL_PREFIX %+ _mlkem_aarch64_zetas_mulcache_native
%xdefine mlkem_aarch64_zetas_mulcache_twisted_native BORINGSSL_PREFIX %+ _mlkem_aarch64_zetas_mulcache_twisted_native
%xdefine mlkem_ct_opt_blocker_u64 BORINGSSL_PREFIX %+ _mlkem_ct_opt_blocker_u64
%xdefine mlkem_intt_asm BORINGSSL_PREFIX %+ _mlkem_intt_asm
%xdefine mlkem_invntt_avx2 BORINGSSL_PREFIX %+ _mlkem_invntt_avx2
%xdefine mlkem_ntt_asm BORINGSSL_PREFIX %+ _mlkem_ntt_asm
%xdefine mlkem_ntt_avx2 BORINGSSL_PREFIX %+ _mlkem_ntt_avx2
%xdefine mlkem_nttfrombytes_avx2 BORINGSSL_PREFIX %+ _mlkem_nttfrombytes_avx2
%xdefine mlkem_ntttobytes_avx2 BORINGSSL_PREFIX %+ _mlkem_ntttobytes_avx2
%xdefine mlkem_nttunpack_avx2 BORINGSSL_PREFIX %+ _mlkem_nttunpack_avx2
%xdefine mlkem_poly_mulcache_compute_asm BORINGSSL_PREFIX %+ _mlkem_poly_mulcache_compute_asm
%xdefine mlkem_poly_mulcache_compute_avx2 BORINGSSL_PREFIX %+ _mlkem_poly_mulcache_compute_avx2
%xdefine mlkem_poly_reduce_asm BORINGSSL_PREFIX %+ _mlkem_poly_reduce_asm
%xdefine mlkem_poly_tobytes_asm BORINGSSL_PREFIX %+ _mlkem_poly_tobytes_asm
%xdefine mlkem_poly_tomont_asm BORINGSSL_PREFIX %+ _mlkem_poly_tomont_asm
%xdefine mlkem_polyvec_basemul_acc_montgomery_cached_asm_k2 BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k2
%xdefine mlkem_polyvec_basemul_acc_montgomery_cached_asm_k3 BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k3
%xdefine mlkem_polyvec_basemul_acc_montgomery_cached_asm_k4 BORINGSSL_PREFIX %+ _mlkem_polyvec_basemul_acc_montgomery_cached_asm_k4
%xdefine mlkem_qdata BORINGSSL_PREFIX %+ _mlkem_qdata
%xdefine mlkem_reduce_avx2 BORINGSSL_PREFIX %+ _mlkem_reduce_avx2
%xdefine mlkem_rej_uniform_asm BORINGSSL_PREFIX %+ _mlkem_rej_uniform_asm
%xdefine mlkem_rej_uniform_table BORINGSSL_PREFIX %+ _mlkem_rej_uniform_table
%xdefine mlkem_tomont_avx2 BORINGSSL_PREFIX %+ _mlkem_tomont_avx2
%xdefine o2i_ECPublicKey BORINGSSL_PREFIX %+ _o2i_ECPublicKey
%xdefine openssl_console_acquire_mutex BORINGSSL_PREFIX %+ _openssl_console_acquire_mutex
%xdefine openssl_console_close BORINGSSL_PREFIX %+ _openssl_console_close
%xdefine openssl_console_open BORINGSSL_PREFIX %+ _openssl_console_open
%xdefine openssl_console_read BORINGSSL_PREFIX %+ _openssl_console_read
%xdefine openssl_console_release_mutex BORINGSSL_PREFIX %+ _openssl_console_release_mutex
%xdefine openssl_console_write BORINGSSL_PREFIX %+ _openssl_console_write
%xdefine openssl_poly1305_neon2_addmulmod BORINGSSL_PREFIX %+ _openssl_poly1305_neon2_addmulmod
%xdefine openssl_poly1305_neon2_blocks BORINGSSL_PREFIX %+ _openssl_poly1305_neon2_blocks
%xdefine override_entropy_source_method_FOR_TESTING BORINGSSL_PREFIX %+ _override_entropy_source_method_FOR_TESTING
%xdefine p256_methods BORINGSSL_PREFIX %+ _p256_methods
%xdefine p256_montjscalarmul BORINGSSL_PREFIX %+ _p256_montjscalarmul
%xdefine p256_montjscalarmul_alt BORINGSSL_PREFIX %+ _p256_montjscalarmul_alt
%xdefine p384_methods BORINGSSL_PREFIX %+ _p384_methods
%xdefine p384_montjdouble BORINGSSL_PREFIX %+ _p384_montjdouble
%xdefine p384_montjdouble_alt BORINGSSL_PREFIX %+ _p384_montjdouble_alt
%xdefine p384_montjscalarmul BORINGSSL_PREFIX %+ _p384_montjscalarmul
%xdefine p384_montjscalarmul_alt BORINGSSL_PREFIX %+ _p384_montjscalarmul_alt
%xdefine p521_jdouble BORINGSSL_PREFIX %+ _p521_jdouble
%xdefine p521_jdouble_alt BORINGSSL_PREFIX %+ _p521_jdouble_alt
%xdefine p521_jscalarmul BORINGSSL_PREFIX %+ _p521_jscalarmul
%xdefine p521_jscalarmul_alt BORINGSSL_PREFIX %+ _p521_jscalarmul_alt
%xdefine p521_methods BORINGSSL_PREFIX %+ _p521_methods
%xdefine p_thread_callback_boringssl BORINGSSL_PREFIX %+ _p_thread_callback_boringssl
%xdefine pkcs12_iterations_acceptable BORINGSSL_PREFIX %+ _pkcs12_iterations_acceptable
%xdefine pkcs12_key_gen BORINGSSL_PREFIX %+ _pkcs12_key_gen
%xdefine pkcs12_pbe_encrypt_init BORINGSSL_PREFIX %+ _pkcs12_pbe_encrypt_init
%xdefine pkcs7_add_signed_data BORINGSSL_PREFIX %+ _pkcs7_add_signed_data
%xdefine pkcs7_final BORINGSSL_PREFIX %+ _pkcs7_final
%xdefine pkcs7_parse_header BORINGSSL_PREFIX %+ _pkcs7_parse_header
%xdefine pkcs8_pbe_decrypt BORINGSSL_PREFIX %+ _pkcs8_pbe_decrypt
%xdefine pmbtoken_exp1_blind BORINGSSL_PREFIX %+ _pmbtoken_exp1_blind
%xdefine pmbtoken_exp1_client_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_exp1_client_key_from_bytes
%xdefine pmbtoken_exp1_derive_key_from_secret BORINGSSL_PREFIX %+ _pmbtoken_exp1_derive_key_from_secret
%xdefine pmbtoken_exp1_generate_key BORINGSSL_PREFIX %+ _pmbtoken_exp1_generate_key
%xdefine pmbtoken_exp1_get_h_for_testing BORINGSSL_PREFIX %+ _pmbtoken_exp1_get_h_for_testing
%xdefine pmbtoken_exp1_issuer_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_exp1_issuer_key_from_bytes
%xdefine pmbtoken_exp1_read BORINGSSL_PREFIX %+ _pmbtoken_exp1_read
%xdefine pmbtoken_exp1_sign BORINGSSL_PREFIX %+ _pmbtoken_exp1_sign
%xdefine pmbtoken_exp1_unblind BORINGSSL_PREFIX %+ _pmbtoken_exp1_unblind
%xdefine pmbtoken_exp2_blind BORINGSSL_PREFIX %+ _pmbtoken_exp2_blind
%xdefine pmbtoken_exp2_client_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_exp2_client_key_from_bytes
%xdefine pmbtoken_exp2_derive_key_from_secret BORINGSSL_PREFIX %+ _pmbtoken_exp2_derive_key_from_secret
%xdefine pmbtoken_exp2_generate_key BORINGSSL_PREFIX %+ _pmbtoken_exp2_generate_key
%xdefine pmbtoken_exp2_get_h_for_testing BORINGSSL_PREFIX %+ _pmbtoken_exp2_get_h_for_testing
%xdefine pmbtoken_exp2_issuer_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_exp2_issuer_key_from_bytes
%xdefine pmbtoken_exp2_read BORINGSSL_PREFIX %+ _pmbtoken_exp2_read
%xdefine pmbtoken_exp2_sign BORINGSSL_PREFIX %+ _pmbtoken_exp2_sign
%xdefine pmbtoken_exp2_unblind BORINGSSL_PREFIX %+ _pmbtoken_exp2_unblind
%xdefine pmbtoken_pst1_blind BORINGSSL_PREFIX %+ _pmbtoken_pst1_blind
%xdefine pmbtoken_pst1_client_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_pst1_client_key_from_bytes
%xdefine pmbtoken_pst1_derive_key_from_secret BORINGSSL_PREFIX %+ _pmbtoken_pst1_derive_key_from_secret
%xdefine pmbtoken_pst1_generate_key BORINGSSL_PREFIX %+ _pmbtoken_pst1_generate_key
%xdefine pmbtoken_pst1_get_h_for_testing BORINGSSL_PREFIX %+ _pmbtoken_pst1_get_h_for_testing
%xdefine pmbtoken_pst1_issuer_key_from_bytes BORINGSSL_PREFIX %+ _pmbtoken_pst1_issuer_key_from_bytes
%xdefine pmbtoken_pst1_read BORINGSSL_PREFIX %+ _pmbtoken_pst1_read
%xdefine pmbtoken_pst1_sign BORINGSSL_PREFIX %+ _pmbtoken_pst1_sign
%xdefine pmbtoken_pst1_unblind BORINGSSL_PREFIX %+ _pmbtoken_pst1_unblind
%xdefine poly_Rq_mul BORINGSSL_PREFIX %+ _poly_Rq_mul
%xdefine pqdsa_asn1_meth BORINGSSL_PREFIX %+ _pqdsa_asn1_meth
%xdefine rand_fips_library_destructor BORINGSSL_PREFIX %+ _rand_fips_library_destructor
%xdefine rdrand_multiple8 BORINGSSL_PREFIX %+ _rdrand_multiple8
%xdefine rndr_multiple8 BORINGSSL_PREFIX %+ _rndr_multiple8
%xdefine rsa_asn1_meth BORINGSSL_PREFIX %+ _rsa_asn1_meth
%xdefine rsa_default_private_transform BORINGSSL_PREFIX %+ _rsa_default_private_transform
%xdefine rsa_default_sign_raw BORINGSSL_PREFIX %+ _rsa_default_sign_raw
%xdefine rsa_default_size BORINGSSL_PREFIX %+ _rsa_default_size
%xdefine rsa_digestsign_no_self_test BORINGSSL_PREFIX %+ _rsa_digestsign_no_self_test
%xdefine rsa_digestverify_no_self_test BORINGSSL_PREFIX %+ _rsa_digestverify_no_self_test
%xdefine rsa_invalidate_key BORINGSSL_PREFIX %+ _rsa_invalidate_key
%xdefine rsa_private_transform BORINGSSL_PREFIX %+ _rsa_private_transform
%xdefine rsa_private_transform_no_self_test BORINGSSL_PREFIX %+ _rsa_private_transform_no_self_test
%xdefine rsa_pss_asn1_meth BORINGSSL_PREFIX %+ _rsa_pss_asn1_meth
%xdefine rsa_sign_no_self_test BORINGSSL_PREFIX %+ _rsa_sign_no_self_test
%xdefine rsa_verify_no_self_test BORINGSSL_PREFIX %+ _rsa_verify_no_self_test
%xdefine rsa_verify_raw_no_self_test BORINGSSL_PREFIX %+ _rsa_verify_raw_no_self_test
%xdefine rsaz_1024_gather5_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_gather5_avx2
%xdefine rsaz_1024_mul_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_mul_avx2
%xdefine rsaz_1024_norm2red_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_norm2red_avx2
%xdefine rsaz_1024_red2norm_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_red2norm_avx2
%xdefine rsaz_1024_scatter5_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_scatter5_avx2
%xdefine rsaz_1024_sqr_avx2 BORINGSSL_PREFIX %+ _rsaz_1024_sqr_avx2
%xdefine rsaz_amm52x20_x1_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x20_x1_ifma256
%xdefine rsaz_amm52x20_x2_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x20_x2_ifma256
%xdefine rsaz_amm52x30_x1_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x30_x1_ifma256
%xdefine rsaz_amm52x30_x2_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x30_x2_ifma256
%xdefine rsaz_amm52x40_x1_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x40_x1_ifma256
%xdefine rsaz_amm52x40_x2_ifma256 BORINGSSL_PREFIX %+ _rsaz_amm52x40_x2_ifma256
%xdefine s2i_ASN1_INTEGER BORINGSSL_PREFIX %+ _s2i_ASN1_INTEGER
%xdefine s2i_ASN1_OCTET_STRING BORINGSSL_PREFIX %+ _s2i_ASN1_OCTET_STRING
%xdefine set_fork_ube_generation_number_FOR_TESTING BORINGSSL_PREFIX %+ _set_fork_ube_generation_number_FOR_TESTING
%xdefine set_thread_and_global_tree_drbg_reseed_counter_FOR_TESTING BORINGSSL_PREFIX %+ _set_thread_and_global_tree_drbg_reseed_counter_FOR_TESTING
%xdefine set_vm_ube_generation_number_FOR_TESTING BORINGSSL_PREFIX %+ _set_vm_ube_generation_number_FOR_TESTING
%xdefine sha1_block_data_order BORINGSSL_PREFIX %+ _sha1_block_data_order
%xdefine sha1_block_data_order_avx BORINGSSL_PREFIX %+ _sha1_block_data_order_avx
%xdefine sha1_block_data_order_avx2 BORINGSSL_PREFIX %+ _sha1_block_data_order_avx2
%xdefine sha1_block_data_order_hw BORINGSSL_PREFIX %+ _sha1_block_data_order_hw
%xdefine sha1_block_data_order_neon BORINGSSL_PREFIX %+ _sha1_block_data_order_neon
%xdefine sha1_block_data_order_nohw BORINGSSL_PREFIX %+ _sha1_block_data_order_nohw
%xdefine sha1_block_data_order_ssse3 BORINGSSL_PREFIX %+ _sha1_block_data_order_ssse3
%xdefine sha1_func BORINGSSL_PREFIX %+ _sha1_func
%xdefine sha224_func BORINGSSL_PREFIX %+ _sha224_func
%xdefine sha256_block_data_order_avx BORINGSSL_PREFIX %+ _sha256_block_data_order_avx
%xdefine sha256_block_data_order_hw BORINGSSL_PREFIX %+ _sha256_block_data_order_hw
%xdefine sha256_block_data_order_neon BORINGSSL_PREFIX %+ _sha256_block_data_order_neon
%xdefine sha256_block_data_order_nohw BORINGSSL_PREFIX %+ _sha256_block_data_order_nohw
%xdefine sha256_block_data_order_ssse3 BORINGSSL_PREFIX %+ _sha256_block_data_order_ssse3
%xdefine sha256_func BORINGSSL_PREFIX %+ _sha256_func
%xdefine sha384_func BORINGSSL_PREFIX %+ _sha384_func
%xdefine sha3_keccak2_f1600 BORINGSSL_PREFIX %+ _sha3_keccak2_f1600
%xdefine sha3_keccak4_f1600_alt BORINGSSL_PREFIX %+ _sha3_keccak4_f1600_alt
%xdefine sha3_keccak4_f1600_alt2 BORINGSSL_PREFIX %+ _sha3_keccak4_f1600_alt2
%xdefine sha3_keccak_f1600 BORINGSSL_PREFIX %+ _sha3_keccak_f1600
%xdefine sha3_keccak_f1600_alt BORINGSSL_PREFIX %+ _sha3_keccak_f1600_alt
%xdefine sha512_block_data_order BORINGSSL_PREFIX %+ _sha512_block_data_order
%xdefine sha512_block_data_order_avx BORINGSSL_PREFIX %+ _sha512_block_data_order_avx
%xdefine sha512_block_data_order_hw BORINGSSL_PREFIX %+ _sha512_block_data_order_hw
%xdefine sha512_block_data_order_neon BORINGSSL_PREFIX %+ _sha512_block_data_order_neon
%xdefine sha512_block_data_order_nohw BORINGSSL_PREFIX %+ _sha512_block_data_order_nohw
%xdefine sha512_func BORINGSSL_PREFIX %+ _sha512_func
%xdefine sk_pop_free BORINGSSL_PREFIX %+ _sk_pop_free
%xdefine sskdf_variant_digest BORINGSSL_PREFIX %+ _sskdf_variant_digest
%xdefine sskdf_variant_hmac BORINGSSL_PREFIX %+ _sskdf_variant_hmac
%xdefine tree_jitter_drbg_destructor BORINGSSL_PREFIX %+ _tree_jitter_drbg_destructor
%xdefine tree_jitter_free_thread_drbg BORINGSSL_PREFIX %+ _tree_jitter_free_thread_drbg
%xdefine tree_jitter_get_seed BORINGSSL_PREFIX %+ _tree_jitter_get_seed
%xdefine tree_jitter_initialize BORINGSSL_PREFIX %+ _tree_jitter_initialize
%xdefine tree_jitter_zeroize_thread_drbg BORINGSSL_PREFIX %+ _tree_jitter_zeroize_thread_drbg
%xdefine tty_in BORINGSSL_PREFIX %+ _tty_in
%xdefine tty_new BORINGSSL_PREFIX %+ _tty_new
%xdefine tty_orig BORINGSSL_PREFIX %+ _tty_orig
%xdefine tty_out BORINGSSL_PREFIX %+ _tty_out
%xdefine used_for_hmac BORINGSSL_PREFIX %+ _used_for_hmac
%xdefine v2i_GENERAL_NAME BORINGSSL_PREFIX %+ _v2i_GENERAL_NAME
%xdefine v2i_GENERAL_NAMES BORINGSSL_PREFIX %+ _v2i_GENERAL_NAMES
%xdefine v2i_GENERAL_NAME_ex BORINGSSL_PREFIX %+ _v2i_GENERAL_NAME_ex
%xdefine v3_akey_id BORINGSSL_PREFIX %+ _v3_akey_id
%xdefine v3_alt BORINGSSL_PREFIX %+ _v3_alt
%xdefine v3_bcons BORINGSSL_PREFIX %+ _v3_bcons
%xdefine v3_cpols BORINGSSL_PREFIX %+ _v3_cpols
%xdefine v3_crl_invdate BORINGSSL_PREFIX %+ _v3_crl_invdate
%xdefine v3_crl_num BORINGSSL_PREFIX %+ _v3_crl_num
%xdefine v3_crl_reason BORINGSSL_PREFIX %+ _v3_crl_reason
%xdefine v3_crld BORINGSSL_PREFIX %+ _v3_crld
%xdefine v3_delta_crl BORINGSSL_PREFIX %+ _v3_delta_crl
%xdefine v3_ext_ku BORINGSSL_PREFIX %+ _v3_ext_ku
%xdefine v3_freshest_crl BORINGSSL_PREFIX %+ _v3_freshest_crl
%xdefine v3_idp BORINGSSL_PREFIX %+ _v3_idp
%xdefine v3_info BORINGSSL_PREFIX %+ _v3_info
%xdefine v3_inhibit_anyp BORINGSSL_PREFIX %+ _v3_inhibit_anyp
%xdefine v3_key_usage BORINGSSL_PREFIX %+ _v3_key_usage
%xdefine v3_name_constraints BORINGSSL_PREFIX %+ _v3_name_constraints
%xdefine v3_ns_ia5_list BORINGSSL_PREFIX %+ _v3_ns_ia5_list
%xdefine v3_nscert BORINGSSL_PREFIX %+ _v3_nscert
%xdefine v3_ocsp_accresp BORINGSSL_PREFIX %+ _v3_ocsp_accresp
%xdefine v3_ocsp_nocheck BORINGSSL_PREFIX %+ _v3_ocsp_nocheck
%xdefine v3_ocsp_nonce BORINGSSL_PREFIX %+ _v3_ocsp_nonce
%xdefine v3_policy_constraints BORINGSSL_PREFIX %+ _v3_policy_constraints
%xdefine v3_policy_mappings BORINGSSL_PREFIX %+ _v3_policy_mappings
%xdefine v3_sinfo BORINGSSL_PREFIX %+ _v3_sinfo
%xdefine v3_skey_id BORINGSSL_PREFIX %+ _v3_skey_id
%xdefine validate_cidr_mask BORINGSSL_PREFIX %+ _validate_cidr_mask
%xdefine vm_ube_fallback_get_seed BORINGSSL_PREFIX %+ _vm_ube_fallback_get_seed
%xdefine voprf_exp2_blind BORINGSSL_PREFIX %+ _voprf_exp2_blind
%xdefine voprf_exp2_client_key_from_bytes BORINGSSL_PREFIX %+ _voprf_exp2_client_key_from_bytes
%xdefine voprf_exp2_derive_key_from_secret BORINGSSL_PREFIX %+ _voprf_exp2_derive_key_from_secret
%xdefine voprf_exp2_generate_key BORINGSSL_PREFIX %+ _voprf_exp2_generate_key
%xdefine voprf_exp2_issuer_key_from_bytes BORINGSSL_PREFIX %+ _voprf_exp2_issuer_key_from_bytes
%xdefine voprf_exp2_read BORINGSSL_PREFIX %+ _voprf_exp2_read
%xdefine voprf_exp2_sign BORINGSSL_PREFIX %+ _voprf_exp2_sign
%xdefine voprf_exp2_unblind BORINGSSL_PREFIX %+ _voprf_exp2_unblind
%xdefine voprf_pst1_blind BORINGSSL_PREFIX %+ _voprf_pst1_blind
%xdefine voprf_pst1_client_key_from_bytes BORINGSSL_PREFIX %+ _voprf_pst1_client_key_from_bytes
%xdefine voprf_pst1_derive_key_from_secret BORINGSSL_PREFIX %+ _voprf_pst1_derive_key_from_secret
%xdefine voprf_pst1_generate_key BORINGSSL_PREFIX %+ _voprf_pst1_generate_key
%xdefine voprf_pst1_issuer_key_from_bytes BORINGSSL_PREFIX %+ _voprf_pst1_issuer_key_from_bytes
%xdefine voprf_pst1_read BORINGSSL_PREFIX %+ _voprf_pst1_read
%xdefine voprf_pst1_sign BORINGSSL_PREFIX %+ _voprf_pst1_sign
%xdefine voprf_pst1_sign_with_proof_scalar_for_testing BORINGSSL_PREFIX %+ _voprf_pst1_sign_with_proof_scalar_for_testing
%xdefine voprf_pst1_unblind BORINGSSL_PREFIX %+ _voprf_pst1_unblind
%xdefine vpaes_cbc_encrypt BORINGSSL_PREFIX %+ _vpaes_cbc_encrypt
%xdefine vpaes_ctr32_encrypt_blocks BORINGSSL_PREFIX %+ _vpaes_ctr32_encrypt_blocks
%xdefine vpaes_decrypt BORINGSSL_PREFIX %+ _vpaes_decrypt
%xdefine vpaes_decrypt_key_to_bsaes BORINGSSL_PREFIX %+ _vpaes_decrypt_key_to_bsaes
%xdefine vpaes_encrypt BORINGSSL_PREFIX %+ _vpaes_encrypt
%xdefine vpaes_encrypt_key_to_bsaes BORINGSSL_PREFIX %+ _vpaes_encrypt_key_to_bsaes
%xdefine vpaes_set_decrypt_key BORINGSSL_PREFIX %+ _vpaes_set_decrypt_key
%xdefine vpaes_set_encrypt_key BORINGSSL_PREFIX %+ _vpaes_set_encrypt_key
%xdefine x25519_asn1_meth BORINGSSL_PREFIX %+ _x25519_asn1_meth
%xdefine x25519_ge_add BORINGSSL_PREFIX %+ _x25519_ge_add
%xdefine x25519_ge_frombytes_vartime BORINGSSL_PREFIX %+ _x25519_ge_frombytes_vartime
%xdefine x25519_ge_p1p1_to_p2 BORINGSSL_PREFIX %+ _x25519_ge_p1p1_to_p2
%xdefine x25519_ge_p1p1_to_p3 BORINGSSL_PREFIX %+ _x25519_ge_p1p1_to_p3
%xdefine x25519_ge_p3_to_cached BORINGSSL_PREFIX %+ _x25519_ge_p3_to_cached
%xdefine x25519_ge_scalarmult BORINGSSL_PREFIX %+ _x25519_ge_scalarmult
%xdefine x25519_ge_scalarmult_base BORINGSSL_PREFIX %+ _x25519_ge_scalarmult_base
%xdefine x25519_ge_scalarmult_small_precomp BORINGSSL_PREFIX %+ _x25519_ge_scalarmult_small_precomp
%xdefine x25519_ge_sub BORINGSSL_PREFIX %+ _x25519_ge_sub
%xdefine x25519_ge_tobytes BORINGSSL_PREFIX %+ _x25519_ge_tobytes
%xdefine x25519_pkey_meth BORINGSSL_PREFIX %+ _x25519_pkey_meth
%xdefine x25519_public_from_private_nohw BORINGSSL_PREFIX %+ _x25519_public_from_private_nohw
%xdefine x25519_public_from_private_s2n_bignum BORINGSSL_PREFIX %+ _x25519_public_from_private_s2n_bignum
%xdefine x25519_sc_reduce BORINGSSL_PREFIX %+ _x25519_sc_reduce
%xdefine x25519_scalar_mult_generic_nohw BORINGSSL_PREFIX %+ _x25519_scalar_mult_generic_nohw
%xdefine x25519_scalar_mult_generic_s2n_bignum BORINGSSL_PREFIX %+ _x25519_scalar_mult_generic_s2n_bignum
%xdefine x509V3_add_value_asn1_string BORINGSSL_PREFIX %+ _x509V3_add_value_asn1_string
%xdefine x509_check_cert_time BORINGSSL_PREFIX %+ _x509_check_cert_time
%xdefine x509_check_issued_with_callback BORINGSSL_PREFIX %+ _x509_check_issued_with_callback
%xdefine x509_digest_sign_algorithm BORINGSSL_PREFIX %+ _x509_digest_sign_algorithm
%xdefine x509_digest_verify_init BORINGSSL_PREFIX %+ _x509_digest_verify_init
%xdefine x509_init_signature_info BORINGSSL_PREFIX %+ _x509_init_signature_info
%xdefine x509_print_rsa_pss_params BORINGSSL_PREFIX %+ _x509_print_rsa_pss_params
%xdefine x509_rsa_ctx_to_pss BORINGSSL_PREFIX %+ _x509_rsa_ctx_to_pss
%xdefine x509_rsa_pss_to_ctx BORINGSSL_PREFIX %+ _x509_rsa_pss_to_ctx
%xdefine x509v3_a2i_ipadd BORINGSSL_PREFIX %+ _x509v3_a2i_ipadd
%xdefine x509v3_bytes_to_hex BORINGSSL_PREFIX %+ _x509v3_bytes_to_hex
%xdefine x509v3_cache_extensions BORINGSSL_PREFIX %+ _x509v3_cache_extensions
%xdefine x509v3_conf_name_matches BORINGSSL_PREFIX %+ _x509v3_conf_name_matches
%xdefine x509v3_ext_free_with_method BORINGSSL_PREFIX %+ _x509v3_ext_free_with_method
%xdefine x509v3_hex_to_bytes BORINGSSL_PREFIX %+ _x509v3_hex_to_bytes
%xdefine x86_64_assembly_implementation_FOR_TESTING BORINGSSL_PREFIX %+ _x86_64_assembly_implementation_FOR_TESTING

%endif ; __OUTPUT_FORMAT__
%endif ; BORINGSSL_PREFIX_SYMBOLS_NASM_INC
