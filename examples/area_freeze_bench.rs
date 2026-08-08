use std::hint::black_box;
use std::time::{Duration, Instant};

use anybytes::area::ByteArea;

const TINY_SECTIONS: usize = 10_000;
const LARGE_BYTES: usize = 64 * 1024 * 1024;
const LARGE_SECTIONS: usize = 4;
const TRIALS: usize = 7;

#[derive(Clone, Copy, Debug)]
struct Sample {
    total: Duration,
    freeze: Duration,
}

fn tiny(sync: bool) -> Sample {
    let started = Instant::now();
    let mut freeze = Duration::ZERO;
    let mut retained = Vec::with_capacity(TINY_SECTIONS);
    let mut area = ByteArea::new().expect("create byte area");
    let mut sections = area.sections();

    for value in 0..TINY_SECTIONS {
        let mut section = sections.reserve::<u8>(1).expect("reserve tiny section");
        section[0] = value as u8;

        let freeze_started = Instant::now();
        if sync {
            section.flush().expect("flush tiny section");
        }
        let bytes = section.freeze().expect("freeze tiny section");
        freeze += freeze_started.elapsed();
        black_box(bytes.as_ref()[0]);
        retained.push(bytes);
    }

    let total = started.elapsed();
    black_box(&retained);
    Sample { total, freeze }
}

fn large(sync: bool) -> Sample {
    let started = Instant::now();
    let mut freeze = Duration::ZERO;
    let mut retained = Vec::with_capacity(LARGE_SECTIONS);

    for value in 0..LARGE_SECTIONS {
        let mut area = ByteArea::new().expect("create byte area");
        let mut sections = area.sections();
        let mut section = sections
            .reserve::<u8>(LARGE_BYTES)
            .expect("reserve large section");
        section.as_mut_slice().fill(value as u8);

        let freeze_started = Instant::now();
        if sync {
            section.flush().expect("flush large section");
        }
        let bytes = section.freeze().expect("freeze large section");
        freeze += freeze_started.elapsed();
        black_box((bytes.as_ref()[0], bytes.as_ref()[LARGE_BYTES - 1]));
        retained.push(bytes);

        drop(sections);
        drop(area);
    }

    let total = started.elapsed();
    black_box(&retained);
    Sample { total, freeze }
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn run(label: &str, benchmark: fn(bool) -> Sample, sync: bool) {
    let warmup = benchmark(sync);
    black_box(warmup);

    let mut totals = Vec::with_capacity(TRIALS);
    let mut freezes = Vec::with_capacity(TRIALS);
    for _ in 0..TRIALS {
        let sample = benchmark(sync);
        totals.push(sample.total);
        freezes.push(sample.freeze);
    }

    println!(
        "{label}: median total {:?}, median freeze {:?}",
        median(&mut totals),
        median(&mut freezes),
    );
}

fn main() {
    run(&format!("tiny {TINY_SECTIONS} x 1 B, no sync"), tiny, false);
    run(&format!("tiny {TINY_SECTIONS} x 1 B, sync"), tiny, true);
    run(
        &format!(
            "large {LARGE_SECTIONS} x {} MiB, no sync",
            LARGE_BYTES / 1024 / 1024
        ),
        large,
        false,
    );
    run(
        &format!(
            "large {LARGE_SECTIONS} x {} MiB, sync",
            LARGE_BYTES / 1024 / 1024
        ),
        large,
        true,
    );
}
