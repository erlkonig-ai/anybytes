/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 * Copyright (c) Jan-Paul Bultmann
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use proptest::prelude::*;

use crate::Bytes;

proptest! {
    #[test]
    fn test_shallow_clone(v in proptest::collection::vec(any::<u8>(), 0..256)) {
        let a: Bytes = v.into();
        let b: Bytes = a.clone();
        prop_assert_eq!(a.as_ref(), b.as_ref());
        prop_assert_eq!(a.as_ptr(), b.as_ptr());
    }

    #[test]
    fn test_shallow_slice(v in proptest::collection::vec(any::<u8>(), 0..256)) {
        let a: Bytes = v.into();
        let b: Bytes = a.slice(..a.len() / 2);
        prop_assert_eq!(&b[..], &a[..b.len()]);
        prop_assert!(b.is_empty() || a.as_ptr() == b.as_ptr());
    }

    #[test]
    fn test_take_prefix_matches_split(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        for len in 0..=data.len() {
            let mut bytes = Bytes::from(data.clone());
            let prefix = bytes.take_prefix(len).expect("prefix within bounds");
            prop_assert_eq!(prefix.as_ref(), &data[..len]);
            prop_assert_eq!(bytes.as_ref(), &data[len..]);
        }

        let mut bytes = Bytes::from(data.clone());
        prop_assert!(bytes.take_prefix(data.len() + 1).is_none());
        prop_assert_eq!(bytes.as_ref(), data.as_slice());
    }

    #[test]
    fn test_take_suffix_matches_split(data in proptest::collection::vec(any::<u8>(), 0..64)) {
        for len in 0..=data.len() {
            let mut bytes = Bytes::from(data.clone());
            let suffix = bytes.take_suffix(len).expect("suffix within bounds");
            let split = data.len() - len;
            prop_assert_eq!(suffix.as_ref(), &data[split..]);
            prop_assert_eq!(bytes.as_ref(), &data[..split]);
        }

        let mut bytes = Bytes::from(data.clone());
        prop_assert!(bytes.take_suffix(data.len() + 1).is_none());
        prop_assert_eq!(bytes.as_ref(), data.as_slice());
    }
}

#[test]
fn test_downcast() {
    let v = b"abcd".to_vec();
    let b = Bytes::from(v);
    assert!(b.downcast_to_owner::<Vec<u8>>().is_ok());

    let v = b"abcd".to_vec();
    let b = Bytes::from(v);
    match b.downcast_to_owner::<String>() {
        Ok(_) => panic!("unexpected success"),
        Err(orig) => assert_eq!(orig.as_ref(), b"abcd"),
    }
}

#[test]
fn test_try_unwrap_owner() {
    // Success when the owner is uniquely referenced
    let b = Bytes::from(b"abcd".to_vec());
    let v = b.try_unwrap_owner::<Vec<u8>>().expect("unwrap owner");
    assert_eq!(v, b"abcd".to_vec());

    // Failure when multiple references exist
    let b1 = Bytes::from(b"abcd".to_vec());
    let b2 = b1.clone();
    let result = b1.try_unwrap_owner::<Vec<u8>>();
    assert!(result.is_err());

    // Failure when type does not match
    let other = b2.try_unwrap_owner::<String>();
    assert!(other.is_err());
}

#[test]
fn test_bytes_debug_format() {
    let v = b"printable\t\r\n\'\"\\\x00\x01\x02printable".to_vec();
    let b = Bytes::from(v);
    let escaped = format!("{:?}", b);
    let expected = r#"b"printable\t\r\n\'\"\\\x00\x01\x02printable""#;
    assert_eq!(escaped, expected);
}

#[test]
fn test_downgrade_upgrade() {
    let v = b"abcd".to_vec();
    let b = Bytes::from(v);

    // `downgrade` -> `upgrade` returns the same slice.
    let b1 = b.slice(1..=2);
    let wb = b1.downgrade();
    let b2 = wb.upgrade().unwrap();
    assert_eq!(b1, b2);

    // `upgrade` returns `None` if all strong refs are dropped.
    drop(b);
    drop(b1);
    drop(b2);
    let b3 = wb.upgrade();
    assert!(b3.is_none());
}
#[test]
fn test_slice_to_bytes_same_source() {
    let bytes = Bytes::from(b"abcdef".to_vec());
    let slice = &bytes[1..4];
    let result = bytes.slice_to_bytes(slice).expect("slice from same bytes");
    assert_eq!(result, bytes.slice(1..4));
}

