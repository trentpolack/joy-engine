// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// libc_alloc uses memalign on non-Apple targets. Adapt it to WASI's posix_memalign so Rust and Lua share one heap.
// libc_alloc already normalizes alignment to at least sizeof(usize), as posix_memalign requires.
#include <stdlib.h>

void* memalign(size_t alignment, size_t bytes)
{
    void* memory = NULL;
    if (posix_memalign(&memory, alignment, bytes) != 0) {
        return NULL;
    }
    return memory;
}
