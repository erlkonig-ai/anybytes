/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 * Copyright (c) Jan-Paul Bultmann
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Temporary byte area backed by a file.
//!
//! The area offers staged writing through a [`SectionWriter`]. Each call to
//! [`SectionWriter::reserve`] returns a mutable [`Section`] tied to the area's
//! lifetime. Multiple sections may coexist; their byte ranges do not overlap.
//! Freezing a section via [`Section::freeze`] changes its range to immutable
//! and returns [`Bytes`].
//!
//! [`SectionWriter::reserve`] grows the file sparsely; use
//! [`SectionWriter::reserve_allocated`] when a full filesystem must be an
//! [`io::Error`] from the reservation rather than a fault on a later write.
//! [`Section::anonymous_allocated`] creates one owned section, with the same
//! allocation guarantee, that borrows no area.
//!
//! # Element types
//!
//! A section starts zero-filled and does not track which slots have been
//! written, so its capabilities depend on what the element type `T` allows:
//!
//! - Any `T: TryFromBytes + Immutable` can be reserved and frozen. The
//!   section is aligned for `T`.
//! - With `T: IntoBytes`, valid values are copied into slots by
//!   [`Section::write`] and [`Section::write_slice`], and
//!   [`Section::as_mut_bytes`] exposes the slots as raw bytes.
//! - Only with `T: FromBytes`, where every bit pattern (the zero fill
//!   included) is a valid `T`, does the section hand out `&[T]` and
//!   `&mut [T]`: [`Section::as_mut_slice`], `Deref`, `DerefMut`, `AsRef` and
//!   `AsMut`.
//!
//! A type with invalid bit patterns, such as one holding a
//! [`NonZero`](core::num::NonZero), is therefore read back only after
//! freezing, through [`Bytes::view`] or [`SectionHandle::view`], which
//! validate every slot. A slot that was never written still holds zeros:
//! where zero is not a valid `T` the view fails rather than producing an
//! invalid value, and where it is (a `bool`, an enum with a zero
//! discriminant) the slot reads back as that value, as it would for a
//! `FromBytes` type.
//!
//! # Examples
//!
//! ```
//! # #[cfg(all(feature = "mmap", feature = "zerocopy"))]
//! # {
//! use anybytes::area::ByteArea;
//!
//! let mut area = ByteArea::new().unwrap();
//! let mut sections = area.sections();
//!
//! let mut a = sections.reserve::<u8>(1).unwrap();
//! a.as_mut_slice()[0] = 1;
//! let handle_a = a.handle();
//!
//! let mut b = sections.reserve::<u32>(1).unwrap();
//! b.as_mut_slice()[0] = 2;
//! let handle_b = b.handle();
//!
//! let bytes_a = a.freeze().unwrap();
//! let bytes_b = b.freeze().unwrap();
//! drop(sections);
//! let all = area.freeze().unwrap();
//!
//! assert_eq!(handle_a.bytes(&all).as_ref(), &[1]);
//! assert_eq!(handle_b.bytes(&all).as_ref(), &2u32.to_ne_bytes());
//!
//! let mut expected = Vec::new();
//! expected.extend_from_slice(&[1]);
//! expected.extend_from_slice(&[0; 3]);
//! expected.extend_from_slice(&2u32.to_ne_bytes());
//! assert_eq!(all.as_ref(), expected.as_slice());
//! # }
//! ```

use std::io::{self, Seek, SeekFrom, Write};
use std::marker::PhantomData;

use memmap2;
use page_size;
use tempfile::NamedTempFile;

use crate::Bytes;

#[cfg(feature = "zerocopy")]
use zerocopy::{FromBytes, Immutable, IntoBytes, TryFromBytes};

#[cfg(feature = "zerocopy")]
use crate::view::{View, ViewError};