#[test]
fn test_slice_to_bytes_unrelated_slice() {
    let bytes = Bytes::from(b"abcdef".to_vec());
    let other = b"xyz123".to_vec();
    let slice = &other[1..4];
    assert!(bytes.slice_to_bytes(slice).is_none());
}

#[test]
fn test_vecdeque_source() {
    use std::collections::VecDeque;

    let mut deque = VecDeque::new();
    deque.extend([1u8, 2, 3, 4]);
    deque.make_contiguous();
    let bytes = Bytes::from_source(deque);
    assert_eq!(bytes.as_ref(), &[1, 2, 3, 4]);
    let owner = bytes
        .downcast_to_owner::<VecDeque<u8>>()
        .expect("downcast VecDeque owner");
    assert_eq!(&*owner, &VecDeque::from([1u8, 2, 3, 4]));
}

#[cfg(feature = "zerocopy")]
#[test]
fn test_vecdeque_generic_source() {
    use std::collections::VecDeque;

    let mut deque = VecDeque::new();
    deque.extend([1u16, 2, 3, 4]);
    deque.make_contiguous();
    let bytes = Bytes::from_source(deque);

    let mut expected = Vec::new();
    for n in [1u16, 2, 3, 4] {
        expected.extend_from_slice(&n.to_ne_bytes());
    }
    assert_eq!(bytes.as_ref(), expected.as_slice());
    let owner = bytes
        .downcast_to_owner::<VecDeque<u16>>()
        .expect("downcast VecDeque owner");
    assert_eq!(&*owner, &VecDeque::from([1u16, 2, 3, 4]));
}

#[test]
#[should_panic]
fn test_vecdeque_noncontiguous_panics() {
    use std::collections::VecDeque;

    let mut deque: VecDeque<u8> = VecDeque::with_capacity(3);
    deque.push_back(1);
    deque.push_back(2);
    deque.push_back(3);
    let _ = deque.pop_front();
    deque.push_back(4); // wraps around
    assert!(!deque.as_slices().1.is_empty());
    let _ = Bytes::from_source(deque);
}

#[test]
fn test_pop_front() {
    let mut bytes = Bytes::from(b"abc".to_vec());
    assert_eq!(bytes.pop_front(), Some(b'a'));
    assert_eq!(bytes.as_ref(), b"bc");
    assert_eq!(bytes.pop_front(), Some(b'b'));
    assert_eq!(bytes.pop_front(), Some(b'c'));
    assert_eq!(bytes.pop_front(), None);
}

#[test]
fn test_pop_back() {
    let mut bytes = Bytes::from(b"abc".to_vec());
    assert_eq!(bytes.pop_back(), Some(b'c'));
    assert_eq!(bytes.as_ref(), b"ab");
    assert_eq!(bytes.pop_back(), Some(b'b'));
    assert_eq!(bytes.pop_back(), Some(b'a'));
    assert_eq!(bytes.pop_back(), None);
}

#[test]
fn test_take_prefix() {
    let mut bytes = Bytes::from(b"abcdef".to_vec());
    let prefix = bytes.take_prefix(2).expect("prefix");
    assert_eq!(prefix.as_ref(), b"ab");
    assert_eq!(bytes.as_ref(), b"cdef");
    assert!(bytes.take_prefix(10).is_none());
}

#[test]
fn test_take_suffix() {
    let mut bytes = Bytes::from(b"abcdef".to_vec());
    let suffix = bytes.take_suffix(2).expect("suffix");
    assert_eq!(suffix.as_ref(), b"ef");
    assert_eq!(bytes.as_ref(), b"abcd");
    assert!(bytes.take_suffix(10).is_none());
}

