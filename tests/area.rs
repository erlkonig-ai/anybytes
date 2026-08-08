#![cfg(all(feature = "mmap", feature = "zerocopy"))]

use anybytes::area::ByteArea;
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
fn persisted_area_observes_frozen_section_without_flush() {
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

#[test]
fn section_flush_is_an_explicit_persistence_barrier() {
    let mut area = ByteArea::new().expect("area");
    let mut sections = area.sections();
    let mut section = sections.reserve::<u8>(4).expect("reserve");
    section.copy_from_slice(&[9, 10, 11, 12]);
    section.flush().expect("flush");

    assert_eq!(section.freeze().expect("freeze").as_ref(), &[9, 10, 11, 12]);
}