/// The byte length of `elems` values of `T`, or an error when it does not fit
/// in `usize`: a wrapped length would map less than the section's slice spans.
///
/// Also refuses a `T` aligned more strictly than a page: a mapping starts on
/// a page boundary and nothing stronger, so such a slice could be misaligned.
fn byte_len<T>(elems: usize) -> io::Result<usize> {
    if core::mem::align_of::<T>() > page_size::get() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "section element alignment exceeds the page size",
        ));
    }
    core::mem::size_of::<T>()
        .checked_mul(elems)
        .ok_or_else(too_large)
}

fn too_large() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "section size overflows usize")
}

/// Area managing a temporary file.
#[derive(Debug)]
pub struct ByteArea {
    /// Temporary file backing the area.
    file: NamedTempFile,
    /// Current length of initialized data in bytes.
    len: usize,
}

impl ByteArea {
    /// Create a new empty area.
    pub fn new() -> io::Result<Self> {
        let file = NamedTempFile::new()?;
        Ok(Self { file, len: 0 })
    }

    /// Obtain a handle for reserving sections.
    pub fn sections(&mut self) -> SectionWriter<'_> {
        SectionWriter { area: self }
    }

    /// Freeze the area and return immutable bytes for the entire file.
    pub fn freeze(self) -> io::Result<Bytes> {
        let file = self.file.into_file();
        let mmap = unsafe { memmap2::MmapOptions::new().map(&file)? };
        Ok(Bytes::from_source(mmap))
    }

    /// Persist the temporary area file to `path` and return a read-only
    /// [`std::fs::File`] for it.
    ///
    /// The handle is read-only because frozen [`Bytes`] from this area map
    /// the same file: writing through a handle would change memory under
    /// views that were already validated, and for a type with invalid bit
    /// patterns that yields an invalid value in safe code. Other handles to
    /// the path, opened elsewhere, stay outside this guarantee, as they do
    /// for [`Bytes::map_file`].
    ///
    /// All mapped writes are synchronized once before the temporary file is
    /// retained. The rename itself and its parent directory are not separately
    /// synchronized, so this does not promise crash-atomic path publication.
    /// On Unix the reopened handle is checked to be the persisted file (same
    /// device and inode), so a path replaced in between is an error rather
    /// than a handle to another file.
    ///
    /// The persistence contract is supported on Unix targets. Other targets
    /// are best-effort rather than part of AnyBytes' support surface.
    pub fn persist<P: AsRef<std::path::Path>>(self, path: P) -> io::Result<std::fs::File> {
        let path = path.as_ref();
        self.file.as_file().sync_all()?;
        let written = self.file.persist(path).map_err(io::Error::from)?;
        let read_only = std::fs::File::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let (persisted, reopened) = (written.metadata()?, read_only.metadata()?);
            if (persisted.dev(), persisted.ino()) != (reopened.dev(), reopened.ino()) {
                return Err(io::Error::other(
                    "persisted area file was replaced before it could be reopened read-only",
                ));
            }
        }
        Ok(read_only)
    }
}

/// RAII guard giving temporary exclusive write access.
#[derive(Debug)]
pub struct SectionWriter<'area> {
    area: &'area mut ByteArea,
}

impl<'area> SectionWriter<'area> {
    /// Reserve a new section inside the area.
    ///
    /// The file grows by `set_len`, so the new range may be sparse: storage
    /// is only claimed when the mapping is first written. If the filesystem
    /// is full at that point, the write faults (SIGBUS on Unix) instead of
    /// returning an error. Use [`SectionWriter::reserve_allocated`] where
    /// that must surface as an [`io::Error`] here instead.
    pub fn reserve<T>(&mut self, elems: usize) -> io::Result<Section<'area, T>>
    where
        T: TryFromBytes + Immutable,
    {
        self.reserve_with(elems, false)
    }

    /// Reserve a new section whose storage is claimed now.
    ///
    /// Like [`SectionWriter::reserve`], but the new range (including any
    /// alignment padding) is written with zeros before it is mapped, so a
    /// full filesystem is reported as an [`io::Error`] by this call rather
    /// than as a fault on a later store into a hole. The section reads as
    /// zeros either way; this costs one write of its length. On
    /// copy-on-write filesystems a later store can still need fresh blocks,
    /// so this narrows the failure window rather than closing it everywhere.
    pub fn reserve_allocated<T>(&mut self, elems: usize) -> io::Result<Section<'area, T>>
    where
        T: TryFromBytes + Immutable,
    {
        self.reserve_with(elems, true)
    }

