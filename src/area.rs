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

use std::io::{self, Seek, SeekFrom};
use std::marker::PhantomData;

use memmap2;
use page_size;
use tempfile::NamedTempFile;

use crate::Bytes;

#[cfg(feature = "zerocopy")]
use zerocopy::{FromBytes, Immutable};

#[cfg(feature = "zerocopy")]
use crate::view::{View, ViewError};

/// Alignment helper.
fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
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

    /// Persist the temporary area file to `path` and return the underlying [`std::fs::File`].
    ///
    /// All mapped writes are synchronized once before the temporary file is
    /// retained. The rename itself and its parent directory are not separately
    /// synchronized, so this does not promise crash-atomic path publication.
    ///
    /// The persistence contract is supported on Unix targets. Other targets
    /// are best-effort rather than part of AnyBytes' support surface.
    pub fn persist<P: AsRef<std::path::Path>>(self, path: P) -> io::Result<std::fs::File> {
        self.file.as_file().sync_all()?;
        self.file.persist(path).map_err(Into::into)
    }
}

/// RAII guard giving temporary exclusive write access.
#[derive(Debug)]
pub struct SectionWriter<'area> {
    area: &'area mut ByteArea,
}

impl<'area> SectionWriter<'area> {
    /// Reserve a new section inside the area.
    pub fn reserve<T>(&mut self, elems: usize) -> io::Result<Section<'area, T>>
    where
        T: FromBytes + Immutable,
    {
        let page = page_size::get();
        let align = core::mem::align_of::<T>();
        let len_bytes = core::mem::size_of::<T>() * elems;
        let start = align_up(self.area.len, align);
        let end = start + len_bytes;

        let aligned_offset = start & !(page - 1);
        let offset = start - aligned_offset;
        let map_len = end - aligned_offset;

        let file = &mut self.area.file;
        file.as_file_mut().set_len(end as u64)?;
        // Ensure subsequent mappings see the extended size.
        file.as_file_mut().seek(SeekFrom::Start(end as u64))?;
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .offset(aligned_offset as u64)
                .len(map_len)
                .map_mut(file.as_file())?
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

/// Mutable section reserved from a [`ByteArea`].
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

impl<'arena, T> Section<'arena, T>
where
    T: FromBytes + Immutable,
{
    /// Access the backing slice.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(self.offset) as *mut T;
            core::slice::from_raw_parts_mut(ptr, self.elems)
        }
    }

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

impl<T> SectionHandle<T>
where
    T: FromBytes + Immutable,
{
    /// Extract the raw bytes for this section from `area`.
    pub fn bytes<'a>(&self, area: &'a Bytes) -> Bytes {
        area.slice(self.offset..self.offset + self.len)
    }

    /// Interpret the section as a typed [`View`].
    pub fn view(&self, area: &Bytes) -> Result<View<[T]>, ViewError> {
        self.bytes(area).view::<[T]>()
    }
}