#[test]
fn test_weakbytes_multiple_upgrades() {
    let bytes = Bytes::from(b"hello".to_vec());
    let weak = bytes.downgrade();

    // Upgrade works while strong reference exists
    let strong1 = weak.upgrade().unwrap();
    assert_eq!(strong1.as_ref(), bytes.as_ref());
    drop(strong1);

    // Can upgrade multiple times
    let strong2 = weak.upgrade().unwrap();
    assert_eq!(strong2.as_ref(), b"hello".as_ref());

    drop(bytes);
    drop(strong2);

    // After all strong refs are gone, upgrade returns None
    assert!(weak.upgrade().is_none());
}

#[test]
fn test_weakbytes_clone_upgrade() {
    let bytes = Bytes::from(b"hello".to_vec());
    let weak = bytes.downgrade();
    let weak_clone = weak.clone();

    let strong = weak_clone.upgrade().unwrap();
    assert_eq!(strong.as_ref(), bytes.as_ref());

    drop(bytes);
    drop(strong);

    assert!(weak.upgrade().is_none());
    assert!(weak_clone.upgrade().is_none());
}

#[cfg(feature = "zerocopy")]
#[test]
fn test_weakview_downgrade_upgrade() {
    let bytes = Bytes::from(b"abcdef".to_vec());
    let view = bytes.clone().view::<[u8]>().unwrap();

    let weak = view.downgrade();
    let strong = weak.upgrade().unwrap();
    assert_eq!(strong.as_ref(), view.as_ref());

    drop(bytes);
    drop(view);
    drop(strong);

    assert!(weak.upgrade().is_none());
}

#[cfg(feature = "zerocopy")]
#[test]
fn test_weakview_clone_upgrade() {
    let bytes = Bytes::from(b"abcdef".to_vec());
    let view = bytes.clone().view::<[u8]>().unwrap();

    let weak = view.downgrade();
    let weak_clone = weak.clone();

    let strong = weak_clone.upgrade().unwrap();
    assert_eq!(strong.as_ref(), view.as_ref());

    drop(bytes);
    drop(view);
    drop(strong);

    assert!(weak.upgrade().is_none());
    assert!(weak_clone.upgrade().is_none());
}

#[cfg(feature = "winnow")]
#[test]
fn test_winnow_stream_take() {
    use winnow::error::ContextError;
    use winnow::stream::AsBytes;
    use winnow::token::take;
    use winnow::Parser;

    let mut input = Bytes::from(vec![1u8, 2, 3, 4]);
    let mut parser = take::<_, _, ContextError>(2usize);
    let prefix: Bytes = parser.parse_next(&mut input).expect("take");
    assert_eq!(prefix.as_ref(), [1u8, 2].as_ref());
    assert_eq!(input.as_bytes(), [3u8, 4].as_ref());
}

#[cfg(feature = "winnow")]
#[test]
fn test_iter_offsets_traits() {
    use std::iter::{ExactSizeIterator, FusedIterator};
    use winnow::stream::Stream;

    fn assert_traits<I: ExactSizeIterator + FusedIterator>(iter: I) -> I {
        iter
    }

    let bytes = Bytes::from(vec![1u8, 2, 3, 4]);
    let mut iter = assert_traits(Stream::iter_offsets(&bytes));
    assert_eq!(iter.len(), 4);
    assert_eq!(iter.size_hint(), (4, Some(4)));

    for (i, (offset, byte)) in (&mut iter).enumerate() {
        assert_eq!(offset, i);
        assert_eq!(byte, bytes.as_ref()[i]);
    }

    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next(), None);
}

#[cfg(all(feature = "winnow", feature = "zerocopy"))]
#[test]
fn test_winnow_view_parser() {
    use winnow::error::ContextError;
    use winnow::stream::AsBytes;
    use winnow::Parser;
    #[derive(zerocopy::TryFromBytes, zerocopy::KnownLayout, zerocopy::Immutable)]
    #[repr(C)]
    struct Pair {
        a: u16,
        b: u16,
    }

    let mut input = Bytes::from(vec![1u8, 0, 2, 0]);
    let mut parser = crate::winnow::view::<Pair, ContextError>;
    let view = parser.parse_next(&mut input).expect("view");
    assert_eq!(view.a, 1);
    assert_eq!(view.b, 2);
    assert!(input.as_bytes().is_empty());
}

