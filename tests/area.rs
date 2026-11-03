#![cfg(all(feature = "mmap", feature = "zerocopy"))]

use anybytes::area::{ByteArea, Section};
use proptest::prelude::*;

fn align_up(val: usize, align: usize) -> usize {
    (val + align - 1) & !(align - 1)
}

#[derive(Debug, Clone)]
enum Segment {
    U8(Vec<u8>),
    U16(Vec<u16>),
    U32(Vec<u32>),
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]
    #[test]
    fn freeze_preserves_layout(segs in prop::collection::vec(
        prop_oneof![
            prop::collection::vec(any::<u8>(), 1..4).prop_map(Segment::U8),
            prop::collection::vec(any::<u16>(), 1..4).prop_map(Segment::U16),
            prop::collection::vec(any::<u32>(), 1..4).prop_map(Segment::U32),
        ],
        0..4,
    )) {
        let mut area = ByteArea::new().expect("area");
        let mut sections = area.sections();
        let mut expected: Vec<u8> = Vec::new();

        for seg in segs {
            match seg {
                Segment::U8(data) => {
                    let start = align_up(expected.len(), core::mem::align_of::<u8>());
                    expected.resize(start, 0);
                    let mut section = sections.reserve::<u8>(data.len()).expect("reserve u8");
                    section.as_mut_slice().copy_from_slice(&data);
                    let bytes = section.freeze().expect("freeze");
                    expected.extend_from_slice(&data);
                    let end = expected.len();
                    prop_assert_eq!(bytes.as_ref(), &expected[start..end]);
                }
                Segment::U16(data) => {
                    let start = align_up(expected.len(), core::mem::align_of::<u16>());
                    expected.resize(start, 0);
                    let mut section = sections.reserve::<u16>(data.len()).expect("reserve u16");
                    section.as_mut_slice().copy_from_slice(&data);
                    let bytes = section.freeze().expect("freeze");
                    for v in &data {
                        expected.extend_from_slice(&v.to_ne_bytes());
                    }
                    let end = expected.len();
                    prop_assert_eq!(bytes.as_ref(), &expected[start..end]);
                }
                Segment::U32(data) => {
                    let start = align_up(expected.len(), core::mem::align_of::<u32>());
                    expected.resize(start, 0);
                    let mut section = sections.reserve::<u32>(data.len()).expect("reserve u32");
                    section.as_mut_slice().copy_from_slice(&data);
                    let bytes = section.freeze().expect("freeze");
                    for v in &data {
                        expected.extend_from_slice(&v.to_ne_bytes());
                    }
                    let end = expected.len();
                    prop_assert_eq!(bytes.as_ref(), &expected[start..end]);
                }
            }
        }

        drop(sections);
        let all = area.freeze().expect("freeze area");
        prop_assert_eq!(all.as_ref(), expected.as_slice());
    }
}

#[test]
fn handles_reconstruct_sections() {
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();

    let mut a = sections.reserve::<u8>(1).expect("reserve u8");
    a.as_mut_slice()[0] = 1;
    let handle_a = a.handle();
    let bytes_a = a.freeze().expect("freeze a");

    let mut b = sections.reserve::<u32>(1).expect("reserve u32");
    b.as_mut_slice()[0] = 2;
    let handle_b = b.handle();
    let bytes_b = b.freeze().expect("freeze b");

    drop(sections);
    let all = area.freeze().expect("freeze area");

    assert_eq!(handle_a.bytes(&all).as_ref(), bytes_a.as_ref());
    assert_eq!(handle_b.bytes(&all).as_ref(), bytes_b.as_ref());
    assert_eq!(handle_a.view(&all).unwrap().as_ref(), &[1]);
    assert_eq!(handle_b.view(&all).unwrap().as_ref(), &[2]);
}

#[test]
fn handles_can_be_stored_in_sections() {
    use anybytes::area::SectionHandle;

    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();

    let mut value = sections.reserve::<u8>(1).expect("reserve value");
    value.as_mut_slice()[0] = 3;
    let handle_value = value.handle();
    value.freeze().expect("freeze value");

    let mut handles = sections
        .reserve::<SectionHandle<u8>>(1)
        .expect("reserve handles");
    handles.as_mut_slice()[0] = handle_value;
    let handle_handles = handles.handle();
    handles.freeze().expect("freeze handles");

    drop(sections);
    let all = area.freeze().expect("freeze area");

    let stored_handle = handle_handles.view(&all).expect("view handles").as_ref()[0];
    assert_eq!(stored_handle.view(&all).unwrap().as_ref(), &[3]);
}

