# Changelog

## Unreleased

## 0.20.3 - 2026-10-01

- Release the byte-owner and area implementation pinned by the TribleSpace
  cohort at `066c32a7cfcc470a6750c849499c1e88a0e0c50a`. This is a new registry
  version, not a claim that the previously published 0.20.2 had those bytes.
- No implementation change accompanies this release metadata update.

## Earlier development

- make `Section::freeze` a memory-protection transition rather than an
  implicit durability barrier; `ByteArea::persist` synchronizes the complete
  backing file once when it is deliberately retained
- **fix soundness**: changed `Bytes` and `View` internal data fields from
  `&'static [u8]`/`&'static T` to raw pointers (`*const [u8]`/`*const T`)
  to fix undefined behavior under both Stacked Borrows and Tree Borrows when
  `Bytes` or `View` is passed by value to a function (including
  `std::mem::drop`) while holding the last strong reference to the owner
- removed `erase_lifetime` helper, now unnecessary with raw pointer storage
- added Miri test suite (`tests/miri.rs`) with 75 tests covering all unsafe
  code paths: lifetime erasure, weak reference upgrades, `try_unwrap_owner`
  data pointer reconstruction, view operations, complex drop orderings,
  thread safety (`Send`/`Sync` validation), all `ByteSource` implementations
  (`VecDeque`, `Cow`, `Box<[u8]>`, `&'static str`), `from_raw_parts` /
  `View::from_raw_parts` direct tests, edge cases (zero-length, single byte,
  exhaustive drain), winnow `Stream` operations, and stress patterns (many
  clones, deeply nested slicing, overlapping slices)
- added `scripts/miri.sh` for running Miri tests with Tree Borrows
- added optional `burn` feature with `ByteSource` support for `burn_tensor::Bytes`
- added zero-copy conversion from `anybytes::Bytes` to `burn_tensor::Bytes`
- added burn-feature tests covering both conversion directions and sliced views
- added proptest coverage for prefix/suffix invariants and introduced a fuzz target for mutating `Bytes` operations
- extended `verify.sh` to install cargo-fuzz and execute deterministic fuzz runs alongside Kani proofs
- added Kani proofs ensuring `is_subslice` accepts only slices from the original allocation
- add Kani proof checking `Bytes::downcast_to_owner` for matching and mismatched owners
- added Kani verification harnesses for `Bytes::pop_front` and `Bytes::pop_back`
- avoid flushing empty memory maps in `Section::freeze` to prevent macOS errors
- derived zerocopy traits for `SectionHandle` to allow storing handles in `ByteArea` sections
- added example demonstrating `ByteArea` with multiple typed sections, concurrent mutations, and freezing or persisting the area
- added example combining Python bindings with winnow parsing
- added Python example demonstrating structured parsing with winnow's `view`
- added `SectionHandle` for reconstructing sections from a frozen `ByteArea`
- added `ByteSource` support for `VecDeque<T>` when `zerocopy` is enabled and kept the deque as owner
- added `ByteSource` support for `Cow<'static, T>` where `T: AsRef<[u8]>`
- added `ByteArea` for staged file writes with `Section::freeze()` to return `Bytes`
- `SectionWriter::reserve` now accepts a zerocopy type instead of an alignment constant
- `ByteArea` reuses previous pages so allocations align only to the element type
- `Section::freeze` converts the writable mapping to read-only instead of remapping
- simplified `ByteArea` by introducing `SectionWriter` for mutable access without
  interior mutability
- tie `Section` lifetime to `ByteArea` to prevent dangling sections
- allow multiple `ByteArea` sections at once with non-overlapping byte ranges
- documented all fields in `ByteArea`, `SectionWriter` and `Section`
- documented ByteArea usage under advanced usage with proper heading
- added `ByteArea::persist` to rename the temporary file
- removed the old `ByteBuffer` type in favor of `ByteArea`
- added tests covering `ByteArea` sections, typed reserves and persistence
- added test verifying alignment padding between differently aligned writes
- added property tests generating random `ByteArea` sections and documented
  multi-typed section layouts
- split Kani verification into `verify.sh` and streamline `preflight.sh`
- clarify that `verify.sh` runs on a dedicated system and document avoiding async code
- install `rustfmt` and the Kani verifier automatically via `cargo install`
- restore Kani proof best practices in `AGENTS.md` and note that proofs run via `verify.sh`
- limit Kani loop unwind by default and set per-harness bounds
- `ByteBuffer::push` now accepts any `IntoBytes + Immutable` value when the
  `zerocopy` feature is enabled