#[cfg(all(feature = "winnow", feature = "zerocopy"))]
#[test]
fn test_winnow_view_elems_parser() {
    use winnow::error::ContextError;
    use winnow::stream::AsBytes;
    use winnow::Parser;

    let mut input = Bytes::from(vec![1u8, 2, 3, 4]);
    let mut parser = crate::winnow::view_elems::<[u8], ContextError>(3);
    let view = parser.parse_next(&mut input).expect("view_elems");
    assert_eq!(view.as_ref(), [1u8, 2, 3].as_ref());
    assert_eq!(input.as_bytes(), [4u8].as_ref());
}

#[cfg(feature = "mmap")]
#[test]
fn test_mmap_mut_source() {
    let mut mmap = memmap2::MmapMut::map_anon(4).expect("mmap");
    mmap.copy_from_slice(b"test");
    let bytes = Bytes::from_source(mmap);
    assert_eq!(bytes.as_ref(), b"test");
}

#[cfg(feature = "mmap")]
#[test]
fn test_map_file() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(b"testfile").expect("write");
    file.flush().unwrap();
    let bytes = unsafe { Bytes::map_file(&file) }.expect("map file");
    assert_eq!(bytes.as_ref(), b"testfile");
}

#[cfg(feature = "mmap")]
#[test]
fn test_map_file_region() {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(b"abcdef").expect("write");
    file.flush().unwrap();
    let bytes = unsafe { Bytes::map_file_region(&file, 1, 3) }.expect("map file region");
    assert_eq!(bytes.as_ref(), b"bcd");
}

#[test]
fn test_cow_u8_owned_source() {
    use std::borrow::Cow;

    let owned: Cow<'static, [u8]> = Cow::Owned(vec![1, 2, 3, 4]);
    let bytes_owned = Bytes::from_source(owned.clone());
    assert_eq!(bytes_owned.as_ref(), owned.as_ref());
}

#[test]
fn test_cow_u8_borrowed_source() {
    use std::borrow::Cow;

    let borrowed: Cow<'static, [u8]> = Cow::Borrowed(b"abcd");
    let bytes_borrowed = Bytes::from_source(borrowed.clone());
    assert_eq!(bytes_borrowed.as_ref(), borrowed.as_ref());
}

#[test]
fn test_cow_str_owned_source() {
    use std::borrow::Cow;

    let owned: Cow<'static, str> = Cow::Owned(String::from("abcd"));
    let bytes_owned = Bytes::from_source(owned.clone());
    assert_eq!(bytes_owned.as_ref(), owned.as_bytes());
}