#[test]
fn frozen_section_outlives_temporary_area() {
    let bytes = {
        let mut area = ByteArea::new().expect("area");
        let mut sections = area.sections();
        let mut section = sections.reserve::<u8>(4).expect("reserve");
        section.copy_from_slice(&[1, 2, 3, 4]);
        section.freeze().expect("freeze")
    };

    assert_eq!(bytes.as_ref(), &[1, 2, 3, 4]);
}

#[test]
fn persisted_area_observes_frozen_section() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("area.bin");

    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let mut section = sections.reserve::<u8>(4).expect("reserve");
    section.copy_from_slice(&[5, 6, 7, 8]);
    let bytes = section.freeze().expect("freeze");
    drop(sections);

    let file = area.persist(&path).expect("persist");
    assert_eq!(
        std::fs::read(&path).expect("read persisted area"),
        bytes.as_ref()
    );

    drop(file);
    assert_eq!(bytes.as_ref(), &[5, 6, 7, 8]);
}

/// The handle `persist` returns cannot write the file that frozen views
/// map: a write through it fails, and a NonZero value validated before
/// persisting still reads back unchanged.
#[test]
fn persisted_area_handle_cannot_change_validated_views() {
    use std::io::Write;

    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("area.bin");

    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let mut section = sections.reserve::<NonZeroPair>(1).expect("reserve");
    section.write(0, &pair(7, 9));
    let bytes = section.freeze().expect("freeze");
    drop(sections);
    let view = bytes.clone().view::<[NonZeroPair]>().expect("valid view");

    let mut file = area.persist(&path).expect("persist");
    assert!(file.write_all(&[0u8; 32]).is_err(), "persist handle wrote");
    assert_eq!(view[0].a.get(), 7);
    assert_eq!(view[0].b.get(), 9);
}

#[test]
fn allocated_sections_lay_out_like_sparse_ones() {
    let mut sparse = ByteArea::new().expect("area");
    let mut allocated = ByteArea::new().expect("area");
    {
        let mut s = sparse.sections();
        let mut a = allocated.sections();
        let mut s1 = s.reserve::<u8>(3).expect("reserve");
        let mut a1 = a.reserve_allocated::<u8>(3).expect("reserve");
        s1.copy_from_slice(&[1, 2, 3]);
        a1.copy_from_slice(&[1, 2, 3]);
        let mut s2 = s.reserve::<u64>(2).expect("reserve");
        let mut a2 = a.reserve::<u64>(2).expect("reserve");
        s2.as_mut_slice().copy_from_slice(&[7, 8]);
        a2.as_mut_slice().copy_from_slice(&[7, 8]);
        let s3 = s.reserve::<u32>(5).expect("reserve");
        let a3 = a.reserve_allocated::<u32>(5).expect("reserve");
        assert!(a3.as_ref().iter().all(|&byte| byte == 0));
        drop((s1, a1, s2, a2, s3, a3));
    }
    let sparse = sparse.freeze().expect("freeze");
    let allocated = allocated.freeze().expect("freeze");
    assert_eq!(sparse.as_ref(), allocated.as_ref());
}

#[test]
fn frozen_allocated_section_outlives_temporary_area() {
    let bytes = {
        let mut area = ByteArea::new().expect("area");
        let mut sections = area.sections();
        let mut section = sections.reserve_allocated::<u8>(4).expect("reserve");
        section.copy_from_slice(&[4, 3, 2, 1]);
        section.freeze().expect("freeze")
    };
    assert_eq!(bytes.as_ref(), &[4, 3, 2, 1]);
}