    fn reserve_with<T>(&mut self, elems: usize, allocate: bool) -> io::Result<Section<'area, T>>
    where
        T: TryFromBytes + Immutable,
    {
        let page = page_size::get();
        let align = core::mem::align_of::<T>();
        let len_bytes = byte_len::<T>(elems)?;
        let start = self
            .area
            .len
            .checked_add(align - 1)
            .map(|padded| padded & !(align - 1))
            .ok_or_else(too_large)?;
        let end = start.checked_add(len_bytes).ok_or_else(too_large)?;

        let aligned_offset = start & !(page - 1);
        let offset = start - aligned_offset;
        let map_len = end - aligned_offset;

        let file = self.area.file.as_file_mut();
        extend(file, self.area.len, end, allocate)?;
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .offset(aligned_offset as u64)
                .len(map_len)
                .map_mut(&*file)?
        };

        self.area.len = end;

        Ok(Section {
            mmap,
            offset,
            start,
            elems,
            _marker: PhantomData,
        })
    }
}

/// Grow `file` from `old` to exactly `end` bytes, leaving its cursor at `end`.
///
/// With `allocate`, the new range is written with zeros first, so its blocks
/// exist before anything maps it and a full filesystem is an [`io::Error`]
/// here. Without it the file grows sparsely by `set_len`.
fn extend(file: &mut std::fs::File, old: usize, end: usize, allocate: bool) -> io::Result<()> {
    if allocate {
        let written = (|| {
            file.seek(SeekFrom::Start(old as u64))?;
            let zeros = [0u8; 64 * 1024];
            let mut remaining = end - old;
            while remaining > 0 {
                let chunk = remaining.min(zeros.len());
                file.write_all(&zeros[..chunk])?;
                remaining -= chunk;
            }
            Ok::<(), io::Error>(())
        })();
        if let Err(error) = written {
            // A partial write may have extended the file; cut it back so
            // the area's length and the file agree again (best effort:
            // the next reservation sets the length exactly anyway).
            let _ = file.set_len(old as u64);
            return Err(error);
        }
    }
    // Set the exact length on both paths, which also removes any tail a
    // failed earlier reservation left beyond the area's length.
    file.set_len(end as u64)?;
    // Ensure subsequent mappings see the extended size.
    file.seek(SeekFrom::Start(end as u64))?;
    Ok(())
}

/// Mutable section reserved from a [`ByteArea`].
///
/// What a section offers depends on its element type; see the
/// [module documentation](self#element-types). Typed references to the slots
/// need `T: FromBytes`. A type with invalid bit patterns is written by value
/// and read back, validated, after [`Section::freeze`]:
///
/// ```
/// use core::num::NonZero;
/// use anybytes::area::Section;
/// use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};
///
/// #[derive(TryFromBytes, IntoBytes, KnownLayout, Immutable, Debug, PartialEq)]
/// #[repr(C)]
/// struct Id(NonZero<u64>);
///
/// let mut section = Section::<Id>::anonymous_allocated(2).unwrap();
/// section.write(0, &Id(NonZero::new(1).unwrap()));
/// section.write(1, &Id(NonZero::new(2).unwrap()));
/// let bytes = section.freeze().unwrap();
/// let ids = bytes.view::<[Id]>().unwrap();
/// assert_eq!(ids[1], Id(NonZero::new(2).unwrap()));
/// ```
///
/// Such a section hands out no `&mut [T]`, because its unwritten slots are
/// zeros and zeros are not a valid `Id`:
///
/// ```compile_fail,E0599
/// # use core::num::NonZero;
/// # use anybytes::area::Section;
/// # use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};
/// # #[derive(TryFromBytes, IntoBytes, KnownLayout, Immutable)]
/// # #[repr(C)]
/// # struct Id(NonZero<u64>);
/// let mut section = Section::<Id>::anonymous_allocated(1).unwrap();
/// let ids: &mut [Id] = section.as_mut_slice();
/// ```
///
/// nor, through `Deref`, any `&[T]`:
///
/// ```compile_fail,E0608
/// # use core::num::NonZero;
/// # use anybytes::area::Section;
/// # use zerocopy::{Immutable, IntoBytes, KnownLayout, TryFromBytes};
/// # #[derive(TryFromBytes, IntoBytes, KnownLayout, Immutable)]
/// # #[repr(C)]
/// # struct Id(NonZero<u64>);
/// let section = Section::<Id>::anonymous_allocated(1).unwrap();
/// let id = &section[0];
/// ```
#[derive(Debug)]
pub struct Section<'arena, T> {
    /// Writable mapping for the current allocation.
    mmap: memmap2::MmapMut,
    /// Offset from the beginning of `mmap` to the start of the buffer.
    offset: usize,
    /// Absolute start offset within the [`ByteArea`] in bytes.
    start: usize,
    /// Number of elements in the buffer.
    elems: usize,
    /// Marker tying the section to the area and element type.
    _marker: PhantomData<(&'arena ByteArea, *mut T)>,
}