#[test]
fn test_cow_str_borrowed_source() {
    use std::borrow::Cow;

    let borrowed: Cow<'static, str> = Cow::Borrowed("abcd");
    let bytes_borrowed = Bytes::from_source(borrowed.clone());
    assert_eq!(bytes_borrowed.as_ref(), borrowed.as_bytes());
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_single_reserve() {
    use crate::area::ByteArea;

    let mut area = ByteArea::new().expect("area");
    {
        let mut sections = area.sections();
        let mut section = sections.reserve::<u8>(4).expect("reserve");
        section.as_mut_slice().copy_from_slice(b"test");
        let bytes = section.freeze().expect("freeze section");
        assert_eq!(bytes.as_ref(), b"test");
    }

    let all = area.freeze().expect("freeze area");
    assert_eq!(all.as_ref(), b"test");
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_multiple_reserves() {
    use crate::area::ByteArea;

    let mut area = ByteArea::new().expect("area");
    {
        let mut sections = area.sections();

        let mut a = sections.reserve::<u8>(5).expect("reserve");
        a.as_mut_slice().copy_from_slice(b"first");
        let bytes_a = a.freeze().expect("freeze");
        assert_eq!(bytes_a.as_ref(), b"first");

        let mut b = sections.reserve::<u8>(6).expect("reserve");
        b.as_mut_slice().copy_from_slice(b"second");
        let bytes_b = b.freeze().expect("freeze");
        assert_eq!(bytes_b.as_ref(), b"second");
    }

    let all = area.freeze().expect("freeze area");
    assert_eq!(all.as_ref(), b"firstsecond");
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_concurrent_sections() {
    use crate::area::ByteArea;

    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();

    let mut a = sections.reserve::<u8>(5).expect("reserve a");
    let mut b = sections.reserve::<u8>(6).expect("reserve b");
    a.as_mut_slice().copy_from_slice(b"first");
    b.as_mut_slice().copy_from_slice(b"second");

    let bytes_b = b.freeze().expect("freeze b");
    let bytes_a = a.freeze().expect("freeze a");

    assert_eq!(bytes_a.as_ref(), b"first");
    assert_eq!(bytes_b.as_ref(), b"second");

    drop(sections);
    let all = area.freeze().expect("freeze area");
    assert_eq!(all.as_ref(), b"firstsecond");
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_typed() {
    use crate::area::ByteArea;

    #[derive(zerocopy::FromBytes, zerocopy::Immutable, Clone, Copy)]
    #[repr(C)]
    struct Pair {
        a: u16,
        b: u32,
    }

    let mut area = ByteArea::new().expect("area");
    let bytes = {
        let mut sections = area.sections();
        let mut section = sections.reserve::<Pair>(2).expect("reserve");
        section.as_mut_slice()[0] = Pair { a: 1, b: 2 };
        section.as_mut_slice()[1] = Pair { a: 3, b: 4 };
        section.freeze().expect("freeze")
    };

    let expected = unsafe {
        core::slice::from_raw_parts(
            [Pair { a: 1, b: 2 }, Pair { a: 3, b: 4 }].as_ptr() as *const u8,
            2 * core::mem::size_of::<Pair>(),
        )
    };
    assert_eq!(bytes.as_ref(), expected);
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_persist() {
    use crate::area::ByteArea;
    use std::fs;

    let dir = tempfile::tempdir().expect("dir");
    let path = dir.path().join("persist.bin");

    let mut area = ByteArea::new().expect("area");
    {
        let mut sections = area.sections();
        let mut section = sections.reserve::<u8>(7).expect("reserve");
        section.as_mut_slice().copy_from_slice(b"persist");
        section.freeze().expect("freeze section");
    }

    let _file = area.persist(&path).expect("persist file");
    let data = fs::read(&path).expect("read");
    assert_eq!(data.as_slice(), b"persist");
}

#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn test_area_alignment_padding() {
    use crate::area::ByteArea;

    let mut area = ByteArea::new().expect("area");
    {
        let mut sections = area.sections();

        let mut a = sections.reserve::<u8>(1).expect("reserve");
        a.as_mut_slice()[0] = 1;
        let bytes_a = a.freeze().expect("freeze a");
        assert_eq!(bytes_a.as_ref(), &[1]);

        let mut b = sections.reserve::<u32>(1).expect("reserve");
        b.as_mut_slice()[0] = 0x01020304;
        let bytes_b = b.freeze().expect("freeze b");
        assert_eq!(bytes_b.as_ref(), &0x01020304u32.to_ne_bytes());

        let mut c = sections.reserve::<u16>(1).expect("reserve");
        c.as_mut_slice()[0] = 0x0506;
        let bytes_c = c.freeze().expect("freeze c");
        assert_eq!(bytes_c.as_ref(), &0x0506u16.to_ne_bytes());
    }

    let all = area.freeze().expect("freeze area");

    let mut expected = Vec::new();
    expected.extend_from_slice(&[1]);
    expected.extend_from_slice(&[0; 3]);
    expected.extend_from_slice(&0x01020304u32.to_ne_bytes());
    expected.extend_from_slice(&0x0506u16.to_ne_bytes());
    assert_eq!(all.as_ref(), expected.as_slice());
}

#[cfg(feature = "pyo3")]
#[test]
fn test_pyanybytes_memoryview() {
    use crate::{Bytes, PyAnyBytes};
    use pyo3::types::{PyAnyMethods, PyMemoryView};
    use pyo3::{Py, Python};

    pyo3::prepare_freethreaded_python();
    Python::with_gil(|py| {
        let data = b"memoryview";
        let bytes = PyAnyBytes::new(Bytes::from(data.to_vec()));
        let py_obj = Py::new(py, bytes).expect("PyAnyBytes");
        let view = PyMemoryView::from(py_obj.bind(py).as_any()).expect("memoryview");

        let mv_bytes: Vec<u8> = view
            .call_method0("tobytes")
            .expect("tobytes")
            .extract()
            .expect("extract bytes");
        assert_eq!(mv_bytes.as_slice(), data);

        let readonly: bool = view
            .getattr("readonly")
            .expect("readonly attr")
            .extract()
            .expect("extract bool");
        assert!(readonly);
    });
}

#[cfg(feature = "burn")]
#[test]
fn test_burn_bytes_source_to_anybytes_is_zero_copy() {
    use burn_tensor::Bytes as BurnBytes;

    let burn = BurnBytes::from_bytes_vec(vec![1u8, 2, 3, 4]);
    let burn_ptr = (&burn[..]).as_ptr();
    let burn_len = burn.len();

    let any = Bytes::from_source(burn);
    assert_eq!(any.as_ref(), [1u8, 2, 3, 4].as_ref());
    assert_eq!(any.len(), burn_len);
    assert_eq!(any.as_ref().as_ptr(), burn_ptr);

    let owner = any
        .clone()
        .downcast_to_owner::<BurnBytes>()
        .expect("owner should be burn_tensor::Bytes");
    assert_eq!(owner.len(), burn_len);
}

#[cfg(feature = "burn")]
#[test]
fn test_anybytes_into_burn_bytes_preserves_full_slice() {
    use burn_tensor::Bytes as BurnBytes;

    let any = Bytes::from_source(vec![9u8, 8, 7, 6]);
    let any_ptr = any.as_ref().as_ptr();

    let burn: BurnBytes = any.clone().into();
    assert_eq!(&burn[..], any.as_ref());
    assert_eq!((&burn[..]).as_ptr(), any_ptr);
}

#[cfg(feature = "burn")]
#[test]
fn test_anybytes_into_burn_bytes_preserves_sliced_view() {
    use burn_tensor::Bytes as BurnBytes;

    let base = Bytes::from_source(vec![10u8, 11, 12, 13, 14, 15]);
    let slice = base.slice(2..5);
    let slice_ptr = slice.as_ref().as_ptr();

    let burn: BurnBytes = slice.clone().into();
    assert_eq!(&burn[..], [12u8, 13, 14].as_ref());
    assert_eq!((&burn[..]).as_ptr(), slice_ptr);

    let any_again = Bytes::from_source(burn);
    drop(base);
    drop(slice);

    assert_eq!(any_again.as_ref(), [12u8, 13, 14].as_ref());
    assert_eq!(any_again.as_ref().as_ptr(), slice_ptr);
}

/// The mappings behind frozen area and section `Bytes` cannot be recovered
/// as a `memmap2::Mmap`, so they cannot be made writable while a validated
/// view of the same file is alive. Before, `area.freeze()` gave a second,
/// independent mapping whose owner could be unwrapped and `make_mut`ed,
/// turning a validated `NonZeroU8` into zero in safe code.
#[cfg(all(feature = "mmap", feature = "zerocopy"))]
#[test]
fn frozen_area_mappings_cannot_be_recovered_as_mmap() {
    use crate::area::ByteArea;
    use core::num::NonZeroU8;

    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let mut section = sections.reserve::<u8>(1).expect("reserve");
    section.as_mut_slice()[0] = 1;
    let frozen = section.freeze().expect("freeze section");
    drop(sections);
    let view = frozen
        .clone()
        .view::<[NonZeroU8]>()
        .expect("valid non-zero view");

    let whole = area.freeze().expect("freeze area");
    let whole = whole
        .try_unwrap_owner::<memmap2::Mmap>()
        .expect_err("area mapping recovered as Mmap");
    assert!(whole.downcast_to_owner::<memmap2::Mmap>().is_err());
    assert!(frozen
        .clone()
        .try_unwrap_owner::<memmap2::Mmap>()
        .is_err());
    assert_eq!(view[0].get(), 1);
}
