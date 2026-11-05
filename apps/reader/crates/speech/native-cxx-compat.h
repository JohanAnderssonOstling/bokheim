// The SDK's GNU C++ headers were configured against glibc headers that expose
// obsolete global isinf/isnan overloads. Zig's glibc 2.17 headers omit those
// overloads; use the standard C++ implementations instead.
#include <bits/c++config.h>
#undef _GLIBCXX_HAVE_OBSOLETE_ISINF
#define _GLIBCXX_HAVE_OBSOLETE_ISINF 0
#undef _GLIBCXX_HAVE_OBSOLETE_ISNAN
#define _GLIBCXX_HAVE_OBSOLETE_ISNAN 0