- increase unwind for prefix/suffix overflow proofs
- move weak reference and downcasting examples into module docs
- expand module introduction describing use cases
- document rationale for separating `ByteSource` and `ByteOwner`
- added optional `winnow` feature for parser integration
- added `INVENTORY.md` for tracking future work and noted it in `AGENTS.md`
- documented safety rationale for `winnow` integration
- implemented `Stream` directly for `Bytes` with a safe `iter_offsets` iterator
- added `pop_back` and `pop_front` helpers and rewrote parser examples
- added tests covering `pop_front` and `pop_back`
- added tests covering `take_prefix` and `take_suffix`
- removed the Completed Work section from `INVENTORY.md` and documented its use
- added `Bytes::try_unwrap_owner` to reclaim the owner when uniquely held
- simplified `Bytes::try_unwrap_owner` implementation
- added `ByteBuffer` for owning aligned byte allocations
- compile-time assertion that `ALIGN` is a power of two
- added `reserve_total` to `ByteBuffer` for reserving absolute capacity
- fixed potential UB in `Bytes::try_unwrap_owner` for custom owners
- renamed `ByteArea::writer` to `sections` for clarity
- prevent dangling `data` by dropping references before unwrapping the owner
- refined `Bytes::try_unwrap_owner` to cast the data slice to a pointer only
  when the owner type matches
- replaced `ByteOwner::as_any` with trait upcasting for simpler downcasting
- rewrote `winnow::view` to use safe helpers and added `view_elems(count)` parser
- `winnow::view_elems` now returns a Parser closure for idiomatic usage
- replaced `ByteOwner::as_any` with trait upcasting to `Any`
- `Bytes::downcast_to_owner` and `View::downcast_to_owner` now return `Result`
  and return the original value on failure
  in a dedicated AGENTS section
- add tests for weak reference upgrade/downgrade and Kani proofs for view helpers
- add Kani proofs covering `Bytes::try_unwrap_owner` and `WeakBytes` upgrade semantics
- add examples for quick start and PyAnyBytes usage
- add example showing how to wrap Python `bytes` into `Bytes`
- summarize built-in `ByteSource`s and show how to extend them
- added tests verifying `WeakView` upgrade and drop semantics
- clarify library overview and development instructions in README
- added crate-level examples for weak references and owner downcasting
- expanded module introduction describing use cases
- update bytes, ownedbytes, memmap2, zerocopy and pyo3 dependencies
- documented rationale for separating `ByteSource` and `ByteOwner`
- verify `cargo fmt` availability and install `rustfmt` via rustup if missing
- note that the `pyo3` feature requires Python development libraries
- documented safety requirements for `erase_lifetime`
- warn about missing documentation by enabling the `missing_docs` lint
- derive `Clone` and `Debug` for `WeakBytes` and `WeakView`
- replaced `quickcheck` property tests with `proptest`
- added `ByteSource` support for `memmap2::MmapMut` and `Cow<'static, [T]>` with `zerocopy`
- split `Cow` ByteSource tests into dedicated cases
- skip Python examples when the `pyo3` feature is disabled to fix `cargo test`
- added `Bytes::map_file` helper for convenient file mapping
  (accepts any `memmap2::MmapAsRawDesc`, e.g. `&File` or `&NamedTempFile`)
- added `Bytes::map_file_region` to map a specific region of a file
- reverted automatic installation of Python development packages in the
  preflight script; rely on the system `python3-dev` package
- set the preflight script to use Python 3.12 for building pyo3 code
- added README example demonstrating `Bytes::try_unwrap_owner`
- expanded `ByteOwner` trait docs to clarify lifetime requirements and trait
  upcasting for downcasting
- removed rope-like store integration and async wrappers from the inventory
- noted new suggestions in `INVENTORY.md` for future work
- clarified that implementing `ByteSource` for `Arc` types would double wrap the
  owner and updated `INVENTORY.md` accordingly
- removed the `serde` support idea from the inventory
- removed the unsafe derive macro idea from the inventory
- removed the `Iterator` support idea from the inventory as `Bytes` already
  dereferences to `[u8]`
- documented creating `Bytes` from `Arc` sources without an extra wrapper and
  removed the corresponding task from the inventory
- implemented `bytes::Buf` for `Bytes` and `From<Bytes>` for `bytes::Bytes` for
  seamless integration with Tokio and other libraries
- implemented `ExactSizeIterator` and `FusedIterator` for `BytesIterOffsets`
- added test exposing `PyAnyBytes` as a read-only `memoryview`
- renamed `PyBytes` wrapper to `PyAnyBytes` to avoid confusion
- renamed `py_anybytes` module to `pyanybytes` for consistency

## 0.19.3 - 2025-05-30
- implemented `Error` for `ViewError`

## 0.19.2 - 2025-01-24
- removed `Sized` constraint from view methods

## 0.19.1 - 2025-01-24
- removed `Sized` bound on `.bytes()`

## 0.19.0 - 2025-01-24
- reworked `take_*` helpers and updated conversions
