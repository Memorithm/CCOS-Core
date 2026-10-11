# SLHAv2 memory accounting contract

The adapter keeps every page in a fixed-size `AlignedMemoryPage` slot. A
HOT→WARM transition zeroes the 32-byte residual but does not resize the page,
remove the `Vec` slot, return memory to the allocator, or establish an RSS
decrease.

`CompressionResult` therefore reports two independent quantities:

- `logical_bytes_before/after`: meaningful codec content, 128→96 bytes on the first compression; an approximate restore keeps the residual absent, so a later compression reports 96→96 rather than fabricating another reduction;
- `resident_bytes_before/after`: `size_of::<AlignedMemoryPage>()`, unchanged.

`resident_storage_released` is false. `restore_fidelity` is
`Approximate`: the removed residual cannot be recovered from the resident page.
The reserved exact mode is not exposed as a qualified capability.

The zero-payload golden vector is stable and executable in
`tests/test_slha_adapter.rs`: all 128 decoded values are `0.0`, logical
content becomes 96 bytes, the resident slot is unchanged, and restore fidelity
is approximate.

No bit-exact parity with a proprietary or external SLHAv2 kernel is claimed.
Such a claim requires separately supplied owner-approved vectors and an
exact-kernel qualification. Process RSS is intentionally not used as proof for
a 32-byte per-page transition; the implementation makes no resident-memory
saving claim.