// SAFETY: a section uniquely owns its writable mapping. The area appears
// only in the lifetime marker (`*mut T` there keeps `T` invariant), so moving
// or sharing a section is moving or sharing the `[T]` it holds.
unsafe impl<T: Send> Send for Section<'_, T> {}
unsafe impl<T: Sync> Sync for Section<'_, T> {}

impl<T> Section<'static, T>
where
    T: TryFromBytes + Immutable,
{
    /// Create a section in its own anonymous temporary file, with its storage
    /// claimed now.
    ///
    /// Like [`SectionWriter::reserve_allocated`], the range is written with
    /// zeros before it is mapped, so a full filesystem is an [`io::Error`]
    /// here rather than a fault on a later store. The file is unnamed from
    /// the start where the platform allows (`O_TMPFILE` on Linux) and
    /// unlinked immediately otherwise, and its descriptor is closed on
    /// return, so a process that dies leaves no file behind and nothing else
    /// can resize the mapping. The section borrows no area: it owns its
    /// mapping and lives as long as it is kept. [`Section::freeze`] turns it
    /// into [`Bytes`] as for any section, and its [`Section::handle`]
    /// addresses those frozen bytes.
    pub fn anonymous_allocated(elems: usize) -> io::Result<Self> {
        let len = byte_len::<T>(elems)?;
        let mut file = tempfile::tempfile()?;
        extend(&mut file, 0, len, true)?;
        // SAFETY: the file has no name and this is its only descriptor,
        // dropped on return, so no one else can write or truncate it.
        let mmap = unsafe { memmap2::MmapOptions::new().len(len).map_mut(&file)? };
        Ok(Section {
            mmap,
            offset: 0,
            start: 0,
            elems,
            _marker: PhantomData,
        })
    }
}

impl<'arena, T> Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    /// Access the backing slice.
    ///
    /// Requires `T: FromBytes`, since unwritten slots hold zeros. For other
    /// element types, write values with [`Section::write`] or
    /// [`Section::write_slice`].
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(self.offset) as *mut T;
            core::slice::from_raw_parts_mut(ptr, self.elems)
        }
    }
}

