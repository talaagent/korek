//! `validate()` does not allocate when the value is valid (after the first
//! call, which compiles the regexes).

#![allow(unsafe_code, clippy::unwrap_used)]

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use std::alloc::System;

use korek_fixture::example::{Contact, McpServer, SearchRequest};
use korek_runtime::Validate as _;
use serde_json::json;

struct Counting;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards to the system allocator; only counts.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from `alloc` above, with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn allocations(f: impl Fn()) -> usize {
    f();
    let before = ALLOCS.load(Ordering::Relaxed);
    f();
    ALLOCS.load(Ordering::Relaxed) - before
}

#[test]
fn valid_values_validate_without_allocating() {
    let contact: Contact = serde_json::from_value(json!({
        "id": "7f1b0c1e-8d4a-4a53-9a0e-3c2b1d4e5f60",
        "name": "Ada",
        "emails": ["ada@example.com", "ada@work.example"],
        "score": 0.5,
        "tags": {"team": ["core"]},
        "created": "2020-01-01T00:00:00Z",
        "phone": "+14155550100"
    }))
    .unwrap();
    let server: McpServer = serde_json::from_value(json!({
        "type": "stdio", "command": "mcp", "env": {"HOME": "/root"}
    }))
    .unwrap();
    let search = SearchRequest {
        query: "a".into(),
        limit: 10,
    };
    assert_eq!(allocations(|| contact.validate().unwrap()), 0);
    assert_eq!(allocations(|| server.validate().unwrap()), 0);
    assert_eq!(allocations(|| search.validate().unwrap()), 0);
}
