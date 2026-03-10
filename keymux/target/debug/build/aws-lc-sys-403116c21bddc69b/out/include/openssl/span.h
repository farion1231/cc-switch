/* Copyright (c) 2017, Google Inc.
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

#ifndef OPENSSL_HEADER_SSL_SPAN_H
#define OPENSSL_HEADER_SSL_SPAN_H

#include <openssl/base.h>

#if !defined(BORINGSSL_NO_CXX)

extern "C++" {

#include <stdlib.h>

#include <algorithm>
#include <type_traits>

BSSL_NAMESPACE_BEGIN

template <typename T>
class Span;

namespace internal {
template <typename T>
class SpanBase {
  // Put comparison operator implementations into a base class with const T, so
  // they can be used with any type that implicitly converts into a Span.
  static_assert(std::is_const<T>::value,
                "Span<T> must be derived from SpanBase<const T>");

  friend bool operator==(Span<T> lhs, Span<T> rhs) {
    // MSVC issues warning C4996 because std::equal is unsafe. The pragma to
    // suppress the warning mysteriously has no effect, hence this
    // implementation. See
    // https://msdn.microsoft.com/en-us/library/aa985974.aspx.
    if (lhs.size() != rhs.size()) {
      return false;
    }
    for (T *l = lhs.begin(), *r = rhs.begin(); l != lhs.end() && r != rhs.end();
         ++l, ++r) {
      if (*l != *r) {
        return false;
      }
    }
    return true;
  }

  friend bool operator!=(Span<T> lhs, Span<T> rhs) { return !(lhs == rhs); }
};
}  // namespace internal

// A Span<T> is a non-owning reference to a contiguous array of objects of type
// |T|. Conceptually, a Span is a simple a pointer to |T| and a count of
// elements accessible via that pointer. The elements referenced by the Span can
// be mutated if |T| is mutable.
//
// A Span can be constructed from container types implementing |data()| and
// |size()| methods. If |T| is constant, construction from a container type is
// implicit. This allows writing methods that accept data from some unspecified
// container type:
//
// // Foo views data referenced by v.
// void Foo(bssl::Span<const uint8_t> v) { ... }
//
// std::vector<uint8_t> vec;
// Foo(vec);
//
// For mutable Spans, conversion is explicit:
//
// // FooMutate mutates data referenced by v.
// void FooMutate(bssl::Span<uint8_t> v) { ... }
//
// FooMutate(bssl::Span<uint8_t>(vec));
//
// You can also use the |MakeSpan| and |MakeConstSpan| factory methods to
// construct Spans in order to deduce the type of the Span automatically.
//
// FooMutate(bssl::MakeSpan(vec));
//
// Note that Spans have value type sematics. They are cheap to construct and
// copy, and should be passed by value whenever a method would otherwise accept
// a reference or pointer to a container or array.
template <typename T>
class Span : private internal::SpanBase<const T> {
 private:
  static const size_t npos = static_cast<size_t>(-1);

 public:
  constexpr Span() : Span(nullptr, 0) {}
  constexpr Span(T *ptr, size_t len) : data_(ptr), size_(len) {}

  template <size_t N>
  constexpr Span(T (&array)[N]) : Span(array, N) {}

  template <
      typename C,
      // TODO(davidben): Switch everything to std::enable_if_t when we remove
      // support for MSVC 2015. Although we could write our own enable_if_t and
      // MSVC 2015 has std::enable_if_t anyway, MSVC 2015's SFINAE
      // implementation is problematic and does not work below unless we write
      // the ::type at use.
      //
      // TODO(davidben): Move this and the identical copy below into an
      // EnableIfContainer alias when we drop MSVC 2015 support. MSVC 2015's
      // SFINAE support cannot handle type aliases.
      typename = typename std::enable_if<
          std::is_convertible<decltype(std::declval<C>().data()), T *>::value &&
          std::is_integral<decltype(std::declval<C>().size())>::value>::type,
      typename = typename std::enable_if<std::is_const<T>::value, C>::type>
  Span(const C &container) : data_(container.data()), size_(container.size()) {}

  template <
      typename C,
      typename = typename std::enable_if<
          std::is_convertible<decltype(std::declval<C>().data()), T *>::value &&
          std::is_integral<decltype(std::declval<C>().size())>::value>::type,
      typename = typename std::enable_if<!std::is_const<T>::value, C>::type>
  explicit Span(C &container)
      : data_(container.data()), size_(container.size()) {}

  T *data() const { return data_; }
  size_t size() const { return size_; }
  bool empty() const { return size_ == 0; }

  T *begin() const { return data_; }
  const T *cbegin() const { return data_; }
  T *end() const { return data_ + size_; }
  const T *cend() const { return end(); }

  T &front() const {
    if (size_ == 0) {
      abort();
    }
    return data_[0];
  }
  T &back() const {
    if (size_ == 0) {
      abort();
    }
    return data_[size_ - 1];
  }

  T &operator[](size_t i) const {
    if (i >= size_) {
      abort();
    }
    return data_[i];
  }
  T &at(size_t i) const { return (*this)[i]; }

  Span subspan(size_t pos = 0, size_t len = npos) const {
    if (pos > size_) {
      // absl::Span throws an exception here. Note std::span and Chromium
      // base::span additionally forbid pos + len being out of range, with a
      // special case at npos/dynamic_extent, while absl::Span::subspan clips
      // the span. For now, we align with absl::Span in case we switch to it in
      // the future.
      abort();
    }
    return Span(data_ + pos, std::min(size_ - pos, len));
  }

  Span first(size_t len) {
    if (len > size_) {
      abort();
    }
    return Span(data_, len);
  }

  Span last(size_t len) {
    if (len > size_) {
      abort();
    }
    return Span(data_ + size_ - len, len);
  }

 private:
  T *data_;
  size_t size_;
};

template <typename T>
const size_t Span<T>::npos;

template <typename T>
Span<T> MakeSpan(T *ptr, size_t size) {
  return Span<T>(ptr, size);
}

template <typename C>
auto MakeSpan(C &c) -> decltype(MakeSpan(c.data(), c.size())) {
  return MakeSpan(c.data(), c.size());
}

template <typename T>
Span<const T> MakeConstSpan(T *ptr, size_t size) {
  return Span<const T>(ptr, size);
}

template <typename C>
auto MakeConstSpan(const C &c) -> decltype(MakeConstSpan(c.data(), c.size())) {
  return MakeConstSpan(c.data(), c.size());
}

template <typename T, size_t size>
Span<const T> MakeConstSpan(T (&array)[size]) {
  return Span<const T>(array, size);
}

BSSL_NAMESPACE_END

}  // extern C++

#endif  // !defined(BORINGSSL_NO_CXX)

#endif  // OPENSSL_HEADER_SSL_SPAN_H