impl<'arena, T> Section<'arena, T>
where
    T: IntoBytes + Immutable,
{
    /// The section's slots as raw bytes, `size_of::<T>()` bytes per slot.
    ///
    /// Bytes stored here are only ever read back as bytes, or as `T` through
    /// a validating view after [`Section::freeze`] (or as `&[T]` when
    /// `T: FromBytes`, where any bytes are a valid `T`).
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        let len_bytes = self.elems * core::mem::size_of::<T>();
        &mut self.mmap[self.offset..self.offset + len_bytes]
    }

    /// Copy `value` into slot `index`.
    ///
    /// # Panics
    ///
    /// If `index` is not less than the section's number of elements.
    pub fn write(&mut self, index: usize, value: &T) {
        self.write_slice(index, core::slice::from_ref(value));
    }

    /// Copy `values` into the slots starting at `start`.
    ///
    /// # Panics
    ///
    /// If the slots `start..start + values.len()` do not all lie within the
    /// section.
    pub fn write_slice(&mut self, start: usize, values: &[T]) {
        let elems = self.elems;
        let end = start
            .checked_add(values.len())
            .filter(|&end| end <= elems)
            .unwrap_or_else(|| {
                panic!(
                    "writing {} elements at slot {start} overruns a section of {elems}",
                    values.len()
                )
            });
        // `end <= elems`, and `elems * size_of::<T>()` fits in `usize`
        // (checked at reservation), so neither product overflows.
        let size = core::mem::size_of::<T>();
        self.as_mut_bytes()[start * size..end * size].copy_from_slice(values.as_bytes());
    }
}

impl<'arena, T> Section<'arena, T> {
    /// Freeze the section and return immutable [`Bytes`].
    ///
    /// Freezing changes the mapping's memory protection without synchronizing
    /// dirty pages to durable storage. [`ByteArea::persist`] owns that barrier
    /// when the backing file is deliberately retained.
    pub fn freeze(self) -> io::Result<Bytes> {
        let len_bytes = self.elems * core::mem::size_of::<T>();
        let offset = self.offset;
        // Convert the writable mapping into a read-only view instead of
        // unmapping and remapping the region. The same mapping retains both
        // its dirty pages and its backing object, so visibility and lifetime
        // do not require a durability flush here.
        let map = self.mmap.make_read_only()?;
        Ok(Bytes::from_source(map).slice(offset..offset + len_bytes))
    }

    /// Return a handle that can reconstruct this section from a frozen [`ByteArea`].
    pub fn handle(&self) -> SectionHandle<T> {
        SectionHandle {
            offset: self.start,
            len: self.elems * core::mem::size_of::<T>(),
            _type: PhantomData,
        }
    }
}

impl<'arena, T> core::ops::Deref for Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe {
            let ptr = self.mmap.as_ptr().add(self.offset) as *const T;
            core::slice::from_raw_parts(ptr, self.elems)
        }
    }
}

impl<'arena, T> core::ops::DerefMut for Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    fn deref_mut(&mut self) -> &mut [T] {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(self.offset) as *mut T;
            core::slice::from_raw_parts_mut(ptr, self.elems)
        }
    }
}

impl<'arena, T> AsRef<[T]> for Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    fn as_ref(&self) -> &[T] {
        self
    }
}

impl<'arena, T> AsMut<[T]> for Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    fn as_mut(&mut self) -> &mut [T] {
        self
    }
}

/// Handle referencing a [`Section`] within a frozen [`ByteArea`].
#[derive(Clone, Copy, Debug)]
#[repr(C)]
#[cfg_attr(
    feature = "zerocopy",
    derive(zerocopy::FromBytes, zerocopy::KnownLayout, zerocopy::Immutable,)
)]
pub struct SectionHandle<T> {
    /// Absolute byte offset from the start of the area.
    pub offset: usize,
    /// Length of the section in bytes.
    pub len: usize,
    /// Marker for the element type stored in the section.
    _type: PhantomData<T>,
}

impl<T> SectionHandle<T> {
    /// Extract the raw bytes for this section from `area`.
    pub fn bytes<'a>(&self, area: &'a Bytes) -> Bytes {
        area.slice(self.offset..self.offset + self.len)
    }
}

impl<T> SectionHandle<T>
where
    T: TryFromBytes + Immutable,
{
    /// Interpret the section as a typed [`View`].
    ///
    /// Every element is validated, so this fails for a section with a slot
    /// that does not hold a valid `T`, such as an unwritten slot of a type
    /// whose all-zero bit pattern is invalid.
    pub fn view(&self, area: &Bytes) -> Result<View<[T]>, ViewError> {
        self.bytes(area).view::<[T]>()
    }
}