#[cfg(unix)]
#[test]
fn allocated_section_claims_its_blocks() {
    use std::os::unix::fs::MetadataExt;
    const LEN: usize = 1 << 20;
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("allocated.bin");
    let mut area = ByteArea::new().expect("area");
    {
        let mut sections = area.sections();
        let section = sections.reserve_allocated::<u8>(LEN).expect("reserve");
        drop(section);
    }
    let file = area.persist(&path).expect("persist");
    let metadata = file.metadata().expect("metadata");
    assert_eq!(metadata.len(), LEN as u64);
    assert!(
        metadata.blocks() * 512 >= LEN as u64,
        "allocated range is backed by blocks"
    );
}

#[test]
fn anonymous_section_is_zeroed_writable_and_freezes() {
    let mut section = Section::<u32>::anonymous_allocated(3).expect("anonymous section");
    assert_eq!(section.as_ref(), &[0, 0, 0]);
    section.as_mut_slice().copy_from_slice(&[7, 8, 9]);
    let handle = section.handle();
    let bytes = section.freeze().expect("freeze");

    let mut expected = Vec::new();
    for value in [7u32, 8, 9] {
        expected.extend_from_slice(&value.to_ne_bytes());
    }
    assert_eq!(bytes.as_ref(), expected.as_slice());
    assert_eq!(handle.view(&bytes).expect("view").as_ref(), &[7, 8, 9]);
}

#[test]
fn anonymous_section_moves_across_threads() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<Section<'static, u8>>();

    let mut section = Section::<u8>::anonymous_allocated(4).expect("anonymous section");
    section.copy_from_slice(&[1, 2, 3, 4]);
    let bytes = std::thread::spawn(move || section.freeze().expect("freeze"))
        .join()
        .expect("thread");
    assert_eq!(bytes.as_ref(), &[1, 2, 3, 4]);
}

/// The mapping is backed by a file that has no name.
#[cfg(target_os = "linux")]
#[test]
fn anonymous_section_is_unlinked() {
    const LEN: usize = 1 << 20;
    let section = Section::<u8>::anonymous_allocated(LEN).expect("anonymous section");
    let at = section.as_ptr() as usize;
    let maps = std::fs::read_to_string("/proc/self/maps").expect("maps");
    let line = maps
        .lines()
        .find(|line| {
            let range = line.split(' ').next().unwrap();
            let (start, end) = range.split_once('-').unwrap();
            let start = usize::from_str_radix(start, 16).unwrap();
            let end = usize::from_str_radix(end, 16).unwrap();
            start <= at && at < end
        })
        .expect("the section is mapped");
    assert!(
        line.ends_with("(deleted)"),
        "backing file has a name: {line}"
    );
}

/// A file that cannot grow is an error from the constructor, not a fault
/// later. Runs itself again in a child with a small file size limit and
/// SIGXFSZ ignored, so the zero write fails with `EFBIG`.
#[cfg(target_os = "linux")]
#[test]
fn anonymous_section_reports_storage_it_cannot_claim() {
    const CHILD: &str = "ANYBYTES_AREA_FSIZE_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let error = Section::<u8>::anonymous_allocated(1 << 20).expect_err("over the limit");
        assert_eq!(error.raw_os_error(), Some(27), "EFBIG: {error}");
        return;
    }
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"trap "" XFSZ; ulimit -f 64 && exec "$0" --exact anonymous_section_reports_storage_it_cannot_claim --test-threads 1"#)
        .arg(std::env::current_exe().expect("test binary"))
        .env(CHILD, "1")
        .status()
        .expect("child");
    assert!(status.success(), "child: {status}");
}

#[test]
fn section_sizes_that_overflow_are_refused() {
    let huge = usize::MAX / 2 + 2;
    let error = anybytes::area::Section::<u16>::anonymous_allocated(huge).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let error = sections.reserve::<u64>(usize::MAX).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let error = sections.reserve_allocated::<u16>(huge).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[repr(C, align(1048576))]
#[derive(zerocopy::FromBytes, zerocopy::Immutable)]
struct OverAligned([u8; 1 << 20]);

#[test]
fn elements_aligned_beyond_a_page_are_refused() {
    let error = Section::<OverAligned>::anonymous_allocated(1)
        .err()
        .expect("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let error = sections.reserve::<OverAligned>(1).err().expect("refused");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

/// A 32-byte record whose all-zero bit pattern is invalid: it can be
/// written and validated, but never handed out as `&[T]` over a zero fill.
#[repr(C, align(16))]
#[derive(
    zerocopy::TryFromBytes,
    zerocopy::IntoBytes,
    zerocopy::KnownLayout,
    zerocopy::Immutable,
    Debug,
    PartialEq,
)]
struct NonZeroPair {
    a: core::num::NonZero<u128>,
    b: core::num::NonZero<u128>,
}

fn pair(a: u128, b: u128) -> NonZeroPair {
    NonZeroPair {
        a: core::num::NonZero::new(a).unwrap(),
        b: core::num::NonZero::new(b).unwrap(),
    }
}

#[test]
fn non_zero_sections_are_written_by_value_and_viewed_after_freeze() {
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();

    // A one-byte section first, so the pairs need alignment padding.
    let mut prefix = sections.reserve::<u8>(1).expect("reserve u8");
    prefix.write(0, &9);
    prefix.freeze().expect("freeze prefix");

    let mut section = sections.reserve::<NonZeroPair>(3).expect("reserve pairs");
    section.write(0, &pair(1, 2));
    section.write_slice(1, &[pair(3, 4), pair(u128::MAX, 5)]);
    let handle = section.handle();
    assert_eq!(handle.offset % core::mem::align_of::<NonZeroPair>(), 0);
    let bytes = section.freeze().expect("freeze");
    let expected = [pair(1, 2), pair(3, 4), pair(u128::MAX, 5)];
    assert_eq!(&*bytes.view::<[NonZeroPair]>().expect("view"), &expected);

    let mut allocated = sections
        .reserve_allocated::<NonZeroPair>(1)
        .expect("reserve allocated");
    allocated
        .as_mut_bytes()
        .copy_from_slice(zerocopy::IntoBytes::as_bytes(&pair(6, 7)));
    let allocated_handle = allocated.handle();
    allocated.freeze().expect("freeze allocated");

    drop(sections);
    let all = area.freeze().expect("freeze area");
    assert_eq!(&*handle.view(&all).expect("handle view"), &expected);
    assert_eq!(
        &*allocated_handle.view(&all).expect("allocated view"),
        &[pair(6, 7)]
    );

    let mut anonymous = Section::<NonZeroPair>::anonymous_allocated(2).expect("anonymous section");
    anonymous.write_slice(0, &[pair(8, 9), pair(10, 11)]);
    let bytes = anonymous.freeze().expect("freeze anonymous");
    assert_eq!(
        &*bytes.view::<[NonZeroPair]>().expect("anonymous view"),
        &[pair(8, 9), pair(10, 11)]
    );
}

#[test]
fn unwritten_non_zero_slot_fails_validation() {
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let mut section = sections.reserve::<NonZeroPair>(2).expect("reserve");
    section.write(0, &pair(1, 2));
    let handle = section.handle();
    let bytes = section.freeze().expect("freeze");
    assert!(bytes.view::<[NonZeroPair]>().is_err());
    drop(sections);
    let all = area.freeze().expect("freeze area");
    assert!(handle.view(&all).is_err());

    let section = Section::<NonZeroPair>::anonymous_allocated(1).expect("anonymous");
    let bytes = section.freeze().expect("freeze anonymous");
    assert!(bytes.view::<[NonZeroPair]>().is_err());
}

#[test]
fn from_bytes_sections_take_writes_by_value_too() {
    let mut section = Section::<u32>::anonymous_allocated(3).expect("anonymous section");
    section.write(2, &9);
    section.write_slice(0, &[7, 8]);
    assert_eq!(section.as_ref(), &[7, 8, 9]);
    section.as_mut_bytes()[..4].copy_from_slice(&1u32.to_ne_bytes());
    assert_eq!(&*section, &[1, 8, 9]);
}

#[test]
#[should_panic(expected = "overruns a section of 2")]
fn writes_past_the_end_panic() {
    let mut section = Section::<u16>::anonymous_allocated(2).expect("anonymous section");
    section.write_slice(1, &[1, 2]);
}

#[test]
#[should_panic(expected = "overruns a section of 2")]
fn writes_at_an_overflowing_slot_panic() {
    let mut section = Section::<u16>::anonymous_allocated(2).expect("anonymous section");
    section.write(usize::MAX, &1);
}
